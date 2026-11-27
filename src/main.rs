//! stealthbar: prints the state of a Turtle Beach Stealth Pro II headset as one JSON line per change.
//! See README.md for the protocol, the output format, the config file and how the Noctalia plugin uses it.

mod config;
mod decode;
mod mix;
mod race;

use std::collections::{HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use decode::Keys;
use race::{CMD_KVP, CMD_LINK, REPORT_IN, REPORT_LEN, Reassembler};
use serde_json::{Value, json};

const VENDOR: &str = "000010F5";
/// The vendor collection in the report descriptor: Usage Page 0xFF13.
const VENDOR_PAGE: [u8; 3] = [0x06, 0x13, 0xff];

/// How often an idle dock is polled. It keeps unread bytes queued, so a slow poll loses nothing; it only
/// delays how soon a change shows. Slower while the headset is off: only a link-up can arrive then.
const IDLE_POLL_ON: Duration = Duration::from_millis(500);
const IDLE_POLL_OFF: Duration = Duration::from_secs(2);
/// Polling right after an ask, while a reply arrives over several reports.
const BUSY_POLL: Duration = Duration::from_millis(15);
/// Requests go out one at a time: the next only after the reply (or this long without one; an empty
/// transmitter slot never answers). Pipelining them overflows the dock's reply queue and loses records.
const REPLY_WAIT: Duration = Duration::from_millis(600);
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
/// Re-apply the chat mix this often, for chat streams that appeared since the wheel last moved.
const MIX_REFRESH: Duration = Duration::from_secs(5);

/// Records the headset answers (each owns a key range: Inf 1xx, GSI 2xx, 3DT 5xx, Mic 6xx, SAF 7xx, Enc axx,
/// Btn bxx, AQG 12xx, AQM 13xx). GSI answering is what "on" means.
const RECORDS: [&str; 9] = [
    "GSI", "Inf", "Mic", "SAF", "3DT", "Enc", "Btn", "AQG", "AQM",
];
/// Transmitter slots: an empty slot never answers, so these are asked but not waited for.
const SLOTS: [&str; 4] = ["TX1", "TX2", "TX3", "TX4"];

#[derive(Default)]
struct State {
    present: bool,
    on: bool,
    device: Option<String>,
    keys: Keys,
}

impl State {
    fn to_json(&self) -> Value {
        let k = &self.keys;
        json!({
            "present": self.present,
            "on": self.on,
            "battery": decode::battery(k),
            "charging": decode::charging(k),
            "mic_muted": decode::mic_muted(k),
            "device": self.device,
            "details": if self.on { decode::details(k) } else { Value::Null },
        })
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "watch".into());
    match mode.as_str() {
        "watch" => watch(),
        "status" => once(false),
        "raw" => once(true),
        "config-path" => println!(
            "{}",
            config::path()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        ),
        "-h" | "--help" | "help" => println!(
            "usage: stealthbar [watch|status|raw|config-path]\n  \
             watch        print a JSON line on every change (default)\n  \
             status       print the current state once and exit\n  \
             raw          print every raw key the headset reported, once\n  \
             config-path  print where the config file is read from"
        ),
        other => {
            eprintln!("stealthbar: unknown command {other:?}; try --help");
            std::process::exit(2);
        }
    }
}

fn watch() {
    let mut out = Emitter::default();
    let cfg = config::load();
    loop {
        match find_device() {
            Some((path, product)) => {
                if let Err(e) = run(&path, &product, &mut out, &cfg, false) {
                    eprintln!("stealthbar: {}: {e}", path.display());
                }
            }
            None => out.emit(&State::default()),
        }
        sleep(RESCAN);
    }
}

/// `status` / `raw`: read everything once and print it. Never writes to the headset (no chat mix).
fn once(raw: bool) {
    let mut out = Emitter {
        quiet: true,
        ..Emitter::default()
    };
    let cfg = config::Config::default();
    if let Some((path, product)) = find_device()
        && let Err(e) = run(&path, &product, &mut out, &cfg, true)
    {
        eprintln!("stealthbar: {}: {e}", path.display());
    }
    let last = if raw { out.last_raw } else { out.last };
    println!("{}", last.unwrap_or_else(|| State::default().to_json()));
}

#[derive(Default)]
struct Emitter {
    last: Option<Value>,
    last_raw: Option<Value>,
    /// `status`/`raw` print only the final state.
    quiet: bool,
}

impl Emitter {
    fn emit(&mut self, state: &State) {
        let value = state.to_json();
        if self.last.as_ref() != Some(&value) && !self.quiet {
            println!("{value}");
            let _ = io::stdout().flush();
        }
        self.last = Some(value);
        self.last_raw = Some(json!(state.keys));
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

fn run(
    path: &Path,
    product: &str,
    out: &mut Emitter,
    cfg: &config::Config,
    once: bool,
) -> io::Result<()> {
    let mut hid = Hid::open(path)?;
    let mut reasm = Reassembler::default();
    let mut state = State {
        present: true,
        device: Some(product.into()),
        ..State::default()
    };
    out.emit(&state);

    let mut token: u8 = 0;
    let mut outbox: VecDeque<[u8; REPORT_LEN]> = VecDeque::new();
    let mut awaiting: Option<Instant> = None;
    let mut asked_at: Option<Instant> = None;
    let mut answered: HashSet<String> = HashSet::new();
    let mut next_ask = Some(Instant::now());
    let mut busy_until = Instant::now();
    // per connection: the chat-mix writes (enable wheel reports, wheel job) are sent once all records are in
    let mut mix_setup_done = false;
    let mut last_mix: Option<u8> = None;
    let mut mix_applied_at = Instant::now();
    let chat = &cfg.chat_mix;

    loop {
        let now = Instant::now();
        if next_ask.is_some_and(|t| now >= t) {
            for record in RECORDS.iter().chain(&SLOTS) {
                token = token.wrapping_add(1);
                outbox.push_back(race::subscribe(record, token));
            }
            asked_at = Some(now);
            answered.clear();
            next_ask = None;
        }
        // one request per poll, so the dock's small receive queue never overflows
        if awaiting.is_none_or(|t| now.duration_since(t) > REPLY_WAIT)
            && let Some(frame) = outbox.pop_front()
        {
            awaiting = Some(now);
            hid.write(&frame)?;
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
                    state.keys.remove("510");
                    mix_setup_done = false;
                }
                CMD_KVP => {
                    if packet.kind == 0x5b {
                        awaiting = None;
                    }
                    for doc in race::documents(&packet.payload) {
                        if let Some(record) = doc.get("OR").and_then(Value::as_str) {
                            answered.insert(record.to_owned());
                        }
                        if let Some(kvp) = doc["KVP"].as_object() {
                            state.on = true;
                            // a read answers 0x510 = 0 before the wheel has moved (a real 0 means game
                            // only): take the wheel position only from pushes, which come when it turns
                            let pushed = doc.get("UP").is_some();
                            state.keys.extend(
                                kvp.iter()
                                    .filter(|(k, _)| pushed || *k != "510")
                                    .map(|(k, v)| (k.clone(), v.clone())),
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        let all_in = RECORDS.iter().all(|r| answered.contains(*r));
        if let Some(at) = asked_at {
            if all_in {
                asked_at = None;
                next_ask.get_or_insert(Instant::now() + HEARTBEAT);
            } else if now.duration_since(at) > ANSWER_TIMEOUT {
                asked_at = None;
                // nothing at all: the headset is off; a partial answer: it is on, ask again soon
                state.on = answered.contains("GSI");
                next_ask.get_or_insert(Instant::now() + RETRY_OFF);
            }
        }
        if !state.on {
            mix_setup_done = false;
        }

        if chat.enabled && state.on {
            if !mix_setup_done && all_in {
                // the headset reports the wheel (0x510) only while 0x500 is set, and clears 0x500 at every connect
                token = token.wrapping_add(1);
                outbox.push_back(race::set_kvp("500", "1", token));
                let wheel = state.keys.get("a20").and_then(Value::as_str);
                if chat.set_wheel && wheel != Some("2") {
                    token = token.wrapping_add(1);
                    outbox.push_back(race::set_kvp("a20", "2", token));
                }
                mix_setup_done = true;
            }
            let mix = decode::chat_mix(&state.keys);
            if let Some(m) = mix
                && (mix != last_mix || now.duration_since(mix_applied_at) > MIX_REFRESH)
            {
                mix::apply(chat, m);
                last_mix = mix;
                mix_applied_at = Instant::now();
            }
        }

        out.emit(&state);
        if once
            && asked_at.is_none()
            && outbox.is_empty()
            && awaiting.is_none_or(|t| now.duration_since(t) > REPLY_WAIT)
        {
            return Ok(());
        }

        let busy = !outbox.is_empty()
            || awaiting.is_some_and(|t| now.duration_since(t) <= REPLY_WAIT)
            || asked_at.is_some()
            || Instant::now() < busy_until
            || report.get(1).is_some_and(|&n| n > 0);
        sleep(if busy {
            BUSY_POLL
        } else if state.on {
            IDLE_POLL_ON
        } else {
            IDLE_POLL_OFF
        });
    }
}
