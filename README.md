# stealthbar

Status-bar indicator for the **Turtle Beach Stealth Pro II** wireless headset on Linux: battery, charging and
mic mute in a capsule that is shown **only while the headset is on**, a panel with everything the headset
reports **and a control for every setting it accepts** (noise cancellation, Superhuman Hearing, game and mic EQ,
mic monitoring and noise gate, wheel and button jobs, auto off, dock lights, loudness boost, …), a `stealthbar set`
CLI for the same settings, and optionally the headset's **game/chat wheel driving the volume of Discord** (or any
chat app) in PipeWire.

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
talking to the dock over a vendor HID channel. `stealthbar` speaks the same channel so a Linux desktop can show
the headset's state, hide the indicator when the headset is off, and change the headset's settings. On PC the headset receives a single stereo
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

- **Bar capsule:** hidden while `on` is false. Otherwise a big headphones glyph and, stacked beside it, the mic
  state (`microphone`, or a red `microphone-off` while muted) above the battery percent (with a bolt while
  charging; red at or below `low_battery` unless charging), plus a tooltip (battery, charging, mic).
- **Panel** (click the capsule, or `noctalia msg panel-toggle ronll/stealthbar:panel`; closes when you click
  outside it): a header (headset, battery) and four tabs, ordered by how often they're needed. The tab bar stays
  put while the content scrolls.
  - **Overview** (opens first): headset and spare-in-dock batteries with bars (bolt while charging, red at or below
    `low_battery`); noise cancellation on/off, ambient↔ANC level, Superhuman Hearing on/off; which mic is in use
    and whether it's muted; volume (the headset's system volume and the chat wheel's Discord volume, with bars);
    connection (2.4 GHz or USB, Bluetooth and its signal).
  - **Sound:** game EQ (preset buttons incl. named custom presets, 10 band sliders); Superhuman Hearing (on/off,
    mode buttons, level); loudness boost.
  - **Mic:** mic in use, monitoring, AI noise reduction, built-in mute, sensitivity; noise gate on/off and level;
    mic EQ (preset buttons, 10 bands).
  - **Device:** mode button and lower wheel jobs (PC); auto off, wake on motion, voice prompt volume, dock status
    and battery-slot lights; the PlayStation/Xbox jobs; about (name, ids, firmware, protocol); raw values (every
    key the headset reported, sorted).

  Toggles, buttons and dropdowns apply at once, sliders on release; each control shows what the headset reports
  back, so a change the headset refuses springs back. While the dock is present but the headset doesn't answer,
  the panel says so and how to clear a stuck dock.

Only the service runs the daemon. Don't run a second reader (Neap, swarm2linux) at the same time: every reader
takes replies off the same dock queue, so each would miss some. A second `stealthbar watch` refuses to start, and
`stealthbar status` / `raw` / `set` talk to the running one instead of reading the dock themselves.

## Changing settings: `stealthbar set`

`stealthbar set <setting> <value…>` sends one change to the running `stealthbar watch` (the Noctalia service's
daemon) over its control socket, `$XDG_RUNTIME_DIR/stealthbar.sock` (or `/tmp/stealthbar-<uid>.sock`; mode 0600).
It prints nothing and exits 0 when the daemon queued the write, or prints the reason and exits 1 (headset off,
bad value, no daemon running). The headset confirms by pushing the new value, visible in `stealthbar status` and
the panel within a second. `stealthbar set --list` prints this table:

