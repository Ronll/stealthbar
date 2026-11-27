--!nonstrict
-- Read-only overview of everything `stealthbar` reports (the `details` object, see README "Output format").

local status = noctalia.state.get("stealthbar.status")

local function str(v, suffix)
  if v == nil then return "—" end
  if type(v) == "boolean" then return if v then "on" else "off" end
  return tostring(v) .. (suffix or "")
end

local function onLevel(t, suffix)
  if type(t) ~= "table" or t.on == nil then return "—" end
  if not t.on then return "off" end
  return if t.level ~= nil then `on · {t.level}{suffix or "%"}` else "on"
end

local function row(key, value)
  return ui.row({ align = "center", gap = 12 }, {
    ui.label({ text = key, color = "on_surface_variant", flexGrow = 1 }),
    ui.label({ text = value, color = "on_surface" }),
  })
end

local function section(title, rows)
  local children = { ui.label({ text = title, fontWeight = "semibold", color = "primary" }) }
  for _, r in rows do table.insert(children, r) end
  return ui.column({ gap = 6, align = "stretch" }, children)
end

local function header(s)
  local battery = if type(s.battery) == "number" then `{s.battery}%` else "—"
  return ui.row({ align = "center", gap = 10 }, {
    ui.glyph({ name = "headphones", size = 22, color = "primary" }),
    ui.label({ text = "Stealth Pro II", fontSize = 16, fontWeight = "bold", color = "on_surface", flexGrow = 1 }),
    ui.label({ text = battery, fontSize = 16, fontWeight = "bold", color = "on_surface" }),
  })
end

local function body(s)
  local d = s.details or {}
  local headset, tx, mic = d.headset or {}, d.transmitter or {}, d.mic or {}
  local shh, eq, controls, power = d.superhuman_hearing or {}, d.eq or {}, d.controls or {}, d.power or {}

  local shhText = onLevel(shh)
  if shh.on and shh.mode then shhText = `{shh.mode} · {shh.level or "—"}%` end

  local autoOff = power.auto_off_minutes
  local autoOffText = if autoOff == 0 then "never" elseif autoOff then `{autoOff} min` else "—"

  return {
    section("Battery", {
      row("Headset", str(s.battery, "%")),
      row("Charging", if s.charging then "yes" else "no"),
      row("Spare in dock", str(tx.spare_battery, "%")),
    }),
    section("Microphone", {
      row("In use", str(mic.source)),
      row("Muted", if mic.muted then "yes" else "no"),
      row("Sensitivity", str(mic.sensitivity, "%")),
      row("Monitoring", str(mic.monitoring, "%")),
      row("AI noise reduction", str(mic.noise_reduction)),
      row("Noise gate", onLevel(d.noise_gate)),
    }),
    section("Sound", {
      row("Volume", str(d.volume, "%")),
      row("Chat wheel", str(d.chat_mix, "%")),
      row("Noise cancellation", onLevel(d.anc)),
      row("Superhuman Hearing", shhText),
      row("Game EQ", str(eq.game)),
      row("Mic EQ", str(eq.mic)),
    }),
    section("Controls", {
      row("Lower wheel", str(controls.wheel)),
      row("Mode button", str(controls.button)),
      row("Auto off", autoOffText),
      row("Wake on motion", str(power.wake_on_motion)),
    }),
    section("Device", {
      row("Name", str(headset.name)),
      row("Headset", `{str(headset.product)} · fw {str(headset.firmware)}`),
      row("Transmitter", `{str(tx.product or s.device)} · fw {str(tx.firmware)}`),
    }),
  }
end

local function render()
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
    ui.scroll({ flexGrow = 1, gap = 14, align = "stretch" }, content),
  }))
end

noctalia.state.watch("stealthbar.status", function(value)
  status = value
  render()
end)
render()
