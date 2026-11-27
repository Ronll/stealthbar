//! The settings `stealthbar set` can change, mapped to headset keys. Only keys and values Turtle Beach's app (Swarm
//! II) or Neap are known to write are offered (sources: Neap FINDINGS.md, swarm2linux docs/protocol.md). Nothing
//! here writes a transmitter slot's base key, RACE storage or firmware commands.

use crate::decode::Keys;
use serde_json::Value;

/// One write to the headset.
#[derive(Debug, PartialEq)]
pub enum Write {
    /// `set_kvp {"0x<key>":"<value>"}`
    Kvp(String, String),
    /// RACE 0x000A, playback input level (Neap's loudness boost), in dB.
    Loudness(f32),
}

/// Name, accepted values, what it does. Printed by `stealthbar set --list` and kept in sync with the README.
pub const TABLE: &[(&str, &str, &str)] = &[
    ("anc", "on|off", "active noise cancellation (0x750)"),
    (
        "anc-level",
        "0-100",
        "0 = ambient/transparency … 100 = full ANC (0x760)",
    ),
    ("shh", "on|off", "Superhuman Hearing (0x720)"),
    (
        "shh-mode",
        "legacy|footsteps|gunshots",
        "Superhuman Hearing mode (0x730)",
    ),
    ("shh-level", "0-100", "Superhuman Hearing level (0x740)"),
    ("gate", "on|off", "mic noise gate (0x700)"),
    ("gate-level", "0-100", "noise gate level (0x710)"),
    (
        "monitoring",
        "0-100",
        "hear your own voice (sidetone) (0x620)",
    ),
    (
        "noise-reduction",
        "on|off",
        "AI noise reduction of the mic (0x630)",
    ),
    (
        "sensitivity",
        "0-100",
        "mic sensitivity; follows the system mic level (0x610)",
    ),
    (
        "mic-mute",
        "on|off",
        "mute the built-in mics (only with the boom unplugged) (0x650)",
    ),
    (
        "eq",
        "signature|bass|bass-treble|vocal|custom1-5",
        "game EQ preset (0x1210)",
    ),
    (
        "eq-band",
        "1-10 -9..9",
        "game EQ band (32 Hz … 16 kHz), dB in steps of 0.5 (0x1220-0x12b0)",
    ),
    (
        "mic-eq",
        "signature|full|clarity|smooth|custom1-5",
        "mic EQ preset (0x1310)",
    ),
    (
        "mic-eq-band",
        "1-10 -9..9",
        "mic EQ band (100 Hz … 6.3 kHz), dB in steps of 0.5 (0x1320-0x13b0)",
    ),
    (
        "wheel",
        "monitoring|chat|bass|treble|gate",
        "lower left wheel's job on PC (0xa20)",
    ),
    (
        "wheel-playstation",
        "monitoring|chat|bass|treble|gate",
        "… on PlayStation (0xa00)",
    ),
    (
        "wheel-xbox",
        "monitoring|chat|bass|treble|gate",
        "… on Xbox (0xa10)",
    ),
    (
        "button",
        "anc|preset|gate",
        "mode button's job on PC (0xb20)",
    ),
    (
        "button-playstation",
        "anc|preset|gate",
        "… on PlayStation (0xb00)",
    ),
    ("button-xbox", "anc|preset|gate", "… on Xbox (0xb10)"),
    (
        "auto-off",
        "never|5|10|20|30",
        "auto shut-off in minutes (0x280)",
    ),
    ("wake-on-motion", "on|off", "wake when picked up (0x2e0)"),
    ("prompts", "0-100", "voice prompt volume (0x2d0)"),
    (
        "ring-light",
        "0-100",
        "dock/transmitter status light brightness (slot key + 1)",
    ),
    (
        "slot-light",
        "0-100",
        "charging dock battery-slot light brightness (slot key + 2)",
    ),
    (
        "loudness",
        "-24..6",
        "playback level before the EQ in dB, stored on the headset; factory -12 (RACE 0x000A)",
    ),
];

