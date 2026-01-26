local process = require("@lune/process")
local serde = require("@lune/serde")
local fs = require("@lune/fs")

local function resolve_path(p)
    if string.match(p, "^%./") or string.match(p, "^%.%./") or not string.match(p, "^%a:") then
        return process.cwd .. "/" .. p
    end
    return p
end

local LUNU_DIR = nil

local possible_paths = {
    "Lunu",
    "./Lunu",
    "../Lunu",
    "../../Lunu",
    "."
}

for _, p in possible_paths do
    if fs.isDir(p) and (fs.isFile(p .. "/config/settings.json") or fs.isDir(p .. "/src/bridge")) then
        LUNU_DIR = resolve_path(p)
        break
    end
end

if not LUNU_DIR then
    if fs.isFile("config/settings.json") then
        LUNU_DIR = resolve_path(".")
    elseif fs.isFile("bin/lunu-bridge.exe") then
        LUNU_DIR = resolve_path(".")
    else
         LUNU_DIR = resolve_path(".")
    end
end

if not string.match(LUNU_DIR, "/$") then LUNU_DIR = LUNU_DIR .. "/" end

local SETTINGS_PATH = LUNU_DIR .. "config/settings.json"
local SECRETS_PATH = LUNU_DIR .. "config/.secrets.json"

local config = {
    modules_dir = "modules"
}

if fs.isFile(SETTINGS_PATH) then
    local ok, data = pcall(function() return serde.decode("json", fs.readFile(SETTINGS_PATH)) end)
    if ok and data.modules_dir then
        config.modules_dir = data.modules_dir
    end
end

local function is_abs_path(path)
    return string.match(path, "^%a:") ~= nil or string.match(path, "^/") ~= nil
end

local function join_path(a, b)
    if string.sub(a, -1) == "/" then
        return a .. b
    end
    return a .. "/" .. b
end

local function modules_root()
    local candidate = config.modules_dir or "modules"
    local resolved = is_abs_path(candidate) and candidate or join_path(LUNU_DIR, candidate)
    if fs.isDir(resolved) then
        return resolved
    end
    return join_path(LUNU_DIR, "modules")
end

local function resolve_exec_path(module_dir, exec_path)
    if is_abs_path(exec_path) then
        return exec_path
    end
    local local_path = join_path(module_dir, exec_path)
    if fs.isFile(local_path) then
        return local_path
    end
    return exec_path
end

local function resolve_cwd_path(module_dir, cwd)
    if not cwd then
        return module_dir
    end
    if is_abs_path(cwd) then
        return cwd
    end
    return join_path(module_dir, cwd)
end

local function load_bridge_config(module_dir)
    local cfg_path = join_path(module_dir, "bridge.json")
    if not fs.isFile(cfg_path) then
        error("[Lunu] Bridge config not found: " .. cfg_path)
    end
    local content = fs.readFile(cfg_path)
    local ok, cfg = pcall(function() return serde.decode("json", content) end)
    if not ok then
        error("[Lunu] Invalid bridge config: " .. cfg_path)
    end
    return cfg
end

local function local_bridge_call(module_name, func_name, args)
    local module_dir = join_path(modules_root(), module_name)
    if not fs.isDir(module_dir) then
        error("[Lunu] Module not found: " .. module_name)
    end
    local cfg = load_bridge_config(module_dir)
    if not cfg.worker or not cfg.worker.cmd or #cfg.worker.cmd == 0 then
        error("[Lunu] Invalid worker command for module: " .. module_name)
    end
    if cfg.methods and cfg.methods[func_name] == nil then
        error("[Lunu] Function not found: " .. func_name)
    end
    local exec_path = resolve_exec_path(module_dir, cfg.worker.cmd[1])
    local cmd_args = {}
    for i = 2, #cfg.worker.cmd do
        table.insert(cmd_args, cfg.worker.cmd[i])
    end
    local request = {
        id = tostring(math.random(1000000000)),
        method = func_name,
        params = args
    }
    local request_line = serde.encode("json", request) .. "\n"
    local result = process.exec(exec_path, cmd_args, {
        cwd = resolve_cwd_path(module_dir, cfg.worker.cwd),
        env = cfg.worker.env or {},
        stdio = {
            stdin = request_line,
            stdout = "default",
            stderr = "default"
        }
    })
    if not result.ok then
        error("[Lunu] Worker error: " .. (result.stderr or "Worker process failed"))
    end
    local output = result.stdout or ""
    local response_line = nil
    for line_value in string.gmatch(output, "[^\r\n]+") do
        if line_value ~= "" then
            response_line = line_value
            break
        end
    end
    if response_line == nil or response_line == "" then
        error("[Lunu] Worker returned no response")
    end
    local ok, response = pcall(function() return serde.decode("json", response_line) end)
    if not ok then
        error("[Lunu] Worker returned invalid JSON")
    end
    if response.error then
        local message = response.error.message or "Worker error"
        error("[Lunu] Worker error: " .. message)
    end
    return response.result
end

local function call(endpoint: string, func_name: string, ...: any)
    local args = {...}
    return local_bridge_call(endpoint, func_name, args)
end

return {
    call = call,
    is_alive = function()
        return true
    end
}
