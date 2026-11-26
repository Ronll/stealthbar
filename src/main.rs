//! stealthbar: prints the state of a Turtle Beach Stealth Pro II headset as one JSON line per change.
//! See README.md for the protocol, the output format and how the Noctalia widget uses it.

mod race;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use race::{CMD_KVP, CMD_LINK, REPORT_IN, REPORT_LEN, Reassembler};
use serde_json::{Value, json};

const VENDOR: &str = "000010F5";
/// The vendor collection in the report descriptor: Usage Page 0xFF13.
const VENDOR_PAGE: [u8; 3] = [0x06, 0x13, 0xff];

/// How often an idle device is polled; the dock keeps unread bytes queued, so a slow poll loses nothing.
const IDLE_POLL: Duration = Duration::from_millis(250);
/// Polling right after an ask, while a reply arrives over several reports.
const BUSY_POLL: Duration = Duration::from_millis(20);
const BUSY_FOR: Duration = Duration::from_secs(2);
/// No answer to an ask within this time means the headset is off (or the dock is stuck).
const ANSWER_TIMEOUT: Duration = Duration::from_secs(3);
/// While on, re-ask now and then so a headset that went away silently is noticed.
const HEARTBEAT: Duration = Duration::from_secs(60);
/// While off, ask again this often to notice it coming on even without a link indication.
const RETRY_OFF: Duration = Duration::from_secs(10);
/// The headset drops subscriptions when it restarts; subscribe again this long after it links up.
const RESUBSCRIBE_AFTER_LINK: Duration = Duration::from_millis(1500);
const RESCAN: Duration = Duration::from_secs(2);

/// Records subscribed to: GSI has battery (0x240) and charging (0x250), Mic has mute (0x600).
const RECORDS: [&str; 2] = ["GSI", "Mic"];

#[derive(Clone, PartialEq, Default)]
struct State {
    present: bool,
    on: bool,
    battery: Option<u8>,
    charging: bool,
    mic_muted: bool,
    device: Option<String>,
    /// Raw mic keys: 0x640 source (1 boom, 2 built-in), 0x600 boom arm up, 0x650 built-in mute.
    mic: [u16; 3],
}

impl State {
    fn to_json(&self) -> Value {
        json!({
            "present": self.present,
            "on": self.on,
            "battery": self.battery,
            "charging": self.charging,
            "mic_muted": self.mic_muted,
            "device": self.device,
        })
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "watch".into());
    match mode.as_str() {
        "watch" => watch(false),
        "status" => watch(true),
        "-h" | "--help" | "help" => println!(
            "usage: stealthbar [watch|status]\n  watch   print a JSON line on every change (default)\n  status  print the current state once and exit"
        ),
        other => {
            eprintln!("stealthbar: unknown command {other:?}; try --help");
            std::process::exit(2);
        }
    }
}

fn watch(once: bool) {
    let mut out = Emitter {
        quiet: once,
        ..Emitter::default()
    };
    loop {
        match find_device() {
            Some((path, product)) => {
                if let Err(e) = run(&path, &product, &mut out, once) {
                    eprintln!("stealthbar: {}: {e}", path.display());
                }
            }
            None => out.emit(&State::default()),
        }
        if once {
            out.flush_once();
            return;
        }
        sleep(RESCAN);
    }
}

#[derive(Default)]
struct Emitter {
    last: Option<State>,
    /// `status` prints only the final state.
    quiet: bool,
}

impl Emitter {
    fn emit(&mut self, state: &State) {
        if self.last.as_ref() != Some(state) && !self.quiet {
            println!("{}", state.to_json());
            let _ = io::stdout().flush();
        }
        self.last = Some(state.clone());
    }

    fn flush_once(&mut self) {
        println!("{}", self.last.clone().unwrap_or_default().to_json());
    }
}

/// The hidraw node of the first Turtle Beach device with the vendor collection, and its product id.
fn find_device() -> Option<(PathBuf, String)> {
    let mut nodes: Vec<_> = fs::read_dir("/sys/class/hidraw")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    nodes.sort();
    nodes.into_iter().find_map(|node| {
        let uevent = fs::read_to_string(node.join("device/uevent")).ok()?;
        let id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
        let mut parts = id.split(':');
        let (_bus, vendor, product) = (parts.next()?, parts.next()?, parts.next()?);
        if !vendor.eq_ignore_ascii_case(VENDOR) {
            return None;
        }
        let descriptor = fs::read(node.join("device/report_descriptor")).ok()?;
        descriptor.windows(3).any(|w| w == VENDOR_PAGE).then(|| {
            (
                Path::new("/dev").join(node.file_name().unwrap()),
                product.trim_start_matches("0000").to_lowercase(),
            )
        })
    })
}

struct Hid {
    file: File,
}