pub const WHEEL_JOBS: [&str; 5] = ["monitoring", "chat", "bass", "treble", "gate"];
pub const BUTTON_JOBS: [&str; 3] = ["anc", "preset", "gate"];
pub const SHH_MODES: [&str; 3] = ["legacy", "footsteps", "gunshots"];
pub const GAME_PRESETS: [&str; 4] = ["signature", "bass", "bass-treble", "vocal"];
pub const MIC_PRESETS: [&str; 4] = ["signature", "full", "clarity", "smooth"];
pub const AUTO_OFF: [&str; 5] = ["never", "5", "10", "20", "30"];

fn kvp(key: &str, value: impl ToString) -> Vec<Write> {
    vec![Write::Kvp(key.into(), value.to_string())]
}

fn on_off(v: &str) -> Result<u8, String> {
    match v {
        "on" | "1" | "true" => Ok(1),
        "off" | "0" | "false" => Ok(0),
        _ => Err(format!("expected on|off, got {v:?}")),
    }
}

fn percent(v: &str) -> Result<u8, String> {
    v.parse::<u8>()
        .ok()
        .filter(|p| *p <= 100)
        .ok_or_else(|| format!("expected 0-100, got {v:?}"))
}

fn pick(v: &str, options: &[&str]) -> Result<usize, String> {
    options
        .iter()
        .position(|o| *o == v)
        .ok_or_else(|| format!("expected {}, got {v:?}", options.join("|")))
}

fn preset(v: &str, names: &[&str]) -> Result<u8, String> {
    if let Some(n) = v
        .strip_prefix("custom")
        .and_then(|n| n.parse::<u8>().ok())
        .filter(|n| (1..=5).contains(n))
    {
        return Ok(15 + n); // custom slots are preset ids 16-20
    }
    pick(v, names)
        .map(|i| i as u8 + 1)
        .map_err(|e| format!("{e} (or custom1-5)"))
}

/// `eq-band 3 -2.5` → key 0x1240, value "-25" (tenths of a dB, snapped to steps of 5 as the headset does).
fn band(base: u32, args: &[&str]) -> Result<Vec<Write>, String> {
    let [n, db] = args else {
        return Err("expected <band 1-10> <dB -9..9>".into());
    };
    let n: u32 = n
        .parse()
        .ok()
        .filter(|n| (1..=10).contains(n))
        .ok_or("band must be 1-10")?;
    let db: f32 = db
        .parse()
        .ok()
        .filter(|d: &f32| (-9.0..=9.0).contains(d))
        .ok_or("dB must be -9..9")?;
    let tenths = ((db * 2.0).round() * 5.0) as i32;
    Ok(kvp(&format!("{:x}", base + (n - 1) * 0x10), tenths))
}

/// The selected transmitter's slot key (400/420/440/460): its light brightnesses sit at +1 and +2.
fn active_slot(keys: &Keys) -> Option<u32> {
    ["400", "420", "440", "460"].iter().find_map(|k| {
        let state = keys.get(*k)?["info"].as_array()?.first()?.as_str()?;
        (state == "2")
            .then(|| u32::from_str_radix(k, 16).ok())
            .flatten()
    })
}

