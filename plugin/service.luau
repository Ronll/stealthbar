--!nonstrict
-- The only owner of the daemon: one `stealthbar watch` for the whole shell (two readers would steal each
-- other's replies from the dock). Each line is the whole state; it is published as `stealthbar.status` for
-- the bar widget(s) and the panel. Changes from the panel arrive as `stealthbar.set` and go to the daemon
-- through `stealthbar set` (its control socket), so the daemon stays the dock's only reader.

local command = noctalia.getConfig("command") or "stealthbar watch"
-- the same binary, `set` instead of `watch`
local binary = command:gsub("%s+watch%s*$", "")

local function publish(line: string)
  local ok, status = pcall(noctalia.json.decode, line)
  if ok and type(status) == "table" then
    noctalia.state.set("stealthbar.status", status)
  end
end

local function quote(word: string): string
  return "'" .. word:gsub("'", "'\\''") .. "'"
end

noctalia.state.watch("stealthbar.set", function(request)
  if type(request) ~= "table" or type(request.setting) ~= "string" then return end
  local words = { binary, "set", quote(request.setting) }
  for _, v in request.values or {} do table.insert(words, quote(tostring(v))) end
  noctalia.runAsync(table.concat(words, " "), function(result)
    if result.exitCode ~= 0 then
      local detail = noctalia.string.trim(result.stderr or "")
      noctalia.notifyError("Stealth Pro II", if detail == "" then "Setting failed" else detail)
    end
  end)
end)

noctalia.state.set("stealthbar.status", { present = false, on = false })
if not noctalia.runStream(command, publish) then
  noctalia.notifyError("Stealth Pro II", `Could not start: {command}`)
end
