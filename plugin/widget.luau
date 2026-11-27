--!nonstrict
-- Bar capsule: hidden unless the headset is on; click opens the panel with everything the daemon reports.

local low = tonumber(noctalia.getConfig("low_battery")) or 15

local function render(s)
  if type(s) ~= "table" or not s.on then
    barWidget.setVisible(false)
    return
  end

  local battery = if type(s.battery) == "number" then s.battery else nil
  local low_now = battery ~= nil and battery <= low and not s.charging
  local children = { ui.glyph({ name = "headphones", color = if low_now then "error" else nil }) }
  if battery then
    table.insert(children, ui.label({ text = `{battery}%`, color = if low_now then "error" else nil }))
  end
  if s.charging then
    table.insert(children, ui.glyph({ name = "bolt" }))
  end
  if s.mic_muted then
    table.insert(children, ui.glyph({ name = "microphone-off", color = "error" }))
  end
  barWidget.render(ui.row({ gap = 4, align = "center" }, children))

  barWidget.setTooltip({
    { key = "Stealth Pro II battery", value = if battery then `{battery}%` else "unknown" },
    { key = "Charging", value = if s.charging then "yes" else "no" },
    { key = "Mic", value = if s.mic_muted then "muted" else "on" },
  })
  barWidget.setVisible(true)
end

noctalia.state.watch("stealthbar.status", render)
render(noctalia.state.get("stealthbar.status"))

function onClick()
  noctalia.togglePanel("ronll/stealthbar:panel")
end
