--!nonstrict
-- Everything `stealthbar` reports, with a control for every setting the headset accepts, in four tabs ordered by
-- how often they're needed: Overview (battery, the everyday switches, mic, chat wheel), Sound (EQ, Superhuman
-- Hearing, loudness), Mic, Device (button/wheel jobs, power, lights, info, raw keys). Changes go out as
-- `stealthbar.set` to the service (which runs `stealthbar set`); the headset then pushes the new value back, so
-- the controls always show what the headset really has.

local status = noctalia.state.get("stealthbar.status")
local requests = 0
-- slider values while dragging, by setting; sent on release
local drafts = {}

local TABS = { { "overview", "Overview" }, { "sound", "Sound" }, { "mic", "Mic" }, { "device", "Device" } }
local activeTab = "overview"

local GAME_FREQS = { "32", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k" }
local MIC_FREQS = { "100", "160", "250", "400", "630", "1k", "1.6k", "2.5k", "4k", "6.3k" }
local WHEEL = { { "monitoring", "Mic monitoring" }, { "chat", "Game / chat" }, { "bass", "Bass boost" },
  { "treble", "Treble boost" }, { "gate", "Noise gate" } }
local BUTTON = { { "anc", "Noise cancellation" }, { "preset", "Next game preset" }, { "gate", "Noise gate" } }
local SHH_MODES = { { "legacy", "Legacy" }, { "footsteps", "Footsteps" }, { "gunshots", "Gunshots" } }
local AUTO_OFF = { { "never", "Never" }, { "5", "5 min" }, { "10", "10 min" }, { "20", "20 min" }, { "30", "30 min" } }
local GAME_PRESETS = { { "signature", "Signature" }, { "bass", "Bass Boost" },
  { "bass-treble", "Bass & Treble" }, { "vocal", "Vocal Boost" } }
local MIC_PRESETS = { { "signature", "Signature" }, { "full", "Full" }, { "clarity", "Clarity" },
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

-- ── building blocks ──────────────────────────────────────────────────────────

local function label(text, color)
  return ui.label({ text = text, color = color or "on_surface_variant", flexGrow = 1 })
end

local function row(key, value)
  return ui.row({ align = "center", gap = 12 }, { label(key), ui.label({ text = value, color = "on_surface" }) })
end

-- A titled group on a soft card, so sections read as blocks.
local function section(title, rows, glyph)
  local head = {}
  if glyph then table.insert(head, ui.glyph({ name = glyph, size = 16, color = "primary" })) end
  table.insert(head, ui.label({ text = title, fontWeight = "semibold", color = "primary" }))
  local children = { ui.row({ align = "center", gap = 8 }, head) }
  for _, r in rows do table.insert(children, r) end
  return ui.column({ gap = 10, align = "stretch", padding = 12, radius = 12, fill = "surface_container/0.5" }, children)
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

-- A dropdown over { {word, text}, … }; `current` is the word the headset reports. For long or rarely used lists.
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

-- Buttons for a short list of choices, `perRow` to a row; the current one is highlighted.
local function choices(options, current, setting, perRow)
  local rows, line = {}, {}
  for _, o in options do
    local selected = o[1] == current
    table.insert(line, ui.button({
      key = setting .. "-" .. o[1],
      text = o[2],
      variant = if selected then "secondary" else "ghost",
      selected = selected,
      controlSize = "sm",
      flexGrow = 1,
      enabled = current ~= nil,
      onClick = function()
        if not selected then send(setting, o[1]) end
      end,
    }))
    if #line == perRow then
      table.insert(rows, ui.row({ gap = 6 }, line))
      line = {}
    end
  end
  if #line > 0 then
    while #line < perRow do table.insert(line, ui.spacer({ flexGrow = 1 })) end
    table.insert(rows, ui.row({ gap = 6 }, line))
  end
  return ui.column({ gap = 6, align = "stretch" }, rows)
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

local function eqRows(bank, base, customs, freqs, presetSetting, bandSetting)
  local out = { choices(presets(base, customs), bank.preset, presetSetting, 2) }
  if bank.preset == "custom (unsaved)" then
    table.insert(out, ui.label({ text = "Custom (unsaved) — set in the bands below", fontSize = 11 }))
  end
  table.insert(out, ui.separator({ thickness = 1, color = "on_surface/0.08" }))
  for _, r in bands(bank, freqs, bandSetting) do table.insert(out, r) end
  return out
end

-- One battery: icon, name, percent (bolt while charging), and a bar.
local function battery(name, glyph, level, charging, low)
  local has = type(level) == "number"
  local isLow = has and level <= low and not charging
  local value = {}
  if charging then table.insert(value, ui.glyph({ name = "bolt", size = 14, color = "secondary" })) end
  table.insert(value, ui.label({
    text = if has then `{level}%` else "—",
    fontSize = 18, fontWeight = "bold",
    color = if isLow then "error" elseif has then "on_surface" else "on_surface_variant",
  }))
  return ui.column({ align = "center", gap = 4, flexGrow = 1 }, {
    ui.glyph({ name = glyph, size = 20, color = "on_surface_variant" }),
    ui.label({ text = name, fontSize = 12, color = "on_surface_variant" }),
    ui.row({ align = "center", gap = 2 }, value),
    ui.progress({
      progress = if has then math.clamp(level, 0, 100) / 100 else 0,
      width = 110, height = 4, radius = 2,
      fill = if isLow then "error" elseif charging then "secondary" else "primary",
    }),
  })
end

-- ── tabs ─────────────────────────────────────────────────────────────────────

local function overview(s, d)
  local tx, mic, anc, shh = d.transmitter or {}, d.mic or {}, d.anc or {}, d.superhuman_hearing or {}
  local conn = d.connection or {}
  local low = tonumber(noctalia.getConfig("low_battery")) or 15

  local micText = "—"
  if mic.source == "boom" then
    micText = if mic.muted then "Boom mic · muted (arm up)" else "Boom mic · live"
  elseif mic.source == "built-in" then
    micText = if mic.muted then "Built-in mics · muted" else "Built-in mics · live"
  end
  local via = ({ wireless = "2.4 GHz wireless", usb = "USB cable" })[conn.via] or "—"
  local bt = if conn.bluetooth then "Bluetooth on" else "Bluetooth off"
  if conn.bluetooth and conn.bluetooth_signal_dbm then bt ..= ` ({conn.bluetooth_signal_dbm} dBm)` end

  local noise = {
    toggle("Noise cancellation", anc.on, "anc"),
    slider("Ambient ↔ ANC", anc.level, 0, 100, 5, "%", "anc-level"),
    toggle("Superhuman Hearing", shh.on, "shh"),
  }
  return {
    ui.row({ gap = 8, padding = 12, radius = 12, fill = "surface_container/0.5" }, {
      battery("Headset", "headphones", s.battery, s.charging, low),
      -- the dock charges the spare until it is full
      battery("Spare in dock", "battery", tx.spare_battery, type(tx.spare_battery) == "number" and tx.spare_battery < 100, low),
    }),
    section("Noise", noise, "ear"),
    section("Microphone", {
      ui.row({ align = "center", gap = 8 }, {
        ui.glyph({ name = if mic.muted then "microphone-off" else "microphone", size = 16,
          color = if mic.muted then "error" else "on_surface" }),
        ui.label({ text = micText, color = "on_surface", flexGrow = 1 }),
      }),
    }, "microphone"),
    section("Volume", {
      row("Headset (system volume)", str(d.volume, "%")),
      ui.progress({ progress = (d.volume or 0) / 100, height = 4, radius = 2, fill = "primary" }),
      row("Discord (chat wheel)", str(d.chat_mix, "%")),
      ui.progress({ progress = (d.chat_mix or 0) / 100, height = 4, radius = 2, fill = "secondary" }),
    }, "volume"),
    section("Connection", { row(via, bt) }, "antenna-bars-5"),
  }
end

local function sound(d)
  local eq, shh = d.eq or {}, d.superhuman_hearing or {}
  return {
    section("Game EQ", eqRows(eq.game or {}, GAME_PRESETS, eq.custom_game, GAME_FREQS, "eq", "eq-band"), "adjustments-horizontal"),
    section("Superhuman Hearing", {
      toggle("On", shh.on, "shh"),
      choices(SHH_MODES, shh.mode, "shh-mode", 3),
      slider("Level", shh.level, 0, 100, 5, "%", "shh-level"),
    }, "ear-scan"),
    section("Loudness", {
      slider("Before the EQ (factory −12)", d.loudness_db, -24, 6, 1, " dB", "loudness"),
      ui.label({ text = "Stored on the headset. Boosting plus EQ can clip.", fontSize = 11 }),
    }, "volume"),
  }
end

local function micTab(d)
  local mic, gate, eq = d.mic or {}, d.noise_gate or {}, d.eq or {}
  local builtin = mic.source == "built-in"
  return {
    section("Microphone", {
      row("In use", if mic.source == "boom" then "Boom mic" elseif builtin then "Built-in mics" else "—"),
      slider("Monitoring (hear yourself)", mic.monitoring, 0, 100, 5, "%", "monitoring"),
      toggle("AI noise reduction", mic.noise_reduction, "noise-reduction"),
      toggle("Mute built-in mics", builtin and mic.muted or false, "mic-mute", builtin),
      row("Sensitivity (system mic level)", str(mic.sensitivity, "%")),
    }, "microphone"),
    section("Noise gate", {
      toggle("On", gate.on, "gate"),
      slider("Level", gate.level, 0, 100, 5, "%", "gate-level"),
    }, "wave-sine"),
    section("Mic EQ", eqRows(eq.mic or {}, MIC_PRESETS, eq.custom_mic, MIC_FREQS, "mic-eq", "mic-eq-band"), "adjustments-horizontal"),
  }
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

-- The power button's short press, set by `[buttons] power` in stealthbar's config file.
local function powerButton(action)
  if action == nil then return "—" end
  if action == "superhuman-hearing" then return "Superhuman Hearing" end
  if action == "none" then return "Nothing" end
  if action:find("media toggle") or action:find("play%-pause") then return "Play / pause" end
  return action
end

local function device(s, d)
  local headset, tx, power = d.headset or {}, d.transmitter or {}, d.power or {}
  local controls = d.controls or {}
  local wheel, button = controls.wheel or {}, controls.button or {}
  return {
    section("Controls", {
      row("Power button (short press)", powerButton(d.power_button)),
      select("Mode button", BUTTON, button.pc, "button"),
      select("Lower wheel", WHEEL, wheel.pc, "wheel"),
    }, "hand-click"),
    section("Power & lights", {
      select("Auto off", AUTO_OFF, power.auto_off, "auto-off"),
      toggle("Wake on motion", power.wake_on_motion, "wake-on-motion"),
      slider("Voice prompts", d.prompts, 0, 100, 5, "%", "prompts"),
      slider("Status light", tx.ring_light, 0, 100, 5, "%", "ring-light"),
      slider("Battery slot light", tx.slot_light, 0, 100, 5, "%", "slot-light"),
    }, "bulb"),
    section("Other platforms", {
      select("Mode button (PlayStation)", BUTTON, button.playstation, "button-playstation"),
      select("Mode button (Xbox)", BUTTON, button.xbox, "button-xbox"),
      select("Lower wheel (PlayStation)", WHEEL, wheel.playstation, "wheel-playstation"),
      select("Lower wheel (Xbox)", WHEEL, wheel.xbox, "wheel-xbox"),
    }, "device-gamepad-2"),
    section("About", {
      row("Name", str(headset.name)),
      row("Headset", `{str(headset.product)} · fw {str(headset.firmware)}`),
      row("Transmitter", `{str(tx.product or s.device)} · fw {str(tx.firmware)}`),
      row("Protocol", str(headset.protocol)),
    }, "info-circle"),
    section("Raw values", rawRows(d.raw), "code"),
  }
end

-- ── frame ────────────────────────────────────────────────────────────────────

local function header(s)
  local battery = if type(s.battery) == "number" then `{s.battery}%` else "—"
  return ui.row({ align = "center", gap = 10 }, {
    ui.glyph({ name = "headphones", size = 22, color = "primary" }),
    ui.label({ text = "Stealth Pro II", fontSize = 16, fontWeight = "bold", color = "on_surface", flexGrow = 1 }),
    ui.label({ text = battery, fontSize = 16, fontWeight = "bold", color = "on_surface" }),
  })
end

-- Fixed above the scroll area: text tabs with an accent underline on the active one, over a hairline.
local function tabBar()
  local tabs = {}
  for _, t in TABS do
    local id, name = t[1], t[2]
    local selected = activeTab == id
    table.insert(tabs, ui.column({
      key = "tab-" .. id, align = "center", gap = 6, flexGrow = 1,
      onClick = function()
        if activeTab ~= id then
          activeTab = id
          render()
        end
      end,
    }, {
      ui.label({
        text = name, fontSize = 13,
        fontWeight = if selected then "bold" else "medium",
        color = if selected then "primary" else "on_surface_variant",
      }),
      ui.box({ width = 48, height = 3, radius = 2, fill = if selected then "primary" else "on_surface/0" }),
    }))
  end
  return ui.column({ gap = 0, align = "stretch" }, {
    ui.row({ gap = 4 }, tabs),
    ui.separator({ thickness = 1, color = "on_surface/0.12" }),
  })
end

function render()
  local s = if type(status) == "table" then status else { present = false, on = false }
  local children = { header(s) }
  if not s.on then
    table.insert(children, ui.label({
      text = if s.present
        then "Headset is off. If it is on, the dock is stuck: flip its Xbox / other switch to Xbox and back, or replug it."
        else "No Stealth Pro II dock found.",
      color = "on_surface_variant",
    }))
  else
    local d = s.details or {}
    local content
    if activeTab == "sound" then
      content = sound(d)
    elseif activeTab == "mic" then
      content = micTab(d)
    elseif activeTab == "device" then
      content = device(s, d)
    else
      content = overview(s, d)
    end
    table.insert(children, tabBar())
    table.insert(children, ui.scroll({ flexGrow = 1, gap = 12, align = "stretch" }, content))
  end
  panel.render(ui.column({ flexGrow = 1, gap = 12, align = "stretch" }, children))
end

noctalia.state.watch("stealthbar.status", function(value)
  status = value
  render()
end)
render()
