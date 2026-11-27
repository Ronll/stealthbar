//! Optional (`[audio] follow_headset`): make the dock's sound card disappear from PipeWire while the headset is
//! off, the way a Bluetooth headset does when it disconnects. The dock's card exists whenever the dock is plugged
//! in, so on its own PipeWire keeps it as the default output even with the headset off.
//!
//! While the headset is off the card's profile is set to "off" (its sink and source go away), and back when it
//! turns on. WirePlumber then picks the default itself from the user's history of chosen devices: the next one
//! while the headset is off, the headset again when it is back (if it was the chosen one). stealthbar never sets
//! the default, so the user's own choices stay in charge.
//!
//! Every switch is sent with `save: false`, so WirePlumber does not store "off" in its default-profile state: after a
//! reboot, replug or WirePlumber restart the card comes back with its normal profile, and stealthbar only turns it
//! off again while it sees the headset off. (`wpctl set-profile` and pavucontrol always save, so they are not
//! used.) WirePlumber can re-select the normal profile on its own (restart, replug, a profile change on any card),
//! so while the headset is off the profile is checked again every `RECHECK`. Needs `pw-dump` and `pw-cli`.

use std::process::Command;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::Value;

const RECHECK: Duration = Duration::from_secs(10);

/// The thread that owns the dock's profile. Send `true`/`false` as the headset turns on/off; dropping the
/// follower (daemon exiting, dock gone) restores the normal profile if it was turned off.
pub struct Follower {
    tx: Option<Sender<bool>>,
    thread: Option<JoinHandle<()>>,
}

impl Follower {
    pub fn spawn(product: &str) -> Self {
        let (tx, rx) = channel();
        let product = format!("0x{product}");
        let thread = std::thread::spawn(move || follow(&product, rx));
        Self {
            tx: Some(tx),
            thread: Some(thread),
        }
    }

    pub fn headset(&self, on: bool) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(on);
        }
    }
}

impl Drop for Follower {
    fn drop(&mut self) {
        self.tx = None; // ends the thread's loop
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn follow(product: &str, rx: Receiver<bool>) {
    // the normal profile to come back to, remembered when stealthbar turned the card off
    let mut restore: Option<u64> = None;
    let mut headset_on: Option<bool> = None;
    loop {
        match rx.recv_timeout(RECHECK) {
            Ok(true) => {
                headset_on = Some(true);
                turn_on(product, &mut restore);
            }
            Ok(false) => {
                headset_on = Some(false);
                turn_off(product, &mut restore);
            }
            // only while off: WirePlumber may have brought the normal profile back
            Err(RecvTimeoutError::Timeout) if headset_on == Some(false) => {
                turn_off(product, &mut restore)
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                if restore.is_some() {
                    turn_on(product, &mut restore);
                }
                return;
            }
        }
    }
}

fn turn_off(product: &str, restore: &mut Option<u64>) {
    let Some(card) = find(product) else { return };
    if card.current_name.as_deref() == Some("off") {
        return;
    }
    let Some(off) = card.off else { return };
    if card.current.is_some() {
        *restore = card.current;
    }
    set_profile(card.id, off);
}

/// Back to the remembered profile, or the best available one when the card is off for another reason (e.g. a
/// stealthbar that was killed while the headset was off). Only acts on a card that is off.
fn turn_on(product: &str, restore: &mut Option<u64>) {
    let Some(card) = find(product) else { return };
    if card.current_name.as_deref() == Some("off")
        && let Some(index) = restore.take().or(card.best)
    {
        set_profile(card.id, index);
    }
    *restore = None;
}

fn set_profile(device: u64, index: u64) {
    let _ = Command::new("pw-cli")
        .args([
            "set-param",
            &device.to_string(),
            "Profile",
            &format!("{{ index: {index}, save: false }}"),
        ])
        .output();
}

#[derive(Debug, PartialEq)]
struct Card {
    id: u64,
    current: Option<u64>,
    current_name: Option<String>,
    /// index of the "off" profile
    off: Option<u64>,
    /// the available profile with the highest priority, other than "off" and "pro-audio"
    best: Option<u64>,
}

fn find(product: &str) -> Option<Card> {
    let out = Command::new("pw-dump").output().ok()?;
    let objects: Vec<Value> = serde_json::from_slice(&out.stdout).ok()?;
    card(&objects, product)
}

/// The dock's PipeWire device (vendor 0x10f5, this product id) and its profiles.
fn card(objects: &[Value], product: &str) -> Option<Card> {
    let device = objects.iter().find(|o| {
        let p = &o["info"]["props"];
        o["type"] == "PipeWire:Interface:Device"
            && p["device.vendor.id"] == "0x10f5"
            && p["device.product.id"].as_str() == Some(product)
    })?;
    let params = &device["info"]["params"];
    let profiles = params["EnumProfile"].as_array()?;
    let current = params["Profile"].as_array().and_then(|p| p.first());
    let off = profiles
        .iter()
        .find(|p| p["name"] == "off")
        .and_then(|p| p["index"].as_u64());
    let best = profiles
        .iter()
        .filter(|p| p["name"] != "off" && p["name"] != "pro-audio" && p["available"] != "no")
        .max_by_key(|p| p["priority"].as_u64().unwrap_or(0))
        .and_then(|p| p["index"].as_u64());
    Some(Card {
        id: device["id"].as_u64()?,
        current: current.and_then(|p| p["index"].as_u64()),
        current_name: current.and_then(|p| p["name"].as_str()).map(str::to_owned),
        off,
        best,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_the_docks_profiles() {
        let dump = json!([
            {"id": 46, "type": "PipeWire:Interface:Device",
             "info": {"props": {"device.vendor.id": "0x8086", "device.product.id": "0x2283"}, "params": {}}},
            {"id": 159, "type": "PipeWire:Interface:Device",
             "info": {"props": {"device.vendor.id": "0x10f5", "device.product.id": "0x2283"},
                      "params": {
                          "EnumProfile": [
                              {"index": 0, "name": "off", "available": "yes", "priority": 0},
                              {"index": 1, "name": "output:analog-stereo+input:mono-fallback", "available": "yes", "priority": 6501},
                              {"index": 2, "name": "output:analog-stereo", "available": "yes", "priority": 6500},
                              {"index": 3, "name": "output:iec958-stereo", "available": "no", "priority": 9000},
                              {"index": 4, "name": "pro-audio", "available": "unknown", "priority": 1}
                          ],
                          "Profile": [{"index": 2, "name": "output:analog-stereo", "save": false}]
                      }}}
        ]);
        let objects = dump.as_array().unwrap();
        assert_eq!(
            card(objects, "0x2283"),
            Some(Card {
                id: 159,
                current: Some(2),
                current_name: Some("output:analog-stereo".into()),
                off: Some(0),
                best: Some(1),
            })
        );
        assert_eq!(card(objects, "0x2286"), None);
    }
}