| Setting | Values | Key / what |
|---|---|---|
| `anc` | `on` / `off` / `toggle` | active noise cancellation (`750`) |
| `anc-level` | 0–100 | 0 = ambient/transparency … 100 = full ANC (`760`) |
| `shh` | `on` / `off` / `toggle` | Superhuman Hearing (`720`) |
| `shh-mode` | `legacy` / `footsteps` / `gunshots` | (`730`) |
| `shh-level` | 0–100 | (`740`) |
| `gate` | `on` / `off` / `toggle` | mic noise gate (`700`) |
| `gate-level` | 0–100 | (`710`) |
| `monitoring` | 0–100 | hear your own voice / sidetone (`620`) |
| `noise-reduction` | `on` / `off` / `toggle` | AI noise reduction of the mic (`630`) |
| `sensitivity` | 0–100 | mic sensitivity (`610`); it follows the system mic input level, so usually set that instead |
| `mic-mute` | `on` / `off` / `toggle` | built-in mics' mute (`650`); refused while the boom mic is in use (its arm is the mute) |
| `eq` | `signature` / `bass` / `bass-treble` / `vocal` / `custom1`–`custom5` / `next` / `prev` | game EQ preset (`1210`) |
| `eq-band` | `<1-10> <dB -9..9>` | game EQ band at 32, 63, 125, 250, 500 Hz, 1, 2, 4, 8, 16 kHz, steps of 0.5 dB (`1220`–`12b0`); the preset becomes "custom (unsaved)" |
| `mic-eq` | `signature` / `full` / `clarity` / `smooth` / `custom1`–`custom5` / `next` / `prev` | mic EQ preset (`1310`) |
| `mic-eq-band` | `<1-10> <dB -9..9>` | mic EQ band at 100, 160, 250, 400, 630 Hz, 1, 1.6, 2.5, 4, 6.3 kHz (`1320`–`13b0`) |
| `wheel`, `wheel-playstation`, `wheel-xbox` | `monitoring` / `chat` / `bass` / `treble` / `gate` | lower left wheel's job per platform (`a20`, `a00`, `a10`) |
| `button`, `button-playstation`, `button-xbox` | `anc` / `preset` / `gate` | mode button's job per platform (`b20`, `b00`, `b10`) |
| `auto-off` | `never` / `5` / `10` / `20` / `30` | minutes (`280`) |
| `wake-on-motion` | `on` / `off` / `toggle` | (`2e0`) |
| `prompts` | 0–100 | voice prompt volume (`2d0`) |
| `ring-light`, `slot-light` | 0–100 | the selected transmitter's status light and (charging dock) battery-slot light (slot key + 1 / + 2, e.g. `401`/`402`) |
| `loudness` | dB −24…6 | playback level before the EQ, **stored on the headset**; factory −12; +6 is clearly louder, but boosting with EQ or Superhuman Hearing on top can clip (RACE `0x000A`, not a key) |

Every setting except `loudness` is stored on the headset too, as with Turtle Beach's app. Not offered: saving or
deleting custom EQ presets (the app writes names longer than one report in a way not captured yet), the master
volume (that is the system volume), the wheel position (read-only), and firmware updates.

**Keyboard shortcuts.** `toggle` flips an on/off setting and `next` / `prev` step through the EQ presets (the four
built-in ones, then the custom slots that have a name, wrapping around), both from what the headset last reported.
A value just written counts as current until the headset pushes it back (at most 3 s), so quick repeated presses
build on each other. Bind them in your compositor, e.g. niri:

```kdl
binds {
    Mod+XF86AudioMute { spawn "stealthbar" "set" "anc" "toggle"; }
    Mod+XF86AudioNext { spawn "stealthbar" "set" "eq" "next"; }
    Mod+XF86AudioPrev { spawn "stealthbar" "set" "eq" "prev"; }
}
```

(Hyprland: `bind = SUPER, XF86AudioMute, exec, stealthbar set anc toggle`; sway: `bindsym Mod4+XF86AudioMute exec
stealthbar set anc toggle`.)

### Other bars

`stealthbar watch` never exits on its own; read its stdout line by line. Example for a Waybar custom module:
`"exec": "stealthbar watch | jq --unbuffered -c '{text: (if .on then \"🎧 \\(.battery)%\" else \"\" end)}'"`
with `"return-type": "json"`.

## Config file: game/chat wheel → app volumes, sound card follows the headset

Optional. Read once at start from `$STEALTHBAR_CONFIG`, else `$XDG_CONFIG_HOME/stealthbar/config.toml`
(default `~/.config/stealthbar/config.toml`; `stealthbar config-path` prints it). Missing file = defaults (off).
Unknown keys are an error (printed to stderr; defaults are used). Restart the daemon (the Noctalia service) after
editing.

```toml
[chat_mix]
enabled = true            # default false
mode = "volume"           # default; or "crossfade"
# Case-insensitive substrings matched against a playback stream's application.name, application.process.binary
# and node.name (see `pw-dump`). Default:
apps = ["discord", "vesktop", "webcord", "WEBRTC VoiceEngine"]
duck_game = false         # crossfade only: also lower every other stream while the wheel leans toward chat
set_wheel = true          # make the lower left wheel the game/chat wheel (PC platform) on every connect

[audio]
follow_headset = false    # true: the dock's sound card disappears while the headset is off, like Bluetooth
```

