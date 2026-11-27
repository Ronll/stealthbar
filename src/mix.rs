//! Game/chat mix in PipeWire: the headset gets one stereo stream from a PC, so it can't mix game against chat
//! itself. The wheel only reports a position (key 0x510, 0 = game only, 50 = centre, 100 = chat only) and the
//! PC applies it. Same curve as Neap: the centre leaves both at full level, moving off centre lowers the side
//! being moved away from.

use std::collections::HashSet;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::config::ChatMix;

/// The wheel is a free-spinning encoder: the headset reports a 0-100 count of notches that is not where the
/// wheel physically sits (it stops at 0 and 100 while the wheel keeps turning). So only the movement between two
/// readings moves the mix, which is kept here, starting at the centre. Stray readings happen (Neap measured one
/// notch reading 10 straight after 50): the first reading after the wheel was at rest that jumps more than a
/// notch's worth is held until the next reading confirms it.
pub struct Wheel {
    pub mix: u8,
    last: Option<i64>,
    seen_at: Option<Instant>,
    doubted: Option<i64>,
}

const REST: Duration = Duration::from_millis(500);
const FIRST_NOTCH_LIMIT: i64 = 15;

impl Default for Wheel {
    fn default() -> Self {
        Self {
            mix: 50,
            last: None,
            seen_at: None,
            doubted: None,
        }
    }
}

impl Wheel {
    /// Forget the count (after a reconnect the next reading is only a starting point); the mix stays.
    pub fn restart(&mut self) {
        self.last = None;
        self.doubted = None;
    }

    pub fn started(&self) -> bool {
        self.last.is_some()
    }

    /// Take a reading of the count; true when the mix moved.
    pub fn read(&mut self, count: i64, now: Instant) -> bool {
        let rested = self.seen_at.is_none_or(|t| now.duration_since(t) >= REST);
        self.seen_at = Some(now);
        let near = |a: i64, b: i64| (a - b).abs() <= FIRST_NOTCH_LIMIT;
        let Some(mut last) = self.last else {
            self.last = Some(count);
            return false;
        };
        if let Some(doubted) = self.doubted.take() {
            if near(count, doubted) {
                last = doubted; // the count really moved while we weren't looking
            } else if !near(count, last) {
                self.last = Some(count); // neither fits: start over from here
                return false;
            } // else: the far reading was stray, continue from `last`
        } else if rested && !near(count, last) {
            self.doubted = Some(count);
            return false;
        }
        self.last = Some(count);
        let old = i64::from(self.mix);
        let mut mix = (old + count - last).clamp(0, 100);
        // centre detent (as Neap): a move onto or across 50 stops there, so "both at full" is easy to hit
        if (old < 50 && mix > 50) || (old > 50 && mix < 50) {
            mix = 50;
        }
        let mix = mix as u8;
        let moved = mix != self.mix;
        self.mix = mix;
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
    fn wheel_moves_by_steps_and_skips_strays() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut w = Wheel::default();
        assert!(!w.read(80, ms(0))); // starting point only
        assert!(w.read(74, ms(100)) && w.mix == 44);
        // crossing the centre stops at 50, the next notch moves on
        let mut c = Wheel::default();
        c.read(0, ms(0));
        assert!(c.read(5, ms(100)) && c.mix == 55);
        assert!(c.read(-5, ms(200)) && c.mix == 50);
        assert!(c.read(-10, ms(300)) && c.mix == 45);
        assert!(w.read(68, ms(200)) && w.mix == 38);
        // after a rest, a far jump is held, then dropped when the next reading is near the old count
        assert!(!w.read(10, ms(2000)));
        assert!(w.read(65, ms(2100)) && w.mix == 35);
        // after a rest, a far jump confirmed by the next reading becomes the new starting point
        assert!(!w.read(20, ms(4000)));
        assert!(w.read(17, ms(4100)) && w.mix == 32);
        // clamps
        assert!(w.read(0, ms(4200)) && w.mix == 15);
        w.restart();
        assert!(!w.read(90, ms(9000)) && w.mix == 15);
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
