--!nonstrict
-- The only owner of the daemon: one `stealthbar watch` for the whole shell (two readers would steal each
-- other's replies from the dock). Each line is the whole state; it is published as `stealthbar.status` for
-- the bar widget(s) and the panel.

local command = noctalia.getConfig("command") or "stealthbar watch"

local function publish(line: string)
  local ok, status = pcall(noctalia.json.decode, line)
  if ok and type(status) == "table" then
    noctalia.state.set("stealthbar.status", status)
  end
end

noctalia.state.set("stealthbar.status", { present = false, on = false })
if not noctalia.runStream(command, publish) then
  noctalia.notifyError("Stealth Pro II", `Could not start: {command}`)
end
