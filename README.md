# 🐝⚡ Hornet — Terminal Database Client

<p align="center">
  <strong>Fast, keyboard-driven, modern Terminal Database Client built with Rust & Ratatui.</strong>
  <br />
  <em>Complete Vim modal editing, live multi-database switching, smart autocomplete, and interactive results.</em>
</p>

---

## ✨ Features

- **⚡ Blazing Fast TUI**: Built from scratch in **Rust** using **Ratatui** and Tokio asynchronous runtime for instant rendering and smooth navigation.
- **⌨️ Native Vim Modal Editor**:
  - Full modal editing: `NORMAL`, `INSERT`, `VISUAL`, and `VISUAL LINE` modes.
  - Classic Vim motions: `h`, `j`, `k`, `l`, `w`, `b`, `e`, `0`, `$`, `^`, `G`, `gg`, `Ctrl+u`, `Ctrl+d`.
  - Vim operators: `dd`, `dw`, `d$`, `cc`, `cw`, `yy`, `yw`, `x`, `p`, `P`, and `u` (undo stack).
  - Visual mode line selection and block yanking/deletion.
- **🔌 Native Async Database Drivers**:
  - **PostgreSQL** (with native `rustls` TLS encryption)
  - **MySQL / MariaDB**
  - **SQLite** (local embedded files)
  - *(Extensible async pool architecture via `sqlx`)*
- **🗄️ Multi-Database Introspection & Live Switching**:
  - Automatically lists **all databases** available on a server connection.
  - Active database is highlighted with schemas, tables, and column hierarchies.
  - Switch active database on the fly simply by navigating to it in the Explorer tree and pressing <kbd>Enter</kbd>.
- **➕ Interactive Connection Manager**:
  - Add new connections directly inside the TUI with the `+ [New Connection]` node or pressing <kbd>a</kbd> / <kbd>N</kbd>.
  - Built-in driver selector with auto-filled connection URI templates.
  - Pre-flight connection test before saving.
- **📊 Interactive Results Grid**:
  - Automatic column width optimization based on content headers and sample rows.
  - Column scrolling (`h`/`l`) and fast jumping (`H`/`L` or `[`/`]`).
  - Execution metrics: duration in milliseconds, total row count, active column name and cell content.
- **📋 Versatile Export / Copy Modal**:
  - Press <kbd>c</kbd> or <kbd>Enter</kbd> in Results to open the Copy Menu:
    1. **Copy Selected Cell**
    2. **Copy Row as JSON**
    3. **Copy Row as CSV**
    4. **Copy Row as SQL INSERT**
    5. **Copy All Results as CSV**
    6. **Copy All Results as JSON**
    7. **Copy All Results as Markdown Table**
- **💡 Smart Auto-Completion & LSP**:
  - Integrated SQL Language Server (`sqls`) protocol support.
  - Local database schema autocompletion: introspected tables and columns.
  - Standard SQL keyword autocompletion triggered seamlessly or via <kbd>Ctrl+Space</kbd>.
- **📝 Saved SQL Notes & Query History**:
  - Save SQL files on the fly with <kbd>Ctrl+S</kbd>.
  - Browse saved queries and past executions with execution metrics in the sidebar.

---

## 🏗️ Architecture

