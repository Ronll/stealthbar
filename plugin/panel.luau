--!nonstrict
-- Everything `stealthbar` reports, with a control for every setting the headset accepts. Changes are sent as
-- `stealthbar.set` to the service (which runs `stealthbar set`); the headset then pushes the new value back,
-- so the controls always show what the headset really has.

local status = noctalia.state.get("stealthbar.status")
local requests = 0
-- slider values while dragging, by setting; sent on release
local drafts = {}

local GAME_FREQS = { "32", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k" }
local MIC_FREQS = { "100", "160", "250", "400", "630", "1k", "1.6k", "2.5k", "4k", "6.3k" }
local WHEEL = { { "monitoring", "Mic monitoring" }, { "chat", "Game / chat" }, { "bass", "Bass boost" },
  { "treble", "Treble boost" }, { "gate", "Noise gate" } }
local BUTTON = { { "anc", "Noise cancellation" }, { "preset", "Next game preset" }, { "gate", "Noise gate" } }
local SHH_MODES = { { "legacy", "Legacy" }, { "footsteps", "Footsteps" }, { "gunshots", "Gunshots" } }
local AUTO_OFF = { { "never", "Never" }, { "5", "5 min" }, { "10", "10 min" }, { "20", "20 min" }, { "30", "30 min" } }
local GAME_PRESETS = { { "signature", "Signature Sound" }, { "bass", "Bass Boost" },
  { "bass-treble", "Bass & Treble Boost" }, { "vocal", "Vocal Boost" } }
local MIC_PRESETS = { { "signature", "Signature Sound" }, { "full", "Full" }, { "clarity", "Clarity" },
  { "smooth", "Smooth" } }

local render

local function send(setting, ...)
  requests += 1
  noctalia.state.set("stealthbar.set", { setting = setting, values = { ... }, id = requests })
end

local function str(v, suffix)
  if v == nil then return "—" end
  if type(v) == "boolean" then return if v then "on" else "off" end
  return tostring(v) .. (suffix or "")
end

local function label(text, color)
  return ui.label({ text = text, color = color or "on_surface_variant", flexGrow = 1 })
end

local function row(key, value)
  return ui.row({ align = "center", gap = 12 }, { label(key), ui.label({ text = value, color = "on_surface" }) })
end

local function section(title, rows)
  local children = { ui.label({ text = title, fontWeight = "semibold", color = "primary" }) }
  for _, r in rows do table.insert(children, r) end
  return ui.column({ gap = 8, align = "stretch" }, children)
end

local function toggle(key, checked, setting, enabled)
  return ui.row({ align = "center", gap = 12 }, {
    label(key),
    ui.toggle({
      checked = checked == true,
      enabled = enabled ~= false and checked ~= nil,
      onChange = function(v) send(setting, if v == "true" then "on" else "off") end,
    }),
  })
end

-- A slider whose value goes out when it is released; `extra` is a leading argument (an EQ band number).
local function slider(key, value, min, max, step, unit, setting, extra)
  local id = setting .. (extra or "")
  local shown = drafts[id] or value
  return ui.column({ gap = 2, align = "stretch" }, {
    ui.row({ align = "center", gap = 12 }, { label(key), ui.label({ text = str(shown, unit), color = "on_surface" }) }),
    ui.slider({
      min = min, max = max, step = step, value = shown or min, enabled = value ~= nil, controlSize = "sm",
      onChange = function(v)
        drafts[id] = tonumber(v)
        render()
      end,
      onDragEnd = function()
        local v = drafts[id]
        drafts[id] = nil
        if v == nil then return end
        if extra then send(setting, extra, v) else send(setting, v) end
      end,
    }),
  })
end

-- A dropdown over { {word, text}, … }; `current` is the word the headset reports.
local function select(key, options, current, setting)
  local texts, index = {}, nil
  for i, o in options do
    table.insert(texts, o[2])
    if o[1] == current then index = i - 1 end
  end
  return ui.row({ align = "center", gap = 12 }, {
    label(key),
    ui.select({
      options = texts, selectedIndex = index, placeholder = "—", controlSize = "sm", width = 170,
      enabled = current ~= nil,
      onChange = function(_, text)
        for _, o in options do
          if o[2] == text then send(setting, o[1]) end
        end
      end,
    }),
  })
end

local function presets(base, customs)
  local options = table.clone(base)
  for i = 1, 5 do
    local name = customs and customs[i]
    if type(name) == "string" and name ~= "" then table.insert(options, { `custom{i}`, name }) end
  end
  return options
end

local function bands(bank, freqs, setting)
  local out = {}
  for i, f in freqs do
    table.insert(out, slider(`{f} Hz`, bank.bands and bank.bands[i], -9, 9, 0.5, " dB", setting, tostring(i)))
  end
  return out
end

local function header(s)
  local battery = if type(s.battery) == "number" then `{s.battery}%` else "—"
  return ui.row({ align = "center", gap = 10 }, {
    ui.glyph({ name = "headphones", size = 22, color = "primary" }),
    ui.label({ text = "Stealth Pro II", fontSize = 16, fontWeight = "bold", color = "on_surface", flexGrow = 1 }),
    ui.label({ text = battery, fontSize = 16, fontWeight = "bold", color = "on_surface" }),
  })
end

local function rawRows(raw)
  local keys = {}
  for k in raw or {} do table.insert(keys, k) end
  table.sort(keys, function(a, b) return (tonumber(a, 16) or 0) < (tonumber(b, 16) or 0) end)
  local out = {}
  for _, k in keys do
    local v = raw[k]
    local text = if type(v) == "table" then noctalia.json.encode(v) else tostring(v)
    table.insert(out, ui.row({ gap = 8 }, {
      ui.label({ text = k, fontSize = 11, color = "on_surface_variant", width = 44 }),
      ui.label({ text = text, fontSize = 11, color = "on_surface", flexGrow = 1, maxWidth = 260 }),
    }))
  end
  return out
end

local function body(s)
  local d = s.details or {}
  local headset, tx, mic = d.headset or {}, d.transmitter or {}, d.mic or {}
  local anc, shh, gate = d.anc or {}, d.superhuman_hearing or {}, d.noise_gate or {}
  local eq, controls, power, conn = d.eq or {}, d.controls or {}, d.power or {}, d.connection or {}
  local game, micEq = eq.game or {}, eq.mic or {}
  local wheel, button = controls.wheel or {}, controls.button or {}

  local spare = str(tx.spare_battery, "%")
  if type(tx.spare_battery) == "number" and tx.spare_battery < 100 then spare ..= " · charging" end
  local via = ({ wireless = "2.4 GHz wireless", usb = "USB cable" })[conn.via] or "—"
  local bt = if conn.bluetooth == nil then "—" elseif conn.bluetooth then "connected" else "off"
  if conn.bluetooth and conn.bluetooth_signal_dbm then bt ..= ` · {conn.bluetooth_signal_dbm} dBm` end

  local gameEq = { select("Preset", presets(GAME_PRESETS, eq.custom_game), game.preset, "eq") }
  for _, r in bands(game, GAME_FREQS, "eq-band") do table.insert(gameEq, r) end
  local micEqRows = { select("Preset", presets(MIC_PRESETS, eq.custom_mic), micEq.preset, "mic-eq") }
  for _, r in bands(micEq, MIC_FREQS, "mic-eq-band") do table.insert(micEqRows, r) end

  local builtin = mic.source == "built-in"
  return {
    section("Battery", {
      row("Headset", str(s.battery, "%")),
      row("Headset charging", if s.charging then "yes" else "no"),
      row("Spare in dock", spare),
    }),
    section("Sound", {
      row("Volume (system)", str(d.volume, "%")),
      row("Chat wheel", str(d.chat_mix, "%")),
      toggle("Noise cancellation", anc.on, "anc"),
      slider("Ambient ↔ ANC", anc.level, 0, 100, 5, "%", "anc-level"),
      toggle("Superhuman Hearing", shh.on, "shh"),
      select("Mode", SHH_MODES, shh.mode, "shh-mode"),
      slider("Level", shh.level, 0, 100, 5, "%", "shh-level"),
      slider("Loudness (factory −12)", d.loudness_db, -24, 6, 1, " dB", "loudness"),
    }),
    section("Game EQ", gameEq),
    section("Microphone", {
      row("In use", str(mic.source)),
      row("Boom arm", if mic.source == "boom" then (if mic.muted then "up (muted)" else "down") else "—"),
      toggle("Mute built-in mics", builtin and mic.muted or false, "mic-mute", builtin),
      row("Sensitivity (system mic level)", str(mic.sensitivity, "%")),
      slider("Monitoring", mic.monitoring, 0, 100, 5, "%", "monitoring"),
      toggle("AI noise reduction", mic.noise_reduction, "noise-reduction"),
      toggle("Noise gate", gate.on, "gate"),
      slider("Gate level", gate.level, 0, 100, 5, "%", "gate-level"),
    }),
    section("Mic EQ", micEqRows),
    section("Controls", {
      select("Lower wheel (PC)", WHEEL, wheel.pc, "wheel"),
      select("Lower wheel (PlayStation)", WHEEL, wheel.playstation, "wheel-playstation"),
      select("Lower wheel (Xbox)", WHEEL, wheel.xbox, "wheel-xbox"),
      select("Mode button (PC)", BUTTON, button.pc, "button"),
      select("Mode button (PlayStation)", BUTTON, button.playstation, "button-playstation"),
      select("Mode button (Xbox)", BUTTON, button.xbox, "button-xbox"),
    }),
    section("Power & lights", {
      select("Auto off", AUTO_OFF, power.auto_off, "auto-off"),
      toggle("Wake on motion", power.wake_on_motion, "wake-on-motion"),
      slider("Voice prompts", d.prompts, 0, 100, 5, "%", "prompts"),
      slider("Status light", tx.ring_light, 0, 100, 5, "%", "ring-light"),
      slider("Battery slot light", tx.slot_light, 0, 100, 5, "%", "slot-light"),
    }),
    section("Device", {
      row("Name", str(headset.name)),
      row("Headset", `{str(headset.product)} · fw {str(headset.firmware)}`),
      row("Transmitter", `{str(tx.product or s.device)} · fw {str(tx.firmware)}`),
      row("Connection", via),
      row("Bluetooth", bt),
      row("Protocol", str(headset.protocol)),
    }),
    section("Raw values", rawRows(d.raw)),
  }
end

function render()
  local s = if type(status) == "table" then status else { present = false, on = false }
  local content
  if not s.on then
    content = {
      ui.label({
        text = if s.present then "Headset is off (or the dock needs a replug)." else "No Stealth Pro II dock found.",
        color = "on_surface_variant",
      }),
    }
  else
    content = body(s)
  end
  panel.render(ui.column({ flexGrow = 1, gap = 14, align = "stretch" }, {
    header(s),
    ui.scroll({ flexGrow = 1, gap = 18, align = "stretch" }, content),
  }))
end

noctalia.state.watch("stealthbar.status", function(value)
  status = value
  render()
end)
render()
