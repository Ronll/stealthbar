//! Game/chat mix in PipeWire: the headset gets one stereo stream from a PC, so it can't mix game against chat
//! itself. The wheel only reports a position (key 0x510, 0 = game only, 50 = centre, 100 = chat only) and the
//! PC applies it. Same curve as Neap: the centre leaves both at full level, moving off centre lowers the side
//! being moved away from.

use std::collections::HashSet;
use std::process::Command;

use serde_json::Value;

use crate::config::ChatMix;

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
