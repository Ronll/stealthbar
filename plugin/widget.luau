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
  -- Compact, like the AI Usage capsule: a big headset glyph, and beside it the mic state above the battery.
  local mic = ui.glyph({
    name = if s.mic_muted then "microphone-off" else "microphone",
    size = 12, width = 12, height = 12,
    color = if s.mic_muted then "error" else "on_surface/0.8",
  })
  local level = { ui.label({
    text = if battery then `{battery}%` else "—",
    fontSize = 10, maxLines = 1,
    color = if low_now then "error" else "on_surface/0.8",
  }) }
  if s.charging then
    table.insert(level, 1, ui.glyph({ name = "bolt", size = 10, width = 10, height = 10, color = "on_surface/0.8" }))
  end
  barWidget.render(ui.row({ gap = 3, align = "center" }, {
    ui.glyph({ name = "headphones", size = 24, width = 24, height = 24, color = if low_now then "error" else nil }),
    ui.column({ gap = 0, align = "center" }, { mic, ui.row({ gap = 0, align = "center" }, level) }),
  }))

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
