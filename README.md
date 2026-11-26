# stealthbar

Status-bar indicator for the **Turtle Beach Stealth Pro II** wireless headset on Linux: battery, charging, and mic
mute, shown **only while the headset is on** (like the AirPods pop-in indicators).

Two parts:

- `stealthbar`: a small Rust daemon (one dependency besides `serde_json`: `libc`). It talks to the headset's USB
  dock or transmitter over `hidraw` and prints the headset state as **one JSON line per change**. It is bar-agnostic:
  any bar that can run a command and read lines (Noctalia, Waybar, eww, i3blocks, a script) can use it.
- `plugin/`: a [Noctalia](https://noctalia.dev) bar widget (plugin API 3) that runs `stealthbar watch` and renders it.
  The widget is the user interface; the daemon has no UI of its own.

This README is written so that an agent (or a human) can install, configure, run, debug and extend the project
without reading the code first.

## What it was built for

Turtle Beach ships no Linux software for this headset. Its Windows app (Swarm II) shows battery and settings by
talking to the dock over a vendor HID channel. `stealthbar` reads the same channel, read-only, so a Linux desktop
can show the headset's battery and mic state and hide the indicator when the headset is off.

It does **not** change any headset setting and does **not** control volume. System volume on this headset is the
standard USB Audio Class volume, which PipeWire/PulseAudio already drives (see [Volume](#volume-and-the-stuck-dock)).

## Supported hardware

Vendor `10f5` (Turtle Beach), any device whose HID report descriptor has the vendor collection (usage page
`0xFF13`). Known product ids of the Stealth Pro II family (from Swarm II's product catalogue, via Neap):

| Product ids | What |
|---|---|
| `2283`, `2284` | Xbox edition, black: charging dock (**`2283` tested**) |
| `2285`, `229f` | Xbox edition, black: USB transmitter |
| `229b`, `229c` | Xbox edition, white: charging dock |
| `229d`, `22a0` | Xbox edition, white: USB transmitter |
| `2287` / `2288` | PC edition: dock / USB transmitter |
| `2286`, `229e`, `2289` | the headset itself on its USB-C cable |

Other Turtle Beach headsets on the same Airoha platform (Atlas Air `225e`, Stealth 700 Gen 3 `2250`/`2251`) use the
same transport and may work; untested. The first matching device is used.

## Install

### 1. Build the daemon

Any Linux with a Rust toolchain (1.85+, edition 2024):

```sh
git clone https://github.com/Ronll/stealthbar.git
cd stealthbar
cargo build --release
install -Dm755 target/release/stealthbar ~/.local/bin/stealthbar   # or /usr/local/bin
```

With Nix: `nix build github:Ronll/stealthbar` (or `nix run github:Ronll/stealthbar -- status`).

### 2. Let your user open the device (udev)

The daemon opens `/dev/hidrawN`, which is root-only by default on most distributions. Install the rule shipped in
this repo; it tags Turtle Beach hidraw nodes `uaccess`, so the logged-in user gets access:

```sh
sudo install -m644 70-stealthbar.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

Then unplug and replug the dock (see the stuck-dock note below for why a fresh plug-in also matters).
Check: `ls -l /dev/hidraw*` plus `getfacl /dev/hidrawN` shows your user, or simply run `stealthbar status`.

On NixOS: `services.udev.packages = [ stealthbar ];` (the package installs the rule in `lib/udev/rules.d`), or put
the rule's line in `services.udev.extraRules`.

### 3. Check it works

```sh
stealthbar status
# {"battery":72,"charging":false,"device":"2283","mic_muted":false,"on":true,"present":true}
```

### 4. Add the Noctalia widget

The plugin id is `ronll/stealthbar`, widget entry `stealthbar`. Noctalia loads plugins from *sources*; a source of
kind `path` is a directory whose subdirectories are plugins.

1. Put the `plugin/` directory somewhere as `<dir>/stealthbar/`, e.g.
   `mkdir -p ~/.local/share/noctalia-plugins && cp -r plugin ~/.local/share/noctalia-plugins/stealthbar`.
   (The Nix package ships it at `$out/share/noctalia/plugins/stealthbar`, so `$out/share/noctalia/plugins` is a ready
   source directory.)
2. In Noctalia's config (`~/.config/noctalia/`, e.g. `plugins.toml`), add the source, enable the plugin, declare the
   widget and place it in a bar lane:

   ```toml
   [plugins]
   enabled = ["ronll/stealthbar"]          # keep your other enabled plugins in this list

   [[plugins.source]]                       # a sources list replaces Noctalia's defaults: keep official/community too
   name = "stealthbar"
   kind = "path"
   location = "/home/you/.local/share/noctalia-plugins"
   enabled = true

   [widget.stealthbar]
   type = "ronll/stealthbar:stealthbar"
   # command = "/full/path/to/stealthbar watch"   # only if stealthbar is not on Noctalia's PATH
   # low_battery = 15

   [bar.default]
   end = ["stealthbar", "bluetooth", "volume", "battery"]   # wherever you want it
   ```

   Or use the CLI on a running Noctalia: `noctalia msg plugins source add …` / `noctalia msg plugins enable
   ronll/stealthbar`, then add the widget to a bar in the settings UI.
3. `noctalia msg config-reload` (or restart Noctalia).

Widget settings (also in Noctalia's settings UI):

| Key | Default | Meaning |
|---|---|---|
| `command` | `stealthbar watch` | how to start the daemon; must stay a `watch` stream |
| `low_battery` | `15` | at or below this %, the glyph turns red (unless charging) |

What the widget shows: hidden while `on` is false. Otherwise a glyph (`microphone-off` if muted, `battery-charging`
while charging, else `headphones`), the battery percent, and a tooltip with battery, charging and mic state.

### Other bars

`stealthbar watch` never exits on its own; read its stdout line by line. Example for a Waybar custom module:
`"exec": "stealthbar watch | jq --unbuffered -c '{text: (if .on then \"🎧 \\(.battery)%\" else \"\" end)}'"`
with `"return-type": "json"`.

## Output format

Every line is a complete JSON object (no partial updates); a new line is printed only when something changed.

| Field | Type | Meaning |
|---|---|---|
| `present` | bool | a supported dock/transmitter is plugged in and could be opened |
| `on` | bool | the headset is switched on and answering through the dock |
| `battery` | int 0–100 or null | headset battery %; null while unknown (not answered yet, or the headset reports 255 right after a battery swap) |
| `charging` | bool | the headset is charging |
| `mic_muted` | bool | mic is muted: the boom arm is flipped up (boom mic in use) or the built-in mics are muted with the button (boom unplugged) |
| `device` | string or null | USB product id of the dock/transmitter in use, e.g. `"2283"` |

Commands: `stealthbar watch` (default) streams; `stealthbar status` prints the state once (after up to ~3 s of
asking) and exits. Errors go to stderr; the exit code is 0 unless the command line is wrong.

## How it works

- **Device discovery:** scans `/sys/class/hidraw/*/device/uevent` for vendor `10F5` and a report descriptor
  containing usage page `0xFF13`. Rescans every 2 s while nothing is found or after the device disappears.
- **Transport:** HID output report `0x06` / input report `0x07`, 62 bytes each. Requests are written with `write()`
  (the interface has no OUT endpoint, so the kernel sends SET_REPORT). **Replies come only from GET_REPORT(Input 7)**
  (`HIDIOCGINPUT`), never on the interrupt endpoint, so the daemon polls: every 250 ms when idle, every 20 ms right
  after a request or while data is flowing. The input side is a byte stream: packets span reports.
- **Protocol:** Airoha RACE packets (`05 <type> <len u16 LE> <cmd u16 LE> <payload>`). Turtle Beach's settings
  protocol rides in RACE command `0x9902`: the daemon subscribes to the records `GSI` (keys `0x240` battery,
  `0x250` charging) and `Mic` (`0x600` boom arm up, `0x640` mic in use: 1 boom / 2 built-in, `0x650` built-in mute).
  The headset answers with `{"OR":"GSI","KVP":{...}}` and afterwards pushes changes as `{"UP":...}`.
- **On/off:** the dock sends RACE `0x2cb1` when the wireless link changes (payload byte 2: 1 up, 0 down). The headset
  drops subscriptions when it restarts, so the daemon subscribes again 1.5 s after link-up. No answer within 3 s
  means off; while off it asks again every 10 s, while on every 60 s as a heartbeat.
- **Safety:** the daemon only sends record *read/subscribe* requests (`S` + record name), the same kind Turtle
  Beach's own app sends. It never writes keys, never sends RACE storage (`0x04xx`) or firmware (`0x1Cxx`) commands.

## Volume and the stuck dock

- **Volume:** there is no HID volume command. The headset's master volume is the dock's standard USB Audio Class
  volume (ALSA `PCM Playback Volume`, 0–74 = −74…0 dB); key `0x2A0` only mirrors it. PipeWire/PulseAudio drive it
  out of the box. The volume wheel just sends Volume Up/Down keys to the PC.
- **Stuck dock:** after a laptop suspend/resume (or other USB re-enumeration, e.g. through a hub) the dock can come
  back in a state where it **ignores system volume and answers no HID requests** (every GET_REPORT is empty).
  `stealthbar` then reports `present: true, on: false` forever. A physical unplug/replug fixes it; a plain
  `USBDEVFS_RESET` did not in our tests. The dock also seems to decide its platform mode (PC / PlayStation / Xbox) at
  enumeration time, and a wrong guess sticks until the next plug-in; this is under investigation.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `{"present":false,...}` | no supported device found: check `lsusb | grep 10f5`; check the product id is in the family above |
| `present` true, `on` false, headset is on | stuck dock (replug it), or the headset is paired to another transmitter (CrossPlay) |
| stderr `Permission denied` | udev rule missing or not applied: install it, `udevadm trigger`, replug |
| widget never appears | `stealthbar` not on Noctalia's PATH: set the widget's `command` to the full path; check `stealthbar status` in a terminal |
| `battery` stays null | headset just had its battery swapped (reports 255); wait for a push |

Debug by hand: `stealthbar watch` in a terminal while switching the headset on/off, flipping the boom arm, or
plugging in the charger; each change prints a line.

## Repository layout (for agents changing it)

| Path | What |
|---|---|
| `src/main.rs` | CLI, device discovery, hidraw I/O (write, `HIDIOCGINPUT` poll), state machine, JSON output |
| `src/race.rs` | report framing, RACE packet reassembly, request builder, JSON document extraction; unit tests |
| `plugin/plugin.toml` | Noctalia manifest: id `ronll/stealthbar`, widget `stealthbar`, settings |
| `plugin/widget.luau` | the bar widget: `noctalia.runStream(command, …)`, hides unless `on` |
| `plugin/translations/en.json` | setting labels |
| `70-stealthbar.rules` | udev rule (uaccess for vendor `10f5` hidraw) |
| `flake.nix` | optional: Nix package (binary + rule + plugin) and dev shell |

Develop: `cargo test`, `cargo clippy --all-targets`, `cargo fmt`; with Nix, `nix develop` gives the toolchain.
Check the plugin with `noctalia plugins lint plugin`. To add a field: read the key in `apply()` in `src/main.rs`,
add it to `State` and `to_json()`, subscribe to its record in `RECORDS` if needed, document it in the table above,
and render it in `plugin/widget.luau`. Known keys and records are documented in the projects below.

## Credits and protocol references

- [Neap](https://github.com/lykosapps/Neap): open-source Swarm II replacement (Windows/Linux); the most complete
  protocol notes (`FINDINGS.md`), product id catalogue, and the stuck-dock findings.
- [swarm2linux](https://github.com/tbt9czsdd4-droid/swarm2linux): Linux settings app for the PC edition
  (`docs/protocol.md`); its captured request frames work unchanged on the Xbox dock `2283`.
- [HaloBattery](https://github.com/HeyOkay/HaloBattery): first public capture of Swarm II's requests.

Not affiliated with Turtle Beach. MIT licensed.
