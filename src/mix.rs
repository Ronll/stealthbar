//! Game/chat mix in PipeWire: the headset gets one stereo stream from a PC, so it can't mix game against chat
//! itself. The wheel only reports a position (key 0x510, 0 = game only, 50 = centre, 100 = chat only) and the
//! PC applies it. Same curve as Neap: the centre leaves both at full level, moving off centre lowers the side
//! being moved away from.

use std::collections::HashSet;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::config::ChatMix;

/// The wheel's position as the headset reports it in pushes of key 0x510: an absolute 0-100 mix the headset keeps
/// itself, clamped at 0 and 100. Measured on a 2283 dock: a slow notch moves it by 5, a normal one by 10, a fast
/// spin by 20-30 per report; nothing arrives while the wheel is still. Because each report is absolute, a lost
/// report doesn't matter: the next one is right again. A *read* of 0x510 (and the first value after reports are
/// switched on) is a stale 0, so only pushes are used, and `mix` is None until the first push.
#[derive(Default)]
pub struct Wheel {
    pub mix: Option<u8>,
    seen_at: Option<Instant>,
    /// NEAP WORKAROUND: a far first reading after rest, waiting for confirmation (value, when it arrived)
    doubted: Option<(u8, Instant)>,
}

// NEAP WORKAROUND (stray first reading after rest), from Neap's ChatWheel
// (github.com/lykosapps/Neap, src/Neap.Core/Mix/ChatWheel.cs). Neap measured that the first reading after the
// wheel had been still can be far off: one notch read 10 straight after 50, and their mix fell from 55 to 11.
// So a first reading after REST that is more than FIRST_NOTCH_LIMIT away is held: the next reading replaces it
// (the wheel's value is absolute, so the next one is right either way), and if none comes within CONFIRM_AFTER
// it is taken as real. Not seen on our 2283 dock (its big jumps were fast spins, which arrive mid-turn, not
// after rest); kept because it costs at most a short delay. Drop it if it never triggers in practice.
const REST: Duration = Duration::from_millis(500);
const FIRST_NOTCH_LIMIT: i64 = 15;
const CONFIRM_AFTER: Duration = Duration::from_millis(700);

impl Wheel {
    /// Take a pushed reading; true when the mix moved.
    pub fn read(&mut self, value: i64, now: Instant) -> bool {
        let value = value.clamp(0, 100) as u8;
        let rested = self.seen_at.is_none_or(|t| now.duration_since(t) >= REST);
        self.seen_at = Some(now);
        // NEAP WORKAROUND, see REST / FIRST_NOTCH_LIMIT above
        if self.doubted.take().is_none()
            && rested
            && let Some(mix) = self.mix
            && (i64::from(value) - i64::from(mix)).abs() > FIRST_NOTCH_LIMIT
        {
            self.doubted = Some((value, now));
            return false;
        }
        self.set(value)
    }

    /// Call regularly: takes a held reading as real once nothing contradicted it. True when the mix moved.
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.doubted {
            Some((value, at)) if now.duration_since(at) >= CONFIRM_AFTER => {
                self.doubted = None;
                self.set(value)
            }
            _ => false,
        }
    }

    fn set(&mut self, value: u8) -> bool {
        let mut mix = value;
        // centre detent (as Neap): a move across 50 stops at 50 for one report, so "both at full" is easy to hit
        if let Some(old) = self.mix
            && ((old < 50 && mix > 50) || (old > 50 && mix < 50))
        {
            mix = 50;
        }
        let moved = self.mix != Some(mix);
        self.mix = Some(mix);
        moved
    }
}

pub fn chat_gain(mix: u8) -> f64 {
    (2.0 * f64::from(mix.min(100)) / 100.0).min(1.0)
}

pub fn game_gain(mix: u8) -> f64 {
    (2.0 * (1.0 - f64::from(mix.min(100)) / 100.0)).min(1.0)
}

