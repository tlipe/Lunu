local process = require("@lune/process")
local net = require("@lune/net")
local serde = require("@lune/serde")
local fs = require("@lune/fs")
local task = require("@lune/task")

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
    -- Fallback: If we are running inside the compiled exe, Lunu is embedded at root or src/libs
    if fs.isFile("config/settings.json") then
        LUNU_DIR = resolve_path(".")
    elseif fs.isFile("src/bridge/server.py") then
        LUNU_DIR = resolve_path(".")
    else
         -- Last resort, maybe we are in Lunu folder itself
         LUNU_DIR = resolve_path(".")
    end
    -- error("[Lunu Client] CRITICAL: Could not locate the folder 'Lunu'. Make sure you are running from the root of the project.")
end

if not string.match(LUNU_DIR, "/$") then LUNU_DIR = LUNU_DIR .. "/" end

local SETTINGS_PATH = LUNU_DIR .. "config/settings.json"
local SECRETS_PATH = LUNU_DIR .. "config/.secrets.json"

local config = {
    port = 8000,
    api_key = ""
}

if fs.isFile(SETTINGS_PATH) then
    local ok, data = pcall(function() return serde.decode("json", fs.readFile(SETTINGS_PATH)) end)
    if ok and data.server and data.server.http_port then
        config.port = data.server.http_port
    end
end

if fs.isFile(SECRETS_PATH) then
    local ok, secrets = pcall(function() return serde.decode("json", fs.readFile(SECRETS_PATH)) end)
    if ok then
        config.api_key = secrets.api_key
    end
end

local BASE_URL = `http://127.0.0.1:{config.port}`

local function is_server_alive()
    local ok, response = pcall(function()
        local target_url = `{BASE_URL}/health`
        return net.request({
            url = target_url,
            method = "GET",
            timeout = 1
        })
    end)
    return ok and response.ok
end

local function start_server()
    print("[Lunu] Attempting to start local Bridge server...")
    
    local server_script = "./src/bridge/server.py"
    
    if not fs.isFile(server_script) then
        if LUNU_DIR and fs.isFile(LUNU_DIR .. "src/bridge/server.py") then
            server_script = LUNU_DIR .. "src/bridge/server.py"
        elseif fs.isFile("Lunu/src/bridge/server.py") then
            server_script = "Lunu/src/bridge/server.py"
        end
    end
    
    if not fs.isFile(server_script) then
         error("\n[Lunu] The Bridge server is offline and could not be auto-started.\nServer script not found at: " .. server_script .. "\nPlease run: lunu dev")
    end

    print("[Lunu] Spawning Python Bridge: " .. server_script)

    local log_file = "server_startup.log"
    local python_cmd = "python"
    if process.env and process.env.LUNU_PYTHON and #process.env.LUNU_PYTHON > 0 then
        python_cmd = process.env.LUNU_PYTHON
    end
    local server_script_win = string.gsub(server_script, "/", "\\")
    if string.sub(server_script_win, 1, 2) == ".\\" then
        server_script_win = string.sub(server_script_win, 3)
    end
    
    local bat_content = "@echo off\r\n\"" .. python_cmd .. "\" \"" .. server_script_win .. "\" > " .. log_file .. " 2>&1"
    fs.writeFile("run_server.cmd", bat_content)

    local child = process.create("cmd", { "/C", "run_server.cmd" }, {
        cwd = process.cwd,
        stdio = "default"
    })

    print("[Lunu] Waiting for Bridge server to respond...")
    task.wait(2)
    
    for i = 1, 40 do
        if is_server_alive() then
            print("[Lunu] Bridge server connected!")
            return
        end
        if i % 5 == 0 then
             print("[Lunu] Still waiting for server... (" .. i .. "/40)")
        end
        task.wait(0.5)
    end
    
    local log_content = "No log generated."
    if fs.isFile(log_file) then
        log_content = fs.readFile(log_file)
    end
    
    error("[Lunu] Failed to connect to Bridge server.\nLog Output:\n" .. log_content)
end

local function ensure_connection()
    if not is_server_alive() then
        start_server()
    end
end

local function call(endpoint: string, func_name: string, ...: any)
    ensure_connection()

    local args = {...}
    local payload = serde.encode("json", { args = args })
    local url = `{BASE_URL}/api/v1/{endpoint}/{func_name}`

    local response = net.request({
        url = url,
        method = "POST",
        headers = {
            ["Content-Type"] = "application/json",
            ["X-LUNU-KEY"] = config.api_key
        },
        body = payload
    })

    if not response.ok then
        error(`[Lunu] Request error ({response.statusCode}): {response.body}`)
    end

    local body = serde.decode("json", response.body)
    return body.result
end

return {
    call = call,
    is_alive = is_server_alive
}
