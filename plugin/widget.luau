--!nonstrict
-- One `stealthbar watch` stream per widget (the host kills it with the widget). Each line is the whole
-- state; the widget is hidden unless the headset is on.

local command = noctalia.getConfig("command") or "stealthbar watch"
local low = tonumber(noctalia.getConfig("low_battery")) or 15

local function apply(line: string)
  local ok, s = pcall(noctalia.json.decode, line)
  if not ok or type(s) ~= "table" then return end
  if not s.on then
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

barWidget.setVisible(false)
if not noctalia.runStream(command, apply) then
  noctalia.notifyError("Stealth Pro II", `Could not start: {command}`)
end
