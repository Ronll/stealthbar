--!nonstrict
-- Bar capsule: hidden unless the headset is on; click opens the panel with everything the daemon reports.

local low = tonumber(noctalia.getConfig("low_battery")) or 15

local function render(s)
  if type(s) ~= "table" or not s.on then
    barWidget.setVisible(false)
    return
  end

  local battery = if type(s.battery) == "number" then s.battery else nil
  if s.mic_muted then
    barWidget.setGlyph("microphone-off")
  elseif s.charging then
    barWidget.setGlyph("battery-charging")
  else
    barWidget.setGlyph("headphones")
  end
  barWidget.setText(if battery then `{battery}%` else "")
  if battery and battery <= low and not s.charging then
    barWidget.setGlyphColor("error")
  else
    barWidget.setGlyphColor("on_surface")
  end

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
