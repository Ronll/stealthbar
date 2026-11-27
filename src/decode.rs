//! Turns the headset's raw keys into the `details` object. Key meanings come from Neap's FINDINGS.md and
//! swarm2linux's docs/protocol.md (measured there on the same firmware family).

use std::collections::BTreeMap;

use serde_json::{Value, json};

pub type Keys = BTreeMap<String, Value>;

fn num(keys: &Keys, key: &str) -> Option<i64> {
    keys.get(key)?.as_str()?.trim().parse().ok()
}

fn text(keys: &Keys, key: &str) -> Option<String> {
    keys.get(key)?.as_str().map(str::to_owned)
}

fn flag(keys: &Keys, key: &str) -> Option<bool> {
    num(keys, key).map(|v| v != 0)
}

pub fn battery(keys: &Keys) -> Option<u8> {
    // 255 = not measured yet (battery just swapped)
    num(keys, "240")
        .filter(|b| (0..=100).contains(b))
        .map(|b| b as u8)
}

pub fn charging(keys: &Keys) -> bool {
    flag(keys, "250").unwrap_or(false)
}

/// Boom arm up (0x600) while the boom mic is in use (0x640 = 1), else the built-in mics' mute (0x650).
pub fn mic_muted(keys: &Keys) -> bool {
    if num(keys, "640") == Some(2) {
        flag(keys, "650")
    } else {
        flag(keys, "600")
    }
    .unwrap_or(false)
}

/// The wheel position, only while reports are switched on (0x500 = 1); otherwise 0x510 is stale.
pub fn chat_mix(keys: &Keys) -> Option<u8> {
    if num(keys, "500") != Some(1) {
        return None;
    }
    num(keys, "510")
        .filter(|m| (0..=100).contains(m))
        .map(|m| m as u8)
}

fn wheel_job(v: Option<i64>) -> Option<&'static str> {
    Some(match v? {
        1 => "mic monitoring",
        2 => "game/chat mix",
        3 => "bass boost",
        4 => "treble boost",
        5 => "noise gate",
        _ => return None,
    })
}

fn button_job(v: Option<i64>) -> Option<&'static str> {
    Some(match v? {
        0 => "noise cancellation",
        1 => "next game preset",
        2 => "noise gate",
        _ => return None,
    })
}

fn preset(keys: &Keys, id_key: &str, name_key: &str) -> Option<String> {
    match num(keys, id_key)? {
        0 => Some("custom (unsaved)".into()),
        _ => text(keys, name_key).filter(|n| !n.is_empty()),
    }
}

/// The paired transmitter the headset has selected (`info[0]` = 2) among TX1..TX4 (keys 400/420/440/460).
fn transmitter(keys: &Keys) -> Option<Value> {
    ["400", "420", "440", "460"].iter().find_map(|k| {
        let info = keys.get(*k)?["info"].as_array()?;
        let s = |i: usize| info.get(i).and_then(Value::as_str);
        (s(0) == Some("2")).then(|| {
            let spare = s(3).and_then(|v| v.parse::<i64>().ok()).filter(|v| (1..=100).contains(v));
            json!({ "product": s(6).map(str::to_lowercase), "firmware": s(7), "spare_battery": spare })
        })
    })
}

pub fn details(keys: &Keys) -> Value {
    json!({
        "headset": {
            "product": text(keys, "110").map(|p| p.to_lowercase()),
            "firmware": text(keys, "130"),
            "name": text(keys, "220"),
        },
        "transmitter": transmitter(keys),
        "volume": num(keys, "2a0"),
        "chat_mix": chat_mix(keys),
        "mic": {
            "source": match num(keys, "640") { Some(1) => Some("boom"), Some(2) => Some("built-in"), _ => None },
            "muted": mic_muted(keys),
            "sensitivity": num(keys, "610"),
            "monitoring": num(keys, "620"),
            "noise_reduction": flag(keys, "630"),
        },
        "noise_gate": { "on": flag(keys, "700"), "level": num(keys, "710") },
        "anc": { "on": flag(keys, "750"), "level": num(keys, "760") },
        "superhuman_hearing": {
            "on": flag(keys, "720"),
            "mode": match num(keys, "730") { Some(0) => Some("legacy"), Some(1) => Some("footsteps"), Some(2) => Some("gunshots"), _ => None },
            "level": num(keys, "740"),
        },
        "eq": { "game": preset(keys, "1210", "12c0"), "mic": preset(keys, "1310", "13c0") },
        "controls": { "wheel": wheel_job(num(keys, "a20")), "button": button_job(num(keys, "b20")) },
        "power": {
            "auto_off_minutes": match num(keys, "280") { Some(0) => Some(0), Some(1) => Some(5), Some(2) => Some(10), Some(3) => Some(20), Some(4) => Some(30), _ => None },
            "wake_on_motion": flag(keys, "2e0"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(pairs: &[(&str, &str)]) -> Keys {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), Value::from(*v)))
            .collect()
    }

    #[test]
    fn decodes_mic_and_battery() {
        let k = keys(&[("240", "72"), ("640", "1"), ("600", "1"), ("650", "0")]);
        assert_eq!((battery(&k), mic_muted(&k)), (Some(72), true));
        let k = keys(&[("240", "255"), ("640", "2"), ("600", "0"), ("650", "1")]);
        assert_eq!((battery(&k), mic_muted(&k)), (None, true));
    }

    #[test]
    fn finds_selected_transmitter() {
        let mut k = Keys::new();
        k.insert("400".into(), json!({"info": ["2","17","1","100","1","10F5","2283","4.107.703.0","x"], "control": []}));
        let d = details(&k);
        assert_eq!(d["transmitter"]["product"], "2283");
        assert_eq!(d["transmitter"]["spare_battery"], 100);
    }
}
