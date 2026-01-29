# Lunu

**Lunu** is a robust toolchain and library manager for **Luau** powered by **Lute** (native) and **Lune** (bridge). It provides a complete set of tools to develop, manage dependencies, and compile Luau projects into standalone native executables.

Made in Rust 🦀

## Features

- **Dependency Management**: Easily install, update, and remove libraries (similar to `npm` or `cargo`).
- **Lute Runtime**: Native execution with `@lute` and `@std`, plus direct native module support (C/C++/Rust) without bridge overhead.
- **Build System**: Compile your Luau scripts (`.luau`) into native executables (`.exe`) that run on any Windows machine. **The builder is built-in**, so you don't need external tools.
- **Lune Runtime**: Bridge runtime with direct stdin/stdout worker execution for Python, Node.js, Rust, and more.
- **Single-File Distribution**: The entire toolchain (Manager, Builder, Bridge, and Lute runtime) is contained in a **single executable** (`lunu.exe`).

---

## Tech Stack

Lunu is built using a modern and high-performance stack:

*   **Core**: Rust (using `tokio` for async I/O, `clap` for CLI, `reqwest` for networking).
*   **Scripting Language**: Luau (type-safe, highly performant Lua derivative).
*   **Runtimes**:
    *   **Lune**: A standalone Luau runtime with file system and process access, used for tooling and bridge integrations.
    *   **Lute**: A high-performance native runtime that allows direct linking with C/C++ libraries.
*   **Serialization**: Serde (Rust) and `serde_json` / `@lune/serde` for robust data exchange.

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

### 3. Selecting a Runtime

Lunu supports multiple runtimes for different use cases. You can configure this in `lunu.toml` or override it via environment variables.

*   **Lune (Default)**: Best for scripting, tooling, and projects requiring Python/Node.js integration via Bridge.
*   **Lute (Native)**: Best for performance-critical applications, games, and native modules (C++/Rust).

**Configuration (`lunu.toml`):**
```toml
[runtime]
name = "lute"  # or "lune"
```

**CLI Override:**
```bash
# Run with Lute explicitly
LUNU_RUNTIME=lute lunu run src/main.luau
```

### 4. Running Scripts
Run your script using the configured runtime:

```bash
lunu run src/main.luau
```

### 5. Compiling to Executable (.exe)
Turn your main script into a standalone program:

```bash
lunu build main.luau
```
*   **Lune Projects**: Embeds the Lune runtime, dependencies, and script into a single `.exe`.
*   **Lute Projects**: Compiles using the native C++ toolchain, linking directly against `@lute` and native modules.

---

## Polyglot Development: Lute vs. Lune

Lunu empowers you to connect Luau with other languages, but the approach depends on your chosen runtime.

### Lune (The Bridge Approach)
**Best for**: Python, Node.js, or safe/sandboxed environments.

Lune uses a **Bridge System** where external workers (Python scripts, Node.js apps, etc.) run in separate processes and communicate via JSON-RPC over Stdin/Stdout.

*   **Pros**: Safe (sandboxed), language-agnostic (works with anything that speaks JSON), easy to distribute Python scripts.
*   **Cons**: Serialization overhead, async communication only.

**Example (Python):**
```lua
local lunu = require("@lunu")
local result = lunu.call("my-python-lib", "greet", "User")
print(`Result: {result}`)
```

### Lute (The Native Approach)
**Best for**: C++, Rust, High-Performance Systems.

Lute allows **direct native modules**. You can write C++ or Rust code that compiles into libraries linked directly to your application.

*   **Pros**: Zero overhead (direct function calls), full system access, maximum performance.
*   **Cons**: Requires a C++ compiler (MSVC/Clang/GCC) installed on the dev machine.

**Example (Native C++):**
In Lute, you don't need a bridge. You simply `require` the native module, and it works as if it were Luau code.

---

## CLI Reference

- `lunu init` - Initialize a project.
- `lunu create <name>` - Create a new project folder.
- `lunu add <user/repo>` - Add a dependency.
- `lunu remove <name>` - Remove a dependency.
- `lunu install` - Install dependencies from `lunu.toml`.
- `lunu build <entry.luau>` - Compile to executable.
- `lunu run <entry.luau> [args...]` - Run a script.
- `lunu check` - Validate environment and types.
- `lunu dev` - Start HTTP bridge server (foreground).
- `lunu scaffold <name> --template <app|game>` - Scaffold a project.
- `lunu module <name> --lang <python|node|rust>` - Create a module scaffold.
- `lunu runtime <lute|lune> [--update]` - Manage runtimes.
- `lunu runtimes [--update]` - Manage all runtimes.
- `lunu upgrade` - Upgrade the CLI.
- `lunu uninstall` - Uninstall the CLI.

---

## License

This project is licensed under the **Mozilla Public License 2.0**. See the `LICENSE.txt` file for more details.

# Made by a Brazilian.