impl Hid {
    fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?;
        Ok(Self { file })
    }

    /// Without an interrupt OUT endpoint the kernel sends this as a SET_REPORT control transfer.
    fn write(&mut self, report: &[u8]) -> io::Result<()> {
        self.file.write_all(report)
    }

    /// GET_REPORT(Input, 7): the dock hands out its queued bytes only this way, never on the interrupt endpoint.
    fn get_input(&mut self) -> io::Result<Vec<u8>> {
        // HIDIOCGINPUT(len) = _IOC(_IOC_READ | _IOC_WRITE, 'H', 0x0A, len)
        let request = (3u64 << 30) | ((REPORT_LEN as u64) << 16) | ((b'H' as u64) << 8) | 0x0A;
        let mut buf = [0u8; REPORT_LEN];
        buf[0] = REPORT_IN;
        let n = unsafe { libc::ioctl(self.file.as_raw_fd(), request as _, buf.as_mut_ptr()) };
        if n < 0 {
            let e = io::Error::last_os_error();
            // the dock fails a control transfer now and then while busy; treat it as an idle poll
            return match e.raw_os_error() {
                Some(libc::EPROTO | libc::EPIPE | libc::ETIMEDOUT | libc::EAGAIN | libc::EIO) => {
                    Ok(Vec::new())
                }
                _ => Err(e),
            };
        }
        Ok(buf[..(n as usize).min(REPORT_LEN)].to_vec())
    }

    /// Discard what the interrupt endpoint queued (media keys), so the kernel's buffer never fills.
    fn drain_interrupt(&mut self) {
        let mut buf = [0u8; 64];
        while matches!(self.file.read(&mut buf), Ok(n) if n > 0) {}
    }
}

fn run(path: &Path, product: &str, out: &mut Emitter, once: bool) -> io::Result<()> {
    let mut hid = Hid::open(path)?;
    let mut reasm = Reassembler::default();
    let mut state = State {
        present: true,
        device: Some(product.into()),
        ..State::default()
    };
    out.emit(&state);

    let mut token: u8 = 0;
    let mut asked_at: Option<Instant> = None;
    let mut answered = 0;
    let mut next_ask = Some(Instant::now());
    let mut busy_until = Instant::now();

    loop {
        let now = Instant::now();
        if next_ask.is_some_and(|t| now >= t) {
            for record in RECORDS {
                token = token.wrapping_add(1);
                hid.write(&race::subscribe(record, token))?;
            }
            asked_at = Some(now);
            answered = 0;
            next_ask = None;
            busy_until = now + BUSY_FOR;
        }

        hid.drain_interrupt();
        let report = hid.get_input()?;
        for packet in reasm.feed(&report) {
            match packet.cmd {
                CMD_LINK if packet.payload.len() > 2 => {
                    if packet.payload[2] == 1 {
                        next_ask = Some(Instant::now() + RESUBSCRIBE_AFTER_LINK);
                    } else {
                        state.on = false;
                    }
                }
                CMD_KVP => {
                    for doc in race::documents(&packet.payload) {
                        if doc.get("OR").is_some() {
                            answered += 1;
                        }
                        state.on = true;
                        apply(&mut state, &doc["KVP"]);
                    }
                }
                _ => {}
            }
        }

        if let Some(at) = asked_at {
            if answered >= RECORDS.len() {
                asked_at = None;
                next_ask.get_or_insert(Instant::now() + HEARTBEAT);
            } else if now.duration_since(at) > ANSWER_TIMEOUT {
                asked_at = None;
                // nothing at all: the headset is off; a partial answer: it is on, ask again soon
                state.on = answered > 0;
                next_ask.get_or_insert(Instant::now() + RETRY_OFF);
            }
        }
        out.emit(&state);
        if once && asked_at.is_none() {
            return Ok(());
        }

        let busy = asked_at.is_some()
            || Instant::now() < busy_until
            || report.get(1).is_some_and(|&n| n > 0);
        sleep(if busy { BUSY_POLL } else { IDLE_POLL });
    }
}

/// Take the keys the widget shows from a document's key/value map.
fn apply(state: &mut State, kvp: &Value) {
    let number = |key: &str| {
        kvp.get(key)
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u16>().ok())
    };
    if let Some(b) = number("240") {
        // 255 = not measured yet (battery just swapped)
        state.battery = (b <= 100).then_some(b as u8);
    }
    if let Some(c) = number("250") {
        state.charging = c != 0;
    }
    for (i, key) in ["640", "600", "650"].into_iter().enumerate() {
        if let Some(v) = number(key) {
            state.mic[i] = v;
        }
    }
    let [source, boom_up, builtin_muted] = state.mic;
    state.mic_muted = if source == 2 {
        builtin_muted != 0
    } else {
        boom_up != 0
    };
}
