//! The running `watch` is the dock's only reader (a second reader takes replies off the same queue), so `set`,
//! `status` and `raw` go to it over a Unix socket: `$XDG_RUNTIME_DIR/stealthbar.sock`, or `/tmp/stealthbar-<uid>.sock`.
//! Protocol: one line in (`status`, `raw`, `set <setting> <value…>`), one line out (JSON, `ok`, or `error: …`).

use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

pub enum Cmd {
    Status,
    Raw,
    Set(String, Vec<String>),
}

pub fn path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("stealthbar.sock"),
        None => PathBuf::from(format!("/tmp/stealthbar-{}.sock", unsafe {
            libc::getuid()
        })),
    }
}

/// Bind the socket; refuses when another `watch` is already serving it (two readers would fight over the dock).
pub fn listen() -> Result<UnixListener, String> {
    let path = path();
    if UnixStream::connect(&path).is_ok() {
        return Err(format!(
            "another `stealthbar watch` is running ({})",
            path.display()
        ));
    }
    let _ = std::fs::remove_file(&path); // stale socket of a daemon that died
    let listener = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    Ok(listener)
}

/// Sleep for `timeout`, waking early when a client connects.
pub fn wait(listener: Option<&UnixListener>, timeout: Duration) {
    let Some(l) = listener else {
        std::thread::sleep(timeout);
        return;
    };
    let mut fd = libc::pollfd {
        fd: l.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    unsafe { libc::poll(&mut fd, 1, ms) };
}

/// Answer every waiting client with `handle`.
pub fn serve(listener: &UnixListener, mut handle: impl FnMut(Cmd) -> Result<String, String>) {
    while let Ok((stream, _)) = listener.accept() {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        let words: Vec<String> = line.split_whitespace().map(String::from).collect();
        let reply = match words.first().map(String::as_str) {
            Some("status") => handle(Cmd::Status),
            Some("raw") => handle(Cmd::Raw),
            Some("set") if words.len() >= 3 => {
                handle(Cmd::Set(words[1].clone(), words[2..].to_vec()))
            }
            _ => Err("expected: status | raw | set <setting> <value…>".into()),
        };
        let reply = reply.unwrap_or_else(|e| format!("error: {e}"));
        let _ = (&stream).write_all(format!("{reply}\n").as_bytes());
    }
}

/// Send one command to the running `watch` and return its reply line.
pub fn request(line: &str) -> io::Result<String> {
    let mut stream = UnixStream::connect(path())?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.write_all(format!("{line}\n").as_bytes())?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply)?;
    Ok(reply.trim_end().to_owned())
}
