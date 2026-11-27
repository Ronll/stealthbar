//! Optional config file: `$XDG_CONFIG_HOME/stealthbar/config.toml` (default `~/.config/stealthbar/config.toml`),
//! or the path in `$STEALTHBAR_CONFIG`. A missing file means defaults (chat mix and follow_headset off).

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub chat_mix: ChatMix,
    pub audio: Audio,
    pub buttons: Buttons,
}

/// What a headset button does when the headset hands it to the app. With the chat mix on, stealthbar sets
/// 0x500 = 1 (the headset then reports the wheel), and the headset then treats stealthbar as Turtle Beach's app:
/// a short press of the power button no longer toggles Superhuman Hearing by itself but only flips 0x520.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Buttons {
    /// "superhuman-hearing" (what the button does without an app), "none", or a shell command.
    pub power: String,
}

impl Default for Buttons {
    fn default() -> Self {
        Self {
            power: "superhuman-hearing".into(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Audio {
    /// Turn the dock's sound card profile off while the headset is off (and back on), so it disappears from
    /// PipeWire like a disconnected Bluetooth headset and WirePlumber picks the default from its own history.
    pub follow_headset: bool,
}

/// How the wheel's 0-100 value is applied.
#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The value is the chat apps' volume: 0 silent … 100 full. Nothing else is touched.
    Volume,
    /// Neap-style game/chat crossfade around a centre (50 = both full, with a detent there): toward game lowers the
    /// chat apps, toward chat lowers everything else when `duck_game` is on.
    Crossfade,
}

/// The headset's game/chat wheel drives the volume of chat apps' playback streams in PipeWire.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChatMix {
    pub enabled: bool,
    /// Case-insensitive substrings matched against a stream's application.name, application.process.binary
    /// and node.name.
    pub apps: Vec<String>,
    pub mode: Mode,
    /// Crossfade mode only: also turn everything else down while the wheel leans toward chat.
    pub duck_game: bool,
    /// Set the second left wheel's job (PC platform, key 0xa20) to game/chat mix when the headset connects.
    pub set_wheel: bool,
}

impl Default for ChatMix {
    fn default() -> Self {
        Self {
            enabled: false,
            apps: ["discord", "vesktop", "webcord", "WEBRTC VoiceEngine"]
                .map(String::from)
                .into(),
            mode: Mode::Volume,
            duck_game: false,
            set_wheel: true,
        }
    }
}

pub fn path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("STEALTHBAR_CONFIG") {
        return Some(p.into());
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("stealthbar/config.toml"))
}

pub fn load() -> Config {
    let Some(path) = path() else {
        return Config::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("stealthbar: {}: {e}; using defaults", path.display());
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_defaults() {
        let c: Config = toml::from_str("[chat_mix]\nenabled = true\n").unwrap();
        assert!(c.chat_mix.enabled && c.chat_mix.set_wheel && !c.chat_mix.duck_game);
        assert_eq!(c.chat_mix.mode, Mode::Volume);
        let c: Config = toml::from_str("[chat_mix]\nmode = \"crossfade\"\n").unwrap();
        assert_eq!(c.chat_mix.mode, Mode::Crossfade);
        assert!(c.chat_mix.apps.iter().any(|a| a == "discord"));
        assert!(!Config::default().chat_mix.enabled);
        assert!(!Config::default().audio.follow_headset);
        assert_eq!(Config::default().buttons.power, "superhuman-hearing");
        let c: Config =
            toml::from_str("[buttons]\npower = \"noctalia msg media toggle\"\n").unwrap();
        assert_eq!(c.buttons.power, "noctalia msg media toggle");
        let c: Config = toml::from_str("[audio]\nfollow_headset = true\n").unwrap();
        assert!(c.audio.follow_headset);
        assert!(toml::from_str::<Config>("[chat_mix]\nbogus = 1\n").is_err());
    }
}