/// Turn `set <name> <args…>` into writes, checked against the current keys where a setting depends on them.
pub fn parse(name: &str, args: &[&str], keys: &Keys) -> Result<Vec<Write>, String> {
    let one = || match args {
        [v] => Ok(*v),
        _ => Err(format!("{name} takes one value")),
    };
    let job = |key: &str, jobs: &[&str], offset: usize| -> Result<Vec<Write>, String> {
        Ok(kvp(key, pick(one()?, jobs)? + offset))
    };
    match name {
        "anc" => Ok(kvp("750", on_off(one()?)?)),
        "anc-level" => Ok(kvp("760", percent(one()?)?)),
        "shh" => Ok(kvp("720", on_off(one()?)?)),
        "shh-mode" => Ok(kvp("730", pick(one()?, &SHH_MODES)?)),
        "shh-level" => Ok(kvp("740", percent(one()?)?)),
        "gate" => Ok(kvp("700", on_off(one()?)?)),
        "gate-level" => Ok(kvp("710", percent(one()?)?)),
        "monitoring" => Ok(kvp("620", percent(one()?)?)),
        "noise-reduction" => Ok(kvp("630", on_off(one()?)?)),
        "sensitivity" => Ok(kvp("610", percent(one()?)?)),
        "mic-mute" => {
            // with the boom plugged in its arm is the only mute and the headset undoes 0x650 writes
            if keys.get("640").and_then(Value::as_str) == Some("1") {
                return Err("the boom mic is in use: mute it with its arm".into());
            }
            Ok(kvp("650", on_off(one()?)?))
        }
        "eq" => Ok(kvp("1210", preset(one()?, &GAME_PRESETS)?)),
        "mic-eq" => Ok(kvp("1310", preset(one()?, &MIC_PRESETS)?)),
        "eq-band" => band(0x1220, args),
        "mic-eq-band" => band(0x1320, args),
        "wheel" => job("a20", &WHEEL_JOBS, 1),
        "wheel-playstation" => job("a00", &WHEEL_JOBS, 1),
        "wheel-xbox" => job("a10", &WHEEL_JOBS, 1),
        "button" => job("b20", &BUTTON_JOBS, 0),
        "button-playstation" => job("b00", &BUTTON_JOBS, 0),
        "button-xbox" => job("b10", &BUTTON_JOBS, 0),
        "auto-off" => Ok(kvp("280", pick(one()?, &AUTO_OFF)?)),
        "wake-on-motion" => Ok(kvp("2e0", on_off(one()?)?)),
        "prompts" => Ok(kvp("2d0", percent(one()?)?)),
        "ring-light" | "slot-light" => {
            let slot = active_slot(keys).ok_or("no transmitter is selected yet")?;
            let offset = if name == "ring-light" { 1 } else { 2 };
            Ok(kvp(&format!("{:x}", slot + offset), percent(one()?)?))
        }
        "loudness" => {
            let db: f32 = one()?.parse().map_err(|_| "expected dB -24..6")?;
            if !(-24.0..=6.0).contains(&db) {
                return Err("loudness must be -24..6 dB (factory -12)".into());
            }
            Ok(vec![Write::Loudness(db.round())])
        }
        _ => Err(format!(
            "unknown setting {name:?}; see `stealthbar set --list`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn w(k: &str, v: &str) -> Vec<Write> {
        vec![Write::Kvp(k.into(), v.into())]
    }

    #[test]
    fn maps_settings() {
        let keys = Keys::new();
        assert_eq!(parse("anc", &["on"], &keys), Ok(w("750", "1")));
        assert_eq!(parse("shh-mode", &["gunshots"], &keys), Ok(w("730", "2")));
        assert_eq!(parse("eq", &["bass-treble"], &keys), Ok(w("1210", "3")));
        assert_eq!(parse("eq", &["custom2"], &keys), Ok(w("1210", "17")));
        assert_eq!(
            parse("eq-band", &["3", "-2.4"], &keys),
            Ok(w("1240", "-25"))
        );
        assert_eq!(
            parse("mic-eq-band", &["10", "9"], &keys),
            Ok(w("13b0", "90"))
        );
        assert_eq!(parse("wheel", &["chat"], &keys), Ok(w("a20", "2")));
        assert_eq!(parse("button-xbox", &["gate"], &keys), Ok(w("b10", "2")));
        assert_eq!(parse("auto-off", &["never"], &keys), Ok(w("280", "0")));
        assert_eq!(
            parse("loudness", &["-6"], &keys),
            Ok(vec![Write::Loudness(-6.0)])
        );
        assert!(parse("loudness", &["12"], &keys).is_err());
        assert!(parse("anc-level", &["101"], &keys).is_err());
        assert!(parse("bogus", &["1"], &keys).is_err());
    }

    #[test]
    fn depends_on_state() {
        let mut keys = Keys::new();
        keys.insert("640".into(), json!("1"));
        assert!(parse("mic-mute", &["on"], &keys).is_err());
        keys.insert("640".into(), json!("2"));
        assert_eq!(parse("mic-mute", &["on"], &keys), Ok(w("650", "1")));
        assert!(parse("ring-light", &["50"], &keys).is_err());
        keys.insert("420".into(), json!({"info": ["2"], "control": []}));
        assert_eq!(parse("ring-light", &["50"], &keys), Ok(w("421", "50")));
        assert_eq!(parse("slot-light", &["5"], &keys), Ok(w("422", "5")));
    }
}
