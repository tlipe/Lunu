# Lunu

**Lunu** is a robust toolchain and library manager for **Luau** (powered by the **Lune** runtime). It provides a complete set of tools to develop, manage dependencies, and compile Luau projects into standalone native executables.

## Features

- **Dependency Management**: Easily install, update, and remove libraries (similar to `npm` or `cargo`).
- **Build System**: Compile your Luau scripts (`.luau`) into native executables (`.exe`) that run on any Windows machine, without requiring a separate Lune installation.
- **Bridge Server**: A high-performance HTTP communication bridge (written in Rust) that allows your Luau scripts to interact with the operating system and other languages.
- **Single-File Distribution**: The installer and toolchain are distributed as a single executable file.

---

## Installation

1. Download the **`lunu.exe`** file from the Releases section.
2. Run `lunu.exe` in your terminal or double-click it.
   - On the first run, it acts as an **installer**, extracting the necessary tools to `~/.lunu/bin` and configuring your system PATH.
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
# Add a library (e.g., numpy-luau)
lunu add numpy-luau

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

### 4. Development (Bridge Server)
For scripts that use advanced Lunu Bridge functionalities:

```bash
# Start the bridge server in development mode (foreground)
lunu dev

# Start the server in background (daemon)
lunu bridge --daemon
```

---

## Repository Structure

Understand how this repository is organized:

### `toolchain/`
The "brain" of Lunu. Contains the **Rust** source code for:
- **CLI (`lunu-cli`)**: The main command-line tool.
- **Installer (`installer`)**: The code that generates the self-installing `lunu.exe`.
- **Bridge (`bridge`)**: The HTTP server that connects Luau to the system.

### `builder/`
The compilation system. Contains the **Rust** source code for:
- **Builder (`lunu-build`)**: The tool that packages Luau scripts.
- **Stub (`lunu-stub`)**: The small "shell" executable that is attached to your script to form the final `.exe`.

### `config/`
Default Lunu configuration files, such as `settings.json`, used to configure server ports and API keys.

### `examples/`
Contains project examples and libraries to serve as a base (`template-lib`).

### `scripts/`
PowerShell scripts for automating Lunu's own development:
- `install.ps1`: Legacy installation script.
- `setup.ps1`: Initial development environment setup.

### `toolchain/scripts/build.ps1`
The master build script. It compiles the entire project in the correct order:
1. Compiles the **Builder** and **Stub**.
2. Compiles the **CLI** and **Bridge**.
3. Compiles the **Installer**, embedding all the above binaries inside it.

### `init.luau`
The main Lunu Lua module. This file is loaded when you `require("@lunu")` in your projects. It manages communication with the Bridge Server.

### `rokit.toml`
**Rokit** configuration file, used to manage the Luau/Lune toolchain version used in this project's development.

---

## Development and Contribution

To modify Lunu:

1. Install **Rust** and **Cargo**.
2. Clone the repository.
3. Use the build script to generate everything:
   ```powershell
   cd toolchain/scripts
   .\build.ps1
   ```
4. The final binary will be at `bin/lunu.exe`.

## License

This project is licensed under the **Mozilla Public License 2.0**. See the `LICENSE.txt` file for more details.

-  Made by a Brazilian.
