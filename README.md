# stealthbar

Status-bar indicator for the **Turtle Beach Stealth Pro II** wireless headset on Linux: battery, charging and
mic mute in a capsule that is shown **only while the headset is on**, a panel with everything the headset
reports (ANC, Superhuman Hearing, EQ presets, mic settings, dock spare battery, firmware, …), and optionally the
headset's **game/chat wheel driving the volume of Discord** (or any chat app) in PipeWire.

Two parts:

- `stealthbar`: a small Rust daemon. It talks to the headset's USB dock or transmitter over `hidraw` and prints
  the headset state as **one JSON line per change**. It is bar-agnostic: any bar that can run a command and read
  lines (Noctalia, Waybar, eww, i3blocks, a script) can use it.
- `plugin/`: a [Noctalia](https://noctalia.dev) plugin (plugin API 43, Noctalia 5.4+): a background service that
  runs the daemon, a bar widget, and a panel opened by clicking the widget. The plugin is the user interface; the
  daemon has none of its own.

This README is written so that an agent (or a human) can install, configure, run, debug and extend the project
without reading the code first.

## What it was built for

Turtle Beach ships no Linux software for this headset. Its Windows app (Swarm II) shows battery and settings by
talking to the dock over a vendor HID channel. `stealthbar` reads the same channel so a Linux desktop can show the
headset's state and hide the indicator when the headset is off. On PC the headset receives a single stereo
stream, so its game/chat wheel can't mix anything by itself; `stealthbar` can apply the wheel to app volumes.

It does **not** control the master volume: that is the standard USB Audio Class volume, which PipeWire/PulseAudio
already drive (see [Volume](#volume-and-the-stuck-dock)).

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

Any Linux with a Rust toolchain (1.88+, edition 2024):

```sh
git clone https://github.com/Ronll/stealthbar.git
cd stealthbar
cargo build --release
install -Dm755 target/release/stealthbar ~/.local/bin/stealthbar   # or /usr/local/bin
```

With Nix: `nix build github:Ronll/stealthbar` (or `nix run github:Ronll/stealthbar -- status`).
Runtime tools: none, except `pw-dump` and `wpctl` (PipeWire) when the chat mix is enabled.

### 2. Let your user open the device (udev)

The daemon opens `/dev/hidrawN`, which is root-only by default on most distributions. Install the rule shipped in
this repo; it tags Turtle Beach hidraw nodes `uaccess`, so the logged-in user gets access:

```sh
sudo install -m644 70-stealthbar.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

Then unplug and replug the dock (see the stuck-dock note below for why a fresh plug-in also matters).
Check: `getfacl /dev/hidrawN` shows your user, or simply run `stealthbar status`.

On NixOS: `services.udev.packages = [ stealthbar ];` (the package installs the rule in `lib/udev/rules.d`), or put
the rule's line in `services.udev.extraRules`.

### 3. Check it works

```sh
stealthbar status
# {"battery":72,"charging":false,"details":{...},"device":"2283","mic_muted":false,"on":true,"present":true}
```

### 4. Add the Noctalia plugin

Plugin id `ronll/stealthbar`; entries: service `service`, bar widget `stealthbar`, panel `panel`. Noctalia loads
plugins from *sources*; a source of kind `path` is a directory whose subdirectories are plugins.

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

   [plugin_settings."ronll/stealthbar"]
   command = "stealthbar watch"             # full path if stealthbar is not on Noctalia's PATH

   [widget.stealthbar]
   type = "ronll/stealthbar:stealthbar"
   # low_battery = 15

   [bar.default]
   end = ["stealthbar", "bluetooth", "volume", "battery"]   # wherever you want it
   ```

   Bar lanes saved from Noctalia's settings UI (in `~/.local/state/noctalia/settings.toml`) override the file; then
   also run `noctalia msg config-set bar.default.end '[..., "stealthbar", ...]'`.
3. Restart Noctalia (`systemctl --user restart noctalia` or however you run it) so the new plugin and its PATH
   dependency are picked up.

Settings:

| Where | Key | Default | Meaning |
|---|---|---|---|
| plugin | `command` | `stealthbar watch` | how the service starts the daemon; must stay a `watch` stream |
| widget | `low_battery` | `15` | at or below this %, the glyph turns red (unless charging) |

What the UI shows:

- **Bar capsule:** hidden while `on` is false. Otherwise a glyph (`microphone-off` if muted, `battery-charging`
  while charging, else `headphones`), the battery percent, and a tooltip (battery, charging, mic).
- **Panel** (click the capsule, or `noctalia msg panel-toggle ronll/stealthbar:panel`): battery (headset, charging,
  spare battery in the dock), microphone (boom/built-in, muted, sensitivity, monitoring, AI noise reduction, noise
  gate), sound (volume, game/chat mix, noise cancellation, Superhuman Hearing, game and mic EQ presets), controls
  (lower wheel and mode button jobs, auto off, wake on motion), device (name, headset and transmitter product ids
  and firmware). Read-only.

Only the service runs the daemon. Don't run a second `stealthbar watch` (or Neap, or swarm2linux) at the same time:
every reader takes replies off the same dock queue, so each would miss some.

### Other bars

`stealthbar watch` never exits on its own; read its stdout line by line. Example for a Waybar custom module:
`"exec": "stealthbar watch | jq --unbuffered -c '{text: (if .on then \"🎧 \\(.battery)%\" else \"\" end)}'"`
with `"return-type": "json"`.

## Config file: game/chat wheel → app volumes

Optional. Read once at start from `$STEALTHBAR_CONFIG`, else `$XDG_CONFIG_HOME/stealthbar/config.toml`
(default `~/.config/stealthbar/config.toml`; `stealthbar config-path` prints it). Missing file = defaults (off).
Unknown keys are an error (printed to stderr; defaults are used). Restart the daemon (the Noctalia service) after
editing.

```toml
[chat_mix]
enabled = true            # default false
# Case-insensitive substrings matched against a playback stream's application.name, application.process.binary
# and node.name (see `pw-dump`). Default:
apps = ["discord", "vesktop", "webcord", "WEBRTC VoiceEngine"]
duck_game = false         # also lower every other stream while the wheel leans toward chat
set_wheel = true          # make the lower left wheel the game/chat wheel (PC platform) on every connect
```

How it behaves:

- The wheel position is 0 (game only) … 50 (centre) … 100 (chat only). Same curve as Neap: the centre leaves
  everything at full volume; turning toward game lowers the chat apps (to silent at the end), turning toward chat
  lowers the rest if `duck_game` is on.
- Volumes are set per stream with `wpctl set-volume <stream id> <gain>`: every matching stream when the wheel
  moves, and a stream that appears later (checked every 5 s) once. In between, volume changes made with other
  tools (pavucontrol, a key binding) are kept until the wheel moves again.
- Nothing is changed until the wheel actually moves after the headset connects (a fresh read reports 0, which would
  otherwise mean "game only").
- **Writes to the headset** (only with `enabled = true`; the same keys and values Swarm II/Neap write): `0x500 = 1`
  after every connect (the headset only reports the wheel while it is set, and clears it at each connect), and
  with `set_wheel`, `0xa20 = 2` (the PC platform's lower-wheel job: 1 mic monitoring (factory default), 2 game/chat
  mix, 3 bass, 4 treble, 5 noise gate). To undo the wheel job, set it back in Turtle Beach's app (or Neap). This is
  stored on the headset.

## Output format

Every line is a complete JSON object (no partial updates); a new line is printed only when something changed.

| Field | Type | Meaning |
|---|---|---|
| `present` | bool | a supported dock/transmitter is plugged in and could be opened |
| `on` | bool | the headset is switched on and answering through the dock |
| `battery` | int 0–100 or null | headset battery % as the headset reports it (key `0x240`); null while unknown |
| `charging` | bool | the headset is charging |
| `mic_muted` | bool | boom arm flipped up (boom mic in use), or built-in mics muted with the button (boom unplugged) |
| `device` | string or null | USB product id of the dock/transmitter in use, e.g. `"2283"` |
| `details` | object or null | everything else, decoded; null while `on` is false. Every leaf can be null (not reported yet) |

`details`:

| Path | Meaning (raw key) |
|---|---|
| `headset.product`, `.firmware`, `.name` | headset USB id (`110`), firmware (`130`), name set in Swarm II (`220`) |
| `transmitter.product`, `.firmware`, `.spare_battery` | the transmitter the headset uses (slot records `400`–`460`); spare battery % in a charging dock |
| `volume` | master volume 0–100 as the headset sees it (`2a0`; mirrors the system volume, read-only) |
| `chat_mix` | game/chat wheel 0–100, only once reports are on and the wheel moved (`510`) |
| `mic.source` | `"boom"` or `"built-in"` (`640`) |
| `mic.muted`, `.sensitivity`, `.monitoring`, `.noise_reduction` | mute (`600`/`650`), sensitivity % (`610`, follows the system mic level), sidetone % (`620`), AI noise reduction (`630`) |
| `noise_gate.on`, `.level` | `700`, `710` |
| `anc.on`, `.level` | active noise cancellation (`750`); level 0 = ambient/transparency … 100 = full (`760`) |
| `superhuman_hearing.on`, `.mode`, `.level` | `720`; `legacy`/`footsteps`/`gunshots` (`730`); `740` |
| `eq.game`, `eq.mic` | preset names (`1210`/`12c0`, `1310`/`13c0`); `"custom (unsaved)"` for edited bands |
| `controls.wheel`, `.button` | PC platform jobs of the lower left wheel (`a20`) and the mode button (`b20`) |
| `power.auto_off_minutes`, `.wake_on_motion` | 0 = never, 5, 10, 20, 30 (`280`); `2e0` |

Commands:

| Command | What |
|---|---|
| `stealthbar watch` | (default) stream a line per change; reconnects on its own; never exits |
| `stealthbar status` | print the state once (about 2 s) and exit; never writes to the headset |
| `stealthbar raw` | print every raw key the headset reported, once, as one JSON object; for finding new keys |
| `stealthbar config-path` | print where the config file is read from |

Errors go to stderr; the exit code is 0 unless the command line is wrong.

## How it works

- **Device discovery:** scans `/sys/class/hidraw/*/device/uevent` for vendor `10F5` and a report descriptor
  containing usage page `0xFF13`. Rescans every 2 s while nothing is found or after the device disappears.
- **Transport:** HID output report `0x06` / input report `0x07`, 62 bytes each. Requests are written with `write()`
  (the interface has no OUT endpoint, so the kernel sends SET_REPORT). **Replies come only from GET_REPORT(Input 7)**
  (`HIDIOCGINPUT`), never on the interrupt endpoint, so the daemon polls: every 500 ms when idle (2 s while the
  headset is off), every 15 ms while requests or replies are in flight. The input side is a byte stream: packets span reports. Requests go out
  **one at a time** (next one after the reply, or after 600 ms): sending them back to back overflows the dock's
  reply queue and loses records.
- **Protocol:** Airoha RACE packets (`05 <type> <len u16 LE> <cmd u16 LE> <payload>`). Turtle Beach's settings
  protocol rides in RACE command `0x9902`. The daemon subscribes (`S` + name) to the records `GSI Inf Mic SAF 3DT
  Enc Btn AQG AQM TX1..TX4`; the headset answers `{"OR":"GSI","KVP":{...}}` and afterwards pushes changes as
  `{"UP":...}`. Writes are `set_kvp {"0x500":"1"}` (only for the chat mix).
- **On/off:** the dock sends RACE `0x2cb1` when the wireless link changes (payload byte 2: 1 up, 0 down). The headset
  drops subscriptions when it restarts, so the daemon subscribes again 1.5 s after link-up. No GSI answer within 3 s
  means off; while off it asks again every 10 s, while on every 60 s as a heartbeat.
- **Safety:** it never sends RACE storage (`0x04xx`) or firmware (`0x1Cxx`) commands. Without a config file it
  only reads.

## Lifecycle and resource use

- **One process, always running with the shell.** The Noctalia service starts `stealthbar watch` when Noctalia
  starts and Noctalia kills it when it stops or reloads the plugin. It is not started on demand: it has to be
  there to notice the dock being plugged in and the headset switching on, and the dock only hands out events to
  someone who polls it.
- **Three states, cheapest first:**
  - *no dock plugged in:* one directory scan of `/sys/class/hidraw` every 2 s; no device is opened.
  - *dock plugged in, headset off:* one USB control transfer (GET_REPORT) every 2 s, plus a 13-request ask every
    10 s to catch a headset that came on without a link message.
  - *headset on:* one GET_REPORT every 500 ms (changes are pushed by the headset into the dock's queue and picked up
    at the next poll), a re-ask every 60 s as heartbeat; bursts of 15 ms polling only while replies are arriving.
- **Measured** (headset on, x86-64 laptop): about 0.03 % of one CPU core, 2.7 MB resident memory, one thread.
  Release builds use LTO and are stripped (about 0.5 MB on disk).
- It does not depend on the headset being the default audio sink; it works with the dock as long as it is
  plugged in. The chat mix touches PipeWire only when the wheel moves and every 5 s while it is enabled and the
  headset is on (one `pw-dump`; a `wpctl` only when the wheel moved or for a new stream).

## Volume and the stuck dock

- **Volume:** there is no HID volume command. The headset's master volume is the dock's standard USB Audio Class
  volume (ALSA `PCM Playback Volume`, 0–74 = −74…0 dB); key `0x2A0` only mirrors it. PipeWire/PulseAudio drive it
  out of the box. The upper wheel just sends Volume Up/Down keys to the PC.
- **Stuck dock:** after a laptop suspend/resume (or other USB re-enumeration, e.g. through a hub) the dock can come
  back in a state where it **ignores system volume and answers no HID requests** (every GET_REPORT is empty).
  `stealthbar` then reports `present: true, on: false` forever. A physical unplug/replug fixes it; a plain
  `USBDEVFS_RESET` did not in our tests. The dock also seems to decide its platform mode (PC / PlayStation / Xbox) at
  enumeration time, and a wrong guess sticks until the next plug-in; this is under investigation.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `{"present":false,...}` | no supported device found: check `lsusb | grep 10f5`; check the product id is in the family above |
| `present` true, `on` false, headset is on | stuck dock (replug it), another program is reading the dock, or the headset is paired to another transmitter (CrossPlay) |
| stderr `Permission denied` | udev rule missing or not applied: install it, `udevadm trigger`, replug |
| widget never appears | `stealthbar` not on Noctalia's PATH: set the plugin's `command` to the full path; check `stealthbar status` in a terminal; restart Noctalia |
| some panel rows stay `—` | that record hasn't been answered yet; `stealthbar raw` shows which keys arrived |
| chat mix does nothing | config not loaded (`stealthbar config-path`, restart the service), the wheel hasn't moved yet, or the chat app's stream doesn't match `apps` (check `pw-dump` for its `application.name`) |
| `battery` stays null | headset just had its battery swapped (reports 255); wait for a push |

Debug by hand: stop the Noctalia service's daemon first (only one reader), then `stealthbar watch` in a terminal
while switching the headset on/off, flipping the boom arm, turning the wheel or plugging in the charger.

## Repository layout (for agents changing it)

| Path | What |
|---|---|
| `src/main.rs` | CLI, device discovery, hidraw I/O (write, `HIDIOCGINPUT` poll), request queue, state machine, JSON output |
| `src/race.rs` | report framing, RACE packet reassembly, request builders (`subscribe`, `set_kvp`), JSON extraction; tests |
| `src/decode.rs` | raw keys → `battery`, `mic_muted`, `details`; the place to add a decoded field |
| `src/config.rs` | config file location and schema |
| `src/mix.rs` | game/chat mix curve and PipeWire stream volumes (`pw-dump`, `wpctl`) |
| `plugin/plugin.toml` | Noctalia manifest: id `ronll/stealthbar`, service, widget, panel, settings |
| `plugin/service.luau` | runs `stealthbar watch` once, publishes each line as state `stealthbar.status` |
| `plugin/widget.luau` | bar capsule from `stealthbar.status`; click toggles the panel |
| `plugin/panel.luau` | the panel (read-only rows from `details`) |
| `plugin/translations/en.json` | setting labels |
| `70-stealthbar.rules` | udev rule (uaccess for vendor `10f5` hidraw) |
| `flake.nix` | optional: Nix package (binary + rule + plugin) and dev shell |

Develop: `cargo test`, `cargo clippy --all-targets`, `cargo fmt`; with Nix, `nix develop` gives the toolchain.
Check the plugin with `noctalia plugins lint plugin`. To add a field: find its key with `stealthbar raw` (and the
references below), decode it in `src/decode.rs`, document it in the tables above, and show it in
`plugin/panel.luau`. New writes must be ones Turtle Beach's app or Neap are known to send.

## Credits and protocol references

- [Neap](https://github.com/lykosapps/Neap): open-source Swarm II replacement (Windows/Linux); the most complete
  protocol notes (`FINDINGS.md`), product id catalogue, chat-mix behaviour and the stuck-dock findings.
- [swarm2linux](https://github.com/tbt9czsdd4-droid/swarm2linux): Linux settings app for the PC edition
  (`docs/protocol.md`); its captured request frames work unchanged on the Xbox dock `2283`.
- [HaloBattery](https://github.com/HeyOkay/HaloBattery): first public capture of Swarm II's requests.

Not affiliated with Turtle Beach. MIT licensed.
