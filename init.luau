local process = require("@lune/process")
local net = require("@lune/net")
local serde = require("@lune/serde")
local fs = require("@lune/fs")
--local task = require("@lune/task")

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
}

for _, p in possible_paths do
    if fs.isDir(p) and fs.isFile(p .. "/config/settings.json") then
        LUNU_DIR = resolve_path(p)
        break
    end
end

if not LUNU_DIR then
    error("[Lunu Client] CRITICAL: Could not locate the folder 'Lunu'. Make sure you are running from the root of the project.")
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
        return net.request({
            url = `{BASE_URL}/health`,
            method = "GET",
            timeout = 0.5 -- Fast Timeout
        })
    end)
    return ok and response.ok
end

local function start_server()
    error("\n[Lunu] The Bridge server is offline.\nPlease run the command below in another terminal:\n    lunu dev\n")
end

local function ensure_connection()
    if not is_server_alive() then
        start_server()
    end
end

local function call(endpoint: string, func_name: string, args: {any})
    ensure_connection()

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