/// Set playback stream volumes: chat apps to the chat gain, the rest to the game gain if `duck_game`.
/// With `all` (the wheel moved) every matching stream is set; otherwise only streams not in `seen` (new ones),
/// so volume changes made by other tools in between are kept. Needs `pw-dump` and `wpctl` (PipeWire).
pub fn apply(cfg: &ChatMix, mix: u8, seen: &mut HashSet<u64>, all: bool) {
    let Ok(out) = Command::new("pw-dump").output() else {
        eprintln!("stealthbar: chat mix needs pw-dump (PipeWire)");
        return;
    };
    let Ok(Value::Array(objects)) = serde_json::from_slice::<Value>(&out.stdout) else {
        return;
    };
    for o in &objects {
        let props = &o["info"]["props"];
        if props["media.class"] != "Stream/Output/Audio" {
            continue;
        }
        let target = if is_chat(cfg, props) {
            chat_gain(mix)
        } else if cfg.duck_game {
            game_gain(mix)
        } else {
            continue;
        };
        let (Some(id), Some(current)) = (o["id"].as_u64(), volume(o)) else {
            continue;
        };
        if !seen.insert(id) && !all {
            continue;
        }
        if (current - target).abs() > 0.005 {
            let _ = Command::new("wpctl")
                .args(["set-volume", &id.to_string(), &format!("{target:.3}")])
                .status();
        }
    }
}

fn is_chat(cfg: &ChatMix, props: &Value) -> bool {
    let fields = [
        "application.name",
        "application.process.binary",
        "node.name",
    ];
    fields.iter().filter_map(|f| props[*f].as_str()).any(|v| {
        let v = v.to_lowercase();
        cfg.apps.iter().any(|a| v.contains(&a.to_lowercase()))
    })
}

/// A stream's linear volume as wpctl sets it (the cube of channelVolumes, which PipeWire stores cubed).
fn volume(o: &Value) -> Option<f64> {
    let params = o["info"]["params"]["Props"].as_array()?;
    let p = params.iter().find(|p| p["channelVolumes"].is_array())?;
    let v = p["channelVolumes"]
        .as_array()?
        .iter()
        .filter_map(Value::as_f64)
        .reduce(f64::max)?;
    Some(v.cbrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve() {
        assert_eq!((chat_gain(50), game_gain(50)), (1.0, 1.0));
        assert_eq!((chat_gain(0), game_gain(0)), (0.0, 1.0));
        assert_eq!((chat_gain(100), game_gain(100)), (1.0, 0.0));
        assert_eq!(chat_gain(25), 0.5);
    }

    #[test]
    fn wheel_follows_pushes_with_detent_and_neap_guard() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut w = Wheel::default();
        assert!(w.mix.is_none());
        assert!(w.read(100, ms(0)) && w.mix == Some(100)); // first push is absolute
        assert!(w.read(90, ms(100)) && w.mix == Some(90));
        assert!(w.read(70, ms(200)) && w.mix == Some(70)); // fast spin mid-turn: taken
        assert!(w.read(40, ms(300)) && w.mix == Some(50)); // crossing 50 stops there
        assert!(w.read(35, ms(400)) && w.mix == Some(35));
        // after a rest, a far reading is held; the next reading replaces it
        assert!(!w.read(90, ms(3000)) && w.mix == Some(35));
        assert!(w.read(40, ms(3100)) && w.mix == Some(40));
        // a held reading nothing contradicts is taken after CONFIRM_AFTER
        assert!(!w.read(10, ms(6000)));
        assert!(!w.tick(ms(6300)));
        assert!(w.tick(ms(6800)) && w.mix == Some(10));
    }

    #[test]
    fn matches_discord() {
        let cfg = ChatMix::default();
        assert!(is_chat(
            &cfg,
            &serde_json::json!({"application.name": "WEBRTC VoiceEngine"})
        ));
        assert!(is_chat(
            &cfg,
            &serde_json::json!({"application.process.binary": ".Discord-wrapped"})
        ));
        assert!(!is_chat(
            &cfg,
            &serde_json::json!({"application.name": "Firefox"})
        ));
    }
}