**`[audio] follow_headset`**: the dock's sound card exists whenever the dock is plugged in, headset on or not, so
PipeWire keeps it as the default output even while the headset is off, and sound goes nowhere. A Bluetooth headset
doesn't have this problem because its sink disappears when it disconnects. With this on, the dock behaves the same
way:

- 8 s after the headset stops answering, the daemon sets the card's profile to `off`, so its sink and source go
  away. When the headset turns on, it sets the profile back (the one that was active before, else the available
  profile with the highest priority).
- The daemon never sets the default device itself. WirePlumber chooses it from its own history of devices the user
  picked (`~/.local/state/wireplumber/default-nodes`): while the headset is off, the most recently picked device
  that is present; when it comes back, the headset again if it was the one picked. So pick the headset as the
  default once (in your shell's audio menu or with `wpctl set-default`), and later choices stay yours.
- Profiles are switched with `pw-cli set-param <device> Profile '{ index: N, save: false }'`. `save: false` keeps
  WirePlumber from storing `off` in its default-profile state, so after a reboot, replug or WirePlumber restart the
  card comes back normal; if stealthbar is not running, nothing changes compared to not having it. `wpctl
  set-profile` and pavucontrol always save, so they are not used. While the headset is off the profile is checked
  every 10 s, because WirePlumber turns the normal profile back on by itself after a restart or a profile change
  on any card.
- When the daemon stops (SIGTERM/SIGINT/SIGHUP) it gives the card its sound back first.
- Only the sound card is affected; the dock's HID interface (battery, settings, headset on/off) is a separate USB
  interface. Apps that were moved to the headset by hand (pinned to it) don't follow the default; they stay on it
  and are silent while it's off.
- Needs `pw-dump` and `pw-cli` (PipeWire + WirePlumber 0.5).

How the game/chat wheel behaves:

- **`mode = "volume"`** (default): the wheel's value is the chat apps' volume, as mixers show it: game end 0 %
  (silent) … chat end 100 %. Nothing else is touched.
- **`mode = "crossfade"`**: Neap's game/chat crossfade. 50 is the centre with both at full volume; toward game
  lowers the chat apps (silent at the game end), toward chat lowers everything else if `duck_game` is on. A move
  across 50 stops there for one report (centre detent), so the centre is easy to hit.
- The headset pushes the wheel's value (key `0x510`) on every notch: an absolute 0–100 position it keeps itself,
  clamped at the ends. Measured on a `2283` dock: a slow notch moves it by 5, a normal one by 10, a fast spin by
  20–30 per report; nothing arrives while the wheel is still. A *read* of `0x510` (and its value right after
  reports are switched on) is a stale 0, so only pushes count, and nothing changes until the wheel has moved.
- Neap workaround (marked in `src/mix.rs`): a first reading after ≥ 0.5 s of rest that jumps more than 15 is held
  until the next reading (or 0.7 s without one), because Neap saw stray readings there. Not seen on our dock.
- Volumes are set per stream with `wpctl set-volume <stream id> <gain>`: every matching stream when the mix
  moves, and a stream that appears later (checked every 2 s) once. In between, volume changes made with other
  tools (pavucontrol, a key binding) are kept until the wheel moves again. A chat app that is silent has no
  stream, so there is nothing to set; PipeWire restores the app's last volume when it plays again, and the
  daemon then applies the current mix within 2 s.
- The dock has no push path (nothing arrives on its interrupt endpoint), so the first notch of a turn shows at
  the next idle poll (≤ 500 ms); the daemon then polls fast for 1 s, so the rest of the turn is immediate.
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

`details` (every leaf can be null until reported; values of settings use the same words as `stealthbar set`):

| Path | Meaning (raw key) |
|---|---|
| `headset.product`, `.firmware`, `.name`, `.protocol` | headset USB id (`110`), firmware (`130`), name set in Swarm II (`220`), protocol version (`Ver` `0`) |
| `transmitter.product`, `.firmware`, `.spare_battery`, `.ring_light`, `.slot_light` | the transmitter the headset uses (slot records `400`–`460`): ids, spare battery % in a charging dock (it charges whatever is in its slot), light brightnesses |
| `connection.via`, `.bluetooth`, `.bluetooth_signal_dbm` | `"wireless"` (2.4 GHz) or `"usb"` (cable) and Bluetooth connected (`290`: bit 0 Bluetooth, bits 1–2 attachment); Bluetooth signal in dBm (`320` pushes) |
| `volume` | master volume 0–100 as the headset sees it (`2a0`; mirrors the system volume, read-only) |
| `chat_mix` | game/chat wheel 0–100 as last pushed (`510`); null until the wheel moved |
| `loudness_db` | the stored playback level before the EQ (RACE `0x000A`), factory −12 |
| `mic.source` | `"boom"` or `"built-in"` (`640`) |
| `mic.muted`, `.sensitivity`, `.monitoring`, `.noise_reduction` | mute (`600` boom arm / `650` built-in), sensitivity % (`610`), sidetone % (`620`), AI noise reduction (`630`) |
| `noise_gate.on`, `.level` | `700`, `710` |
| `anc.on`, `.level` | active noise cancellation (`750`); 0 = ambient/transparency … 100 = full (`760`) |
| `superhuman_hearing.on`, `.mode`, `.level` | `720`; `legacy`/`footsteps`/`gunshots` (`730`); `740` |
| `eq.game`, `eq.mic` | `{preset, name, bands}`: preset as a `set` word (`1210`/`1310`; `"custom (unsaved)"` after a band edit), its display name (`12c0`/`13c0` or the custom slot's name), 10 bands in dB (`1220`–`12b0`, `1320`–`13b0`, tenths of a dB) |
| `eq.custom_game`, `eq.custom_mic` | names of custom presets 1–5 (records `CG1`–`CG5` at `1700`…, `CM1`–`CM5` at `1800`…), null for empty slots |
| `controls.wheel.{pc,playstation,xbox}`, `controls.button.{…}` | jobs of the lower left wheel (`a20`/`a00`/`a10`) and the mode button (`b20`/`b00`/`b10`) |
| `power.auto_off`, `.wake_on_motion` | `never`/`5`/`10`/`20`/`30` (`280`); `2e0` |
| `prompts` | voice prompt volume (`2d0`) |
| `raw` | every key the headset reported, as it reported it (also `stealthbar raw`); undecoded keys included |

Commands:

| Command | What |
|---|---|
| `stealthbar watch` | (default) stream a line per change; reconnects on its own; never exits |
| `stealthbar status` | print the state once and exit: from the running `watch` if there is one, else by reading the dock (about 2 s); never writes |
| `stealthbar raw` | every raw key the headset reported, once, as one JSON object; for finding new keys |
| `stealthbar set <setting> <value…>` | change a setting through the running `watch` (see above); `--list` prints the settings |
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
  protocol rides in RACE command `0x9902`. The headset lists its records with an `index` request: `Ver Inf GSI BT
  TX1-TX4 3DT Mic SAF Enc Btn AQG AQM CEC CG1-CG5 CM1-CM5` (no others answer). Once per connection the daemon
  subscribes (`S` + name) to all of them but `CEC` (undecoded) and reads `BT` once (`R` + name; it doesn't answer a
  subscribe); the headset answers `{"OR":"GSI","KVP":{...}}` and afterwards pushes changes as `{"UP":...}`. Writes
  are `set_kvp {"0x<key>":"<value>"}` with an ack. The loudness level is RACE `0x000A` (payload: op u16 BE 0 set /
  1 read, parameter `0x00040000` u32 BE, value f32 BE in dB), sent with the relay bit (`0x80` in byte 2) so the dock
  passes it to the headset.
- **On/off:** the dock sends RACE `0x2cb1` when the wireless link changes (payload byte 2: 1 up, 0 down). The headset
  drops subscriptions when it restarts, so the daemon subscribes again 1.5 s after link-up. No GSI answer within 3 s
  means off; while off it asks again every 10 s, while on every 60 s as a heartbeat.
- **Safety:** only the keys and values in the `set` table (which Turtle Beach's app or Neap write) are ever
  written, plus `0x500`/`0xa20` for the chat mix; never a transmitter slot's base key (Neap saw that unpair the
  headset), RACE storage (`0x04xx`) or firmware (`0x1Cxx`) commands. Without `set` calls and the chat mix it only
  reads.
- **Control socket:** `watch` listens on `$XDG_RUNTIME_DIR/stealthbar.sock` and waits on it (`poll`) instead of
  sleeping between polls of the dock, so a `set` goes out at once. One line in (`status`, `raw`, `set <setting>
  <value…>`), one line out (JSON, `ok`, or `error: …`).

## Lifecycle and resource use

- **One process, always running with the shell.** The Noctalia service starts `stealthbar watch` when Noctalia
  starts and Noctalia kills it when it stops or reloads the plugin. It is not started on demand: it has to be
  there to notice the dock being plugged in and the headset switching on, and the dock only hands out events to
  someone who polls it.
- **Three states, cheapest first:**
  - *no dock plugged in:* one directory scan of `/sys/class/hidraw` every 2 s; no device is opened.
  - *dock plugged in, headset off:* one USB control transfer (GET_REPORT) every 2 s, plus one GSI request every
    10 s to catch a headset that came on without a link message.
  - *headset on:* one GET_REPORT every 500 ms (changes are pushed by the headset into the dock's queue and picked
    up at the next poll); every 60 s one GSI request as heartbeat (all records are read once per connection);
    15 ms polling while replies arrive and for 1 s after a wheel report.
- **Measured** (headset on, chat mix on, wheel still, x86-64 laptop): about 0.05 % of one CPU core over 2 minutes,
  2.8 MB resident memory, one thread. Nearly all of it is the kernel side of the USB poll (≈ 0.25 ms per
  GET_REPORT), so the idle poll interval is the only real lever (1 s would halve it, at up to 1 s of delay).
  Release builds use LTO and are stripped (about 0.5 MB on disk).
- It does not depend on the headset being the default audio sink; it works with the dock as long as it is
  plugged in. The chat mix touches PipeWire only when the wheel moves and every 2 s while it is enabled and the
  headset is on (one `pw-dump`; a `wpctl` only when the wheel moved or for a new stream).

## Volume and the stuck dock

- **Volume:** there is no HID volume command. The headset's master volume is the dock's standard USB Audio Class
  volume (ALSA `PCM Playback Volume`, 0–74 = −74…0 dB); key `0x2A0` only mirrors it. PipeWire/PulseAudio drive it
  out of the box. The volume wheel changes that hardware volume itself and PipeWire follows; it sends no media
  keys (none were seen on the dock's consumer-control report, whose Play/Pause/Next/Prev codes also go unused).
- **Stuck dock:** after a laptop suspend/resume (or other USB re-enumeration, e.g. through a hub) the dock can come
  back in a state where it **ignores system volume and answers no HID requests** (every GET_REPORT is empty).
  `stealthbar` then reports `present: true, on: false` forever. A physical unplug/replug fixes it; a plain
  `USBDEVFS_RESET` did not in our tests, and neither did de-/re-authorizing the dock in sysfs (`authorized` 0 → 1,
  tested both from a resume hook and by hand on a stuck dock): the sound card keeps working, the HID channel stays
  silent. What works is a power cycle of the dock: unplug it, or flip the dock's **Xbox / other switch** to Xbox and
  back (it restarts the dock). While stuck, the widget stays hidden because the headset never answers.
- **The Xbox / other switch:** on the Xbox edition's dock it picks the USB mode. In the PC ("other") position the
  dock is `10f5:2283`, a high-speed USB Audio Class 1 card plus the HID interface this tool uses. In the Xbox
  position it re-enumerates as `10f5:2284`, full speed, with only two vendor-specific (Xbox GIP) interfaces that
  Linux binds to `xpad`: no sound card and nothing stealthbar can read. Keep it on the PC position. (Switching
  over, the dock briefly shows up as `0e8d:0003`, its MediaTek chip.) Turtle Beach's app also shows a platform mode
  (PC / PlayStation / Xbox); which key holds that is still unknown.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `{"present":false,...}` | no supported device found: check `lsusb | grep 10f5`; check the product id is in the family above |
| `present` true, `on` false, headset is on | stuck dock (replug it, or flip the dock's Xbox / other switch to Xbox and back), another program is reading the dock, or the headset is paired to another transmitter (CrossPlay) |
| stderr `Permission denied` | udev rule missing or not applied: install it, `udevadm trigger`, replug |
| widget never appears | `stealthbar` not on Noctalia's PATH: set the plugin's `command` to the full path; check `stealthbar status` in a terminal; restart Noctalia |
| some panel rows stay `—` | that record hasn't been answered yet; `stealthbar raw` shows which keys arrived |
| chat mix does nothing | config not loaded (`stealthbar config-path`, restart the service), the wheel hasn't moved yet, or the chat app's stream doesn't match `apps` (check `pw-dump` for its `application.name`) |
| `battery` stays null | headset just had its battery swapped (reports 255); wait for a push |
| `stealthbar set` says "no running `stealthbar watch`" | the Noctalia service (or your own `watch`) isn't running; settings only go through it |
| `watch` exits with "another `stealthbar watch` is running" | one daemon per user; stop the other one (or use its socket via `status`/`set`) |
| a panel control springs back | the headset refused the value (e.g. built-in mic mute while the boom is in use); see the `set` table |

Debug by hand: stop the Noctalia service's daemon first (only one reader: a second reader takes replies off the
same queue and makes both flaky), then `stealthbar watch` in a terminal while switching the headset on/off,
flipping the boom arm, turning the wheel or plugging in the charger. `STEALTHBAR_TRACE=1` prints every report
written and every non-empty report read as hex on stderr; replies echo the request's token bytes (byte 11 is a
per-process session byte), so replies to someone else's requests stand out.

## Repository layout (for agents changing it)

| Path | What |
|---|---|
| `src/main.rs` | CLI, device discovery, hidraw I/O (write, `HIDIOCGINPUT` poll), request queue, state machine, JSON output |
| `src/control.rs` | the control socket: `set`/`status`/`raw` from clients to the running `watch` |
| `src/settings.rs` | the `set` table: setting names → keys, value checks; the place to add a writable setting |
| `src/race.rs` | report framing, RACE packet reassembly, request builders (`subscribe`, `read_once`, `set_kvp`, `dsp`), JSON extraction; tests |
| `src/decode.rs` | raw keys → `battery`, `mic_muted`, `details`; the place to add a decoded field |
| `src/config.rs` | config file location and schema |
| `src/mix.rs` | game/chat mix curve and PipeWire stream volumes (`pw-dump`, `wpctl`) |
| `src/audio.rs` | `[audio] follow_headset`: the dock's sound card profile off while the headset is off (unsaved), back when it's on |
| `plugin/plugin.toml` | Noctalia manifest: id `ronll/stealthbar`, service, widget, panel, settings |
| `plugin/service.luau` | runs `stealthbar watch` once, publishes each line as state `stealthbar.status`; runs `stealthbar set` for the panel |
| `plugin/widget.luau` | bar capsule from `stealthbar.status`; click toggles the panel |
| `plugin/panel.luau` | the panel: rows and controls from `details`; changes go to the service as `stealthbar.set` |
| `plugin/translations/en.json` | setting labels |
| `70-stealthbar.rules` | udev rule (uaccess for vendor `10f5` hidraw) |
| `flake.nix` | optional: Nix package (binary + rule + plugin) and dev shell |

Develop: `cargo test`, `cargo clippy --all-targets`, `cargo fmt`; with Nix, `nix develop` gives the toolchain.
Check the plugin with `noctalia plugins lint plugin`. To add a field: find its key with `stealthbar raw` (and the
references below), decode it in `src/decode.rs`, document it in the tables above, and show it in
`plugin/panel.luau`. To add a writable setting: add it to `TABLE` and `parse` in `src/settings.rs` (with a test),
to the `set` table in this README, and a control in `plugin/panel.luau`. New writes must be ones Turtle Beach's app
or Neap are known to send.

## Credits and protocol references

- [Neap](https://github.com/lykosapps/Neap): open-source Swarm II replacement (Windows/Linux); the most complete
  protocol notes (`FINDINGS.md`), product id catalogue, chat-mix behaviour and the stuck-dock findings.
- [swarm2linux](https://github.com/tbt9czsdd4-droid/swarm2linux): Linux settings app for the PC edition
  (`docs/protocol.md`); its captured request frames work unchanged on the Xbox dock `2283`.
- [HaloBattery](https://github.com/HeyOkay/HaloBattery): first public capture of Swarm II's requests.

Not affiliated with Turtle Beach. MIT licensed.
