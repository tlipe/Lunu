local RUNTIME = "lune"
if _G.lute or pcall(require, "@lute/process") then RUNTIME = "lute" end

local process_mod, fs_impl, json_impl

if RUNTIME == "lute" then
    process_mod = require("@lute/process")
    fs_impl = require("@std/fs")
    json_impl = require("@std/json")
else
    process_mod = require("@lune/process")
    fs_impl = require("@lune/fs")
    local serde = require("@lune/serde")
    json_impl = {
        serialize = function(v) return serde.encode("json", v) end,
        deserialize = function(s) return serde.decode("json", s) end
    }
end

local fs = {}
if RUNTIME == "lute" then
    fs.isDir = function(p)
        if not fs_impl.exists(p) then return false end
        local ok, meta = pcall(fs_impl.metadata, p)
        return ok and meta.kind == "dir"
    end
    fs.isFile = function(p)
        if not fs_impl.exists(p) then return false end
        local ok, meta = pcall(fs_impl.metadata, p)
        return ok and meta.kind == "file"
    end
    fs.readFile = fs_impl.readfiletostring
else
    fs.isDir = fs_impl.isDir
    fs.isFile = fs_impl.isFile
    fs.readFile = fs_impl.readFile
end

local function get_cwd()
    if RUNTIME == "lute" then
        return process_mod.cwd()
    else
        return process_mod.cwd
    end
end

local function resolve_path(p)
    if p == "." then return get_cwd() end
    if string.match(p, "^%./") then
        return get_cwd() .. string.sub(p, 2)
    end
    if string.match(p, "^%.%./") or not string.match(p, "^%a:") then
        return `{get_cwd()}/{p}`
    end
    return p
end

local function is_abs_path(path)
    return string.match(path, "^%a:") ~= nil or string.match(path, "^/") ~= nil or string.match(path, "^\\") ~= nil
end

local function join_path(a, b)
    local last = string.sub(a, -1)
    if last == "/" or last == "\\" then
        return `{a}{b}`
    end
    return `{a}/{b}`
end

-- Lazy state
local _LUNU_DIR = nil
local _config = nil

local function get_lunu_dir()
    if _LUNU_DIR then return _LUNU_DIR end
    
    local possible_paths = {
        "Lunu",
        "./Lunu",
        "../Lunu",
        "../../Lunu",
        "."
    }

    for _, p in possible_paths do
        if fs.isDir(p) and (fs.isFile(`{p}/config/settings.json`) or fs.isDir(`{p}/src/bridge`)) then
            _LUNU_DIR = resolve_path(p)
            break
        end
    end

    if not _LUNU_DIR then
        if fs.isFile("config/settings.json") then
            _LUNU_DIR = resolve_path(".")
        elseif fs.isFile("bin/lunu-bridge.exe") then
            _LUNU_DIR = resolve_path(".")
        else
             _LUNU_DIR = resolve_path(".")
        end
    end

    if not string.match(_LUNU_DIR, "/$") then _LUNU_DIR = `{_LUNU_DIR}/` end
    return _LUNU_DIR
end

local function get_config()
    if _config then return _config end
    
    local lunu_dir = get_lunu_dir()
    local settings_path = `{lunu_dir}config/settings.json`
    
    _config = { modules_dir = "modules" }
    
    if fs.isFile(settings_path) then
        local ok, data = pcall(function() return json_impl.deserialize(fs.readFile(settings_path)) end)
        if ok and data.modules_dir then
            _config.modules_dir = data.modules_dir
        end
    end
    return _config
end

local function modules_root()
    local cfg = get_config()
    local lunu_dir = get_lunu_dir()
    local candidate = cfg.modules_dir or "modules"
    local resolved = is_abs_path(candidate) and candidate or join_path(lunu_dir, candidate)
    if fs.isDir(resolved) then
        return resolved
    end
    return join_path(lunu_dir, "modules")
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
        error(`[Lunu] Bridge config not found: {cfg_path}`)
    end
    local content = fs.readFile(cfg_path)
    local ok, cfg = pcall(function() return json_impl.deserialize(content) end)
    if not ok then
        error(`[Lunu] Invalid bridge config: {cfg_path}`)
    end
    return cfg
end

local function run_worker(exec_path, cmd_args, cwd, env, request_line)
    if RUNTIME == "lune" then
        local result = process_mod.exec(exec_path, cmd_args, {
            cwd = cwd,
            env = env,
            stdio = {
                stdin = request_line,
                stdout = "default",
                stderr = "default"
            }
        })
        return {
            ok = result.ok,
            stdout = result.stdout or "",
            stderr = result.stderr or ""
        }
    else
        -- Lute implementation
        local temp_file = `lunu_req_{tostring(math.random(1000000))}.json`
        fs_impl.writestringtofile(temp_file, request_line)
        
        local full_cmd = `"{exec_path}"`
        for _, arg in cmd_args do
            full_cmd = `{full_cmd} "{arg}"`
        end
        full_cmd = `{full_cmd} < {temp_file}`
        
        local res = process_mod.run({"cmd", "/c", full_cmd}, {
            cwd = cwd,
            env = env
        })
        
        if fs_impl.remove then fs_impl.remove(temp_file) end
        
        return {
            ok = (res.exitcode == 0),
            stdout = res.stdout,
            stderr = res.stderr
        }
    end
end

local function normalize_separators(p)
    -- Lute might not have package.config, assume windows if \\ is present in other paths or check _G.lute
    if RUNTIME == "lute" then
        return string.gsub(p, "/", "\\")
    end
    return p
end

local function local_bridge_call(module_name, func_name, args)
    local root = modules_root()
    -- print("[DEBUG] modules_root:", root)
    local module_dir = join_path(root, module_name)
    module_dir = normalize_separators(module_dir)
    -- print("[DEBUG] module_dir:", module_dir)
    
    if not fs.isDir(module_dir) then
        error(`[Lunu] Module not found: {module_name} at {module_dir}`)
    end
    local cfg = load_bridge_config(module_dir)
    if not cfg.worker or not cfg.worker.cmd or #cfg.worker.cmd == 0 then
        error(`[Lunu] Invalid worker command for module: {module_name}`)
    end
    if cfg.methods and cfg.methods[func_name] == nil then
        error(`[Lunu] Function not found: {func_name}`)
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
    local request_line = json_impl.serialize(request) .. "\n"
    
    local result = run_worker(exec_path, cmd_args, resolve_cwd_path(module_dir, cfg.worker.cwd), cfg.worker.env or {}, request_line)
    
    if not result.ok then
        error(`[Lunu] Worker error: {result.stderr or "Worker process failed"}`)
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
        error(`[Lunu] Worker returned no response. Stderr: {result.stderr}`)
    end
    local ok, response = pcall(function() return json_impl.deserialize(response_line) end)
    if not ok then
        error(`[Lunu] Worker returned invalid JSON: {response_line}`)
    end
    if response.error then
        local message = response.error.message or "Worker error"
        error(`[Lunu] Worker error: {message}`)
    end
    return response.result
end

local function call(endpoint, func_name, ...)
    local args = {...}
    return local_bridge_call(endpoint, func_name, args)
end

return {
    call = call,
    is_alive = function()
        return true
    end
}
