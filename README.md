# Lunu

**Lunu** is a robust toolchain and library manager for **Luau** (powered by the **Lune** runtime). It provides a complete set of tools to develop, manage dependencies, and compile Luau projects into standalone native executables.

Made in Rust 🦀

## Features

- **Dependency Management**: Easily install, update, and remove libraries (similar to `npm` or `cargo`).
- **Build System**: Compile your Luau scripts (`.luau`) into native executables (`.exe`) that run on any Windows machine. **The builder is built-in**, so you don't need external tools.
- **Bridge Runtime**: Direct stdin/stdout worker execution for Python, Node.js, Rust, and more.
- **Single-File Distribution**: The entire toolchain (Manager, Builder, Bridge) is contained in a **single executable** (`lunu.exe`).

---

## Installation

1. Download the **`lunu.exe`** file from the Releases section.
2. Run `lunu.exe` in your terminal or double-click it.
   - On the first run, it acts as an **installer**, extracting itself to `~/.lunu/bin` and configuring your system PATH.
3. Restart your terminal.
4. Type `lunu --help` to verify the installation.

---

## Usage

### 1. Creating a Project
Start a new project in an empty folder or create a new one:

```bash
# Create a new project
lunu create my-project

# Or initialize in the current folder
lunu init
```

### 2. Managing Libraries
Add libraries from GitHub or the central registry:

```bash
# Add from a specific repository
lunu add user/repo

# Remove a library
lunu remove lib-name
```

### 3. Compiling to Executable (.exe)
Turn your main script into a standalone program:

```bash
lunu build main.luau
```
This will generate a `main.exe` file in the same folder. This executable contains the Lune runtime, your dependencies, and your script, all embedded.

### 4. Bridge Runtime
The Bridge is a tooling integration layer. It starts external workers only when a bridge call happens and exchanges data over stdin/stdout for that call. This path is for build, analysis, lint, debug, hot reload, and optional integrations. It is not part of the gameplay/runtime-critical loop, not an engine execution path, and not a replacement for FFI. For advanced debugging and tooling, use `lunu dev` to start the HTTP server in the foreground.

**Foreground Benefits**
- **Software development**: fast iteration, predictable logs, and simpler local setup.
- **Game development**: deterministic startup, easier profiling, and fewer background conflicts.
- **Tooling & automation**: portable executions in CI, clean teardown per call, and fewer environment dependencies.

### CLI Reference

- `lunu init` - Initialize a project in the current directory.
- `lunu create <name>` - Create and initialize a new project folder.
- `lunu add <user/repo>` - Add a dependency from GitHub.
- `lunu remove <name>` - Remove a dependency.
- `lunu install` - Install dependencies from `lunu.toml`.
- `lunu update [name]` - Update dependencies.
- `lunu list` - List installed dependencies.
- `lunu build <entry.luau>` - Create a standalone executable with brute optimization in C
- `lunu package` - Create a distributable bundle.
- `lunu check` - Validate project environment.
- `lunu dev` - Start the HTTP bridge server in the foreground.
- `lunu scaffold <name> --template <app|game>` - Scaffold a new project.
- `lunu module <name> --lang <python|node>` - Create a bridge module scaffold.
- `lunu profile <script> --runs <n>` - Profile a script with Lune.
- `lunu upgrade` - Upgrade the CLI.
- `lunu uninstall` - Uninstall the CLI.

---

## Polyglot Development (Connecting External Languages)

Lunu allows you to extend your Luau applications with **any language** (Python, Rust, C++, Node.js) using the **Bridge System**.

### How it works
1. You create a module in the `modules/` directory.
2. You define a `bridge.json` file mapping function names to system commands.
3. Your Luau script calls these functions via the Lunu Bridge API.
4. **Communication**: On a bridge call, the worker is spawned and JSON-RPC is exchanged via **Standard Input/Output (stdin/stdout)** for that call.

The Bridge is a dev/tooling path, not a runtime-critical execution path.

### Example: Connecting Python

