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

use crate::settings::{AUTO_OFF, BUTTON_JOBS, GAME_PRESETS, MIC_PRESETS, SHH_MODES, WHEEL_JOBS};

/// A raw value as the matching `stealthbar set` word (so the panel and CLI share one vocabulary).
fn word(options: &[&'static str], index: Option<i64>) -> Option<&'static str> {
    options.get(usize::try_from(index?).ok()?).copied()
}

/// Custom preset names, slots 1-5 (records CG1-CG5 at 1700, 1720, … or CM1-CM5 at 1800, …).
pub fn custom_names(keys: &Keys, base: u32) -> Vec<Option<String>> {
    (0..5)
        .map(|i| {
            let name = keys.get(&format!("{:x}", base + i * 0x20))?["name"].as_str()?;
            (!name.is_empty()).then(|| name.to_owned())
        })
        .collect()
}

/// One EQ bank: preset as a `set` word, its display name, and the 10 bands in dB.
fn eq(keys: &Keys, base: u32, presets: &[&'static str], customs: &[Option<String>]) -> Value {
    let id = num(keys, &format!("{:x}", base + 0x10));
    let preset = match id {
        Some(0) => Some("custom (unsaved)".to_owned()),
        Some(n @ 1..=4) => word(presets, Some(n - 1)).map(str::to_owned),
        Some(n @ 16..=20) => Some(format!("custom{}", n - 15)),
        _ => None,
    };
    let name = text(keys, &format!("{:x}", base + 0xc0))
        .filter(|n| !n.is_empty())
        .or_else(|| {
            id.filter(|n| (16..=20).contains(n))
                .and_then(|n| customs.get((n - 16) as usize).cloned().flatten())
        });
    let bands: Vec<Option<f64>> = (0..10)
        .map(|i| num(keys, &format!("{:x}", base + 0x20 + i * 0x10)).map(|t| t as f64 / 10.0))
        .collect();
    json!({ "preset": preset, "name": name, "bands": bands })
}

/// The paired transmitter the headset has selected (`info[0]` = 2) among TX1..TX4 (keys 400/420/440/460).
fn transmitter(keys: &Keys) -> Option<Value> {
    ["400", "420", "440", "460"].iter().find_map(|k| {
        let slot = keys.get(*k)?;
        let info = slot["info"].as_array()?;
        let s = |i: usize| info.get(i).and_then(Value::as_str);
        let control = |i: usize| {
            slot["control"]
                .get(i)
                .and_then(Value::as_str)
                .and_then(|v| v.parse::<i64>().ok())
        };
        (s(0) == Some("2")).then(|| {
            // 255 while the slot is empty after a battery went in or out; 0 when it was empty at power-on
            let spare = s(3)
                .and_then(|v| v.parse::<i64>().ok())
                .filter(|v| (1..=100).contains(v));
            json!({
                "product": s(6).map(str::to_lowercase),
                "firmware": s(7),
                "spare_battery": spare,
                "ring_light": control(1),
                "slot_light": control(2),
            })
        })
    })
}

pub fn details(keys: &Keys) -> Value {
    // 0x290: bit 0 Bluetooth connected, bits 1-2 attachment (1 = 2.4 GHz, 2 = USB cable) (Neap, measured)
    let link = num(keys, "290");
    // 0x320 is pushed as a signed level in dBm; a read answers an unrelated positive number
    let bt_signal = num(keys, "320").filter(|v| *v < 0);
    json!({
        "headset": {
            "product": text(keys, "110").map(|p| p.to_lowercase()),
            "firmware": text(keys, "130"),
            "name": text(keys, "220"),
            "protocol": text(keys, "0"),
        },
        "transmitter": transmitter(keys),
        "connection": {
            "via": link.and_then(|l| match (l >> 1) & 3 { 1 => Some("wireless"), 2 => Some("usb"), _ => None }),
            "bluetooth": link.map(|l| l & 1 == 1),
            "bluetooth_signal_dbm": bt_signal,
        },
        "volume": num(keys, "2a0"),
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
            "mode": word(&SHH_MODES, num(keys, "730")),
            "level": num(keys, "740"),
        },
        "eq": {
            "game": eq(keys, 0x1200, &GAME_PRESETS, &custom_names(keys, 0x1700)),
            "mic": eq(keys, 0x1300, &MIC_PRESETS, &custom_names(keys, 0x1800)),
            "custom_game": custom_names(keys, 0x1700),
            "custom_mic": custom_names(keys, 0x1800),
        },
        "controls": {
            "wheel": {
                "pc": word(&WHEEL_JOBS, num(keys, "a20").map(|v| v - 1)),
                "playstation": word(&WHEEL_JOBS, num(keys, "a00").map(|v| v - 1)),
                "xbox": word(&WHEEL_JOBS, num(keys, "a10").map(|v| v - 1)),
            },
            "button": {
                "pc": word(&BUTTON_JOBS, num(keys, "b20")),
                "playstation": word(&BUTTON_JOBS, num(keys, "b00")),
                "xbox": word(&BUTTON_JOBS, num(keys, "b10")),
            },
        },
        "power": {
            "auto_off": word(&AUTO_OFF, num(keys, "280")),
            "wake_on_motion": flag(keys, "2e0"),
        },
        "prompts": num(keys, "2d0"),
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

    #[test]
    fn decodes_eq_controls_and_link() {
        let mut k = keys(&[
            ("1210", "17"),
            ("1220", "-25"),
            ("12c0", ""),
            ("a20", "2"),
            ("b10", "1"),
            ("280", "0"),
            ("290", "3"),
            ("320", "-55"),
            ("730", "1"),
        ]);
        k.insert("1720".into(), json!({"name": "De Mud", "bands": []}));
        let d = details(&k);
        assert_eq!(d["eq"]["game"]["preset"], "custom2");
        assert_eq!(d["eq"]["game"]["name"], "De Mud");
        assert_eq!(d["eq"]["game"]["bands"][0], -2.5);
        assert_eq!(d["eq"]["custom_game"][1], "De Mud");
        assert_eq!(d["controls"]["wheel"]["pc"], "chat");
        assert_eq!(d["controls"]["button"]["xbox"], "preset");
        assert_eq!(d["power"]["auto_off"], "never");
        assert_eq!(d["connection"]["via"], "wireless");
        assert_eq!(d["connection"]["bluetooth"], true);
        assert_eq!(d["connection"]["bluetooth_signal_dbm"], -55);
        assert_eq!(d["superhuman_hearing"]["mode"], "footsteps");
    }
}
