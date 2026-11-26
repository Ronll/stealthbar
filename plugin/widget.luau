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

  local tip = { "Stealth Pro II", if battery then `Battery {battery}%` else "Battery unknown" }
  if s.charging then table.insert(tip, "Charging") end
  table.insert(tip, if s.mic_muted then "Mic muted" else "Mic on")
  barWidget.setTooltip(table.concat(tip, "\n"))
  barWidget.setVisible(true)
end

barWidget.setVisible(false)
if not noctalia.runStream(command, apply) then
  noctalia.notifyError("Stealth Pro II", `Could not start: {command}`)
end