1. Create a folder `modules/my-python-lib`.
2. Create your Python script `worker.py` inside it. It must read JSON from stdin and write JSON to stdout:
   ```python
   import sys
   import json

   def normalize_params(params):
       if len(params) == 1 and isinstance(params[0], list):
           return params[0]
       return params

   def handle(method, params):
       params = normalize_params(params)
       if method == "greet":
           name = params[0] if len(params) > 0 else ""
           return {"result": f"Hello from Python, {name}!"}
       return {"error": {"code": "404", "message": "Method not found"}}

   def main():
       while True:
           line = sys.stdin.readline()
           if not line: break
           
           msg = json.loads(line)
           req_id = msg.get("id")
           method = msg.get("method")
           params = msg.get("params", [])
           
           response = {"id": req_id}
           response.update(handle(method, params))
               
           print(json.dumps(response))
           sys.stdout.flush()

   if __name__ == "__main__":
       main()
   ```
3. Create a `bridge.json` file in the same folder:
   ```json
   {
     "worker": {
       "cmd": ["python", "worker.py"],
       "timeout_ms": 5000
     },
     "methods": {
       "greet": {"timeout_ms": 1000}
     }
   }
   ```
4. In your Luau code (`init.luau`):
   ```lua
   local lunu = require("@lunu")
   
   -- Calls 'greet' command in 'my-python-lib' module
   local result = lunu.call("my-python-lib", "greet", "Lunu User")
   print(result) -- Output: Hello from Python, Lunu User!
   ```

---

### Example: Connecting Node.js

1. Create a folder `modules/my-node-lib`.
2. Create your JavaScript file `worker.js` inside it:
   ```javascript
   const readline = require('readline');
   const rl = readline.createInterface({input: process.stdin, output: process.stdout});

   rl.on('line', (line) => {
       if (!line) return;
       const msg = JSON.parse(line);
       const response = {id: msg.id};

       if (msg.method === 'greet') {
       response.result = `Hello from Node.js, ${msg.params[0]}!`;
       } else {
           response.error = {code: '404', message: 'Not found'};
       }
       console.log(JSON.stringify(response));
   });
   ```
3. Create a `bridge.json` file:
   ```json
   {
     "worker": {"cmd": ["node", "worker.js"]},
     "methods": {"greet": {}}
   }
   ```

---

### 5. CLI Utilities

Project scaffolding:

```bash
lunu scaffold my-app --template app
lunu scaffold my-game --template game
```

Bridge module scaffolding:

```bash
lunu module my-python-lib --lang python
lunu module my-node-lib --lang node
```

Lightweight profiling:

```bash
lunu profile src/main.luau --runs 5
```

## Builder Optimizations & Binary Size

Lunu Builder now includes advanced optimizations to produce the smallest possible executables.

### Size Comparison (Benchmark)

| Implementation | Binary Size (Stub) | Reduction |
|----------------|-------------------|-----------|
| **Legacy Rust** | ~950 KB           | 0%        |
| **Optimized Rust** (Current) | ~304 KB | **~68%** |
| **Experimental C** | < 50 KB | ~95% |

### Optimized Rust Stub (Default)
The default builder uses a highly optimized Rust implementation (`opt-level='z'`, `lto=true`, `panic='abort'`, `strip=true`). This delivers a ~300KB stub overhead, which is negligible for most applications compared to the Lune runtime size.

### Experimental C Stub
For users demanding absolute minimal size, a C implementation is available in `builder/src/stub.c`.
To use it:
1. Compile `stub.c` using GCC/MSVC to `lunu-stub.exe`.
2. Replace the embedded stub in the toolchain or manually place it in `bin/lunu-stub.exe`.
See `builder/BUILD_C.md` for details.

---

## Security & Performance

### `toolchain/`
The "brain" of Lunu. Contains the **Rust** source code for:
- **CLI (`lunu-cli`)**: The main command-line tool.
- **Bridge**: The HTTP server.
- **Builder Integration**: The builder logic is now embedded directly into the CLI.

### `builder/`
The compilation system source code. It is compiled as a library and embedded into the main `lunu` binary.

### Bridge Runtime
The Bridge runs inside the runtime and spawns workers per call.
- **Security**:
  - **Path Validation**: Workers run only within the module or allowed paths.
  - **Isolation**: Each call starts its own process.
- **Performance**:
  - **Zero-Copy**: Lightweight stdin/stdout communication.
  - **Simplicity**: Fewer moving parts and a smaller operational footprint.

## License

This project is licensed under the **Mozilla Public License 2.0**. See the `LICENSE.txt` file for more details.

# Made by a Brazilian.