Hornet is built as a single, high-performance, **100% pure Rust** standalone binary:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Hornet Standalone Binary                        │
│                 (Rust + Ratatui + Crossterm + Tokio)                   │
├────────────────────────────────────────────────────────────────────────┤
│  • Vim Modal Engine               • Dynamic Results Grid & Exporter    │
│  • Local & LSP Autocompletion     • Interactive Sidebar & Note Manager │
│  • ANSI Syntax Highlighting       • Multi-DB Hierarchy Navigation      │
├────────────────────────────────────────────────────────────────────────┤
│                Async Native Database Drivers (sqlx)                    │
│   • PostgreSQL (rustls)    • MySQL / MariaDB    • SQLite (embedded)    │
└────────────────────────────────────────────────────────────────────────┘
```

- **Single Binary (`hornet`)**: Zero external daemon processes, zero runtime C dependencies, and no background IPC overhead. Everything runs seamlessly in a unified Tokio asynchronous runtime.
- **Async Driver Pool (`sqlx`)**: Native asynchronous connection pooling with Rustls TLS encryption.


---

## ⌨️ Keybindings Reference

### Global Shortcuts

| Keybinding | Action |
| :--- | :--- |
| <kbd>Tab</kbd> / <kbd>Shift+Tab</kbd> | Cycle focus between panes (`Explorer` ➔ `Editor` ➔ `Results`) |
| <kbd>1</kbd> / <kbd>2</kbd> / <kbd>3</kbd> | Jump directly to pane (1: Explorer, 2: Editor, 3: Results) |
| <kbd>F5</kbd> or <kbd>Ctrl+r</kbd> | Execute entire SQL buffer |
| <kbd>Ctrl+Enter</kbd> / <kbd>Ctrl+e</kbd> | Execute statement under cursor |
| <kbd>Ctrl+l</kbd> | Execute current line |
| <kbd>Ctrl+s</kbd> | Save current editor content as a Saved SQL Note |
| <kbd>Ctrl+c</kbd> | Quit Hornet |

### SQL Editor (Vim Modes)

| Mode | Keybinding | Action |
| :--- | :--- | :--- |
| **Normal** | <kbd>i</kbd> / <kbd>I</kbd> | Enter Insert mode (at cursor / line start) |
| **Normal** | <kbd>a</kbd> / <kbd>A</kbd> | Enter Insert mode (after cursor / line end) |
| **Normal** | <kbd>o</kbd> / <kbd>O</kbd> | Open new line below / above and enter Insert mode |
| **Normal** | <kbd>v</kbd> / <kbd>V</kbd> | Enter Visual character / line selection mode |
| **Normal** | <kbd>h</kbd> <kbd>j</kbd> <kbd>k</kbd> <kbd>l</kbd> | Standard cursor movement |
| **Normal** | <kbd>w</kbd> / <kbd>b</kbd> / <kbd>e</kbd> | Move by word forward / backward / word end |
| **Normal** | <kbd>0</kbd> / <kbd>$</kbd> | Move to start / end of line |
| **Normal** | <kbd>gg</kbd> / <kbd>G</kbd> | Move to top / bottom of buffer |
| **Normal** | <kbd>dd</kbd> / <kbd>dw</kbd> / <kbd>d$</kbd> | Delete line / word / to line end |
| **Normal** | <kbd>cc</kbd> / <kbd>cw</kbd> | Change line / word |
| **Normal** | <kbd>yy</kbd> / <kbd>yw</kbd> | Yank (copy) line / word |
| **Normal** | <kbd>p</kbd> / <kbd>P</kbd> | Paste after / before cursor |
| **Normal** | <kbd>u</kbd> | Undo last change |
| **Normal** | <kbd>Enter</kbd> | Execute current statement |
| **Insert** | <kbd>Esc</kbd> | Return to Normal mode |
| **Insert** | <kbd>Ctrl+Space</kbd> | Trigger autocompletion popup |
| **Visual** | <kbd>y</kbd> / <kbd>d</kbd> / <kbd>c</kbd> | Yank / Cut / Change visual selection |

### Explorer (Sidebar)

| Keybinding | Action |
| :--- | :--- |
| <kbd>j</kbd> / <kbd>k</kbd> or <kbd>▲</kbd> / <kbd>▼</kbd> | Navigate tree items |
| <kbd>Enter</kbd> | Expand node / switch database / preview table query |
| <kbd>Space</kbd> / <kbd>o</kbd> / <kbd>l</kbd> | Toggle node expand / collapse |
| <kbd>a</kbd> / <kbd>N</kbd> | Open New Connection modal |
| <kbd>r</kbd> | Refresh connections and saved notes |

### Results Grid

| Keybinding | Action |
| :--- | :--- |
| <kbd>j</kbd> / <kbd>k</kbd> or <kbd>▲</kbd> / <kbd>▼</kbd> | Navigate rows |
| <kbd>h</kbd> / <kbd>l</kbd> or <kbd>◄</kbd> / <kbd>►</kbd> | Navigate columns |
| <kbd>H</kbd> / <kbd>L</kbd> or <kbd>[</kbd> / <kbd>]</kbd> | Jump 4 columns left / right |
| <kbd>PageUp</kbd> / <kbd>PageDown</kbd> | Jump 8 rows up / down |
| <kbd>y</kbd> | Copy selected cell value |
| <kbd>Y</kbd> | Copy selected row as JSON |
| <kbd>c</kbd> or <kbd>Enter</kbd> | Open Clipboard Export Modal |

---

## 🚀 Installation & Building

### Prerequisites

- **Rust** (1.75+ recommended): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- *(Optional)* **sqls** (for SQL language server completions): `go install github.com/sqls-server/sqls@latest`

### Build from Source

Clone the repository and compile with Cargo:

```bash
git clone https://github.com/thiagopinheiro/hornet.git
cd hornet

# Build release binary
make build

# Run Hornet
./bin/hornet
```

Or run directly with cargo:

```bash
cargo run --release
```

### Install to System

To install `hornet` directly to your user binary path (`~/.local/bin`):

```bash
make install
```

Make sure `~/.local/bin` is in your `$PATH`.


---

## ⚙️ Configuration & Storage

Hornet stores its configuration in standard XDG directories:

- **Connections**: `~/.config/hornet/connections.json`
- **Saved SQL Notes**: `~/.local/state/hornet/notes/global/`

### Backwards Compatibility

Hornet automatically detects and imports connections from:
- `~/.local/state/nvim/dbee/persistence.json` (nvim-dbee)
- `~/.config/sqls/config.yml` (sqls)

---

## 🔒 Security & Privacy

Hornet is 100% local and open source.
- Connections and credentials are stored strictly on your local machine in `~/.config/hornet/connections.json`.
- No telemetry, analytics, or remote calls are ever performed.

---

## 📄 License

This project is licensed under the **MIT License**. See the [LICENSE](LICENSE) file for details.
