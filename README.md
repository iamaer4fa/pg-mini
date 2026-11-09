# ⚡ pg-mini: Ultra-Lightweight PostgreSQL Manager

[![Language](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)
[![GUI](https://img.shields.io/badge/GUI-egui%20%2F%20eframe-blue.svg)](https://github.com/emilk/egui)
[![Database](https://img.shields.io/badge/Database-PostgreSQL-336791.svg)](https://www.postgresql.org/)
[![Platform](https://img.shields.io/badge/Platform-Linux%20(Wayland%20%26%20X11)-black.svg)](#)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](#license)

**pg-mini** is an ultra-lightweight, high-performance native desktop GUI client for PostgreSQL built in **pure Rust**. 

**This is pre-alpha software, do not use this in your production servers.**

If you find **pgAdmin 4**, **DBeaver**, or **Beekeeper Studio** too heavy, bloated, or slow to start due to embedded web engines (Chromium/Electron) or Java virtual machines, **pg-mini** was created for you.

---

## 🚀 Key Highlights

- **⚡ Instant Startup**: Launches in under 100 milliseconds.
- **🪶 Tiny Memory Footprint**: Uses only **~25–35 MB of RAM** under normal usage (compared to 400 MB – 1 GB+ in pgAdmin and Electron tools).
- **🚫 Zero Web Engine / Electron / JVM**: Compiles to an **18 MB native binary** with zero Chromium, Node.js, or Java runtime dependencies.
- **🎯 Asynchronous Architecture**: Dedicated background thread with Tokio runtime keeps the user interface buttery-smooth (60+ FPS) during heavy database queries.
- **🖥️ Native Linux Display**: Full support for both **Wayland** and **X11** display servers.

---

## 📊 Comparison

| Feature / Metric | **pg-mini** | **pgAdmin 4** | **Beekeeper Studio** | **DBeaver** |
| :--- | :--- | :--- | :--- | :--- |
| **Technology** | **Native Rust (`egui`)** | Python + NW.js/Chromium | Electron (Chromium + Node) | Java Eclipse RCP |
| **Startup Time** | **< 0.1s** | 4 – 10s | 2 – 5s | 5 – 12s |
| **RAM Usage** | **~30 MB** | 400 MB – 1 GB+ | 300 – 600 MB | 500 MB – 1.5 GB |
| **Binary Size** | **~18 MB** | ~250 MB | ~150 MB | ~300 MB |
| **UI Responsiveness** | Instant | Laggy / Web DOM | Moderate | Heavy |

---

## 🛠️ Features

### 1. Database & Connection Management
- **Sidebar Databases Tree**: Lists all active databases on your PostgreSQL server (e.g. `steniel_cmms`, `postgres`, `nocobase`) with real-time size indicators.
- **1-Click Database Switching**: Click any database on the sidebar to instantly switch your active connection.
- **Database Context Menu (Right-Click)**:
  - `⚡ Open Query Tool`: Opens SQL Editor targeting this database.
  - `➕ Create Table...`: Launches a quick table creation modal.
  - `🔄 Refresh Tables`: Reloads schema objects.
  - `Backup...` / `Restore...`: Save custom-format database backups and restore PostgreSQL archives or plain SQL scripts.
  - `🗑️ Drop Database...`: Drops the database with confirmation (protected against dropping active DB).
- **Multiple Live Servers**: Connect to several PostgreSQL servers at once, switch the active server from the top bar, and keep each server connected while working with another.
- **Profile Management**: Save and switch between multiple PostgreSQL connection profiles (Host, Port, Database, Username, Password).
- **Persistent Storage**: Saved connections and settings are automatically stored in `~/.config/pg-mini/config.json`.
- **Show/Hide Password**: Quick toggle to verify database passwords.
- **Connection Status Bar**: Displays current active database, host, port, and PostgreSQL server version.

### 2. Schema & Table Explorer
- **Hierarchical Object Browser**: Servers → Databases → Schemas, with Tables, Views, Materialized Views, Sequences, Functions, Procedures, Types, and Extensions.
- **Lazy Catalog Loading**: Schemas and object categories load from PostgreSQL catalogs only when their tree nodes are expanded. Refresh and retry are available per node.
- **PostgreSQL Temporary Schemas**: Names like `pg_temp_3` are server-created namespaces for temporary tables used by database sessions; they are transient, not regular application schemas.
- **Live Search Filter**: Quickly find tables by typing in the search box.
- **Quick Action Badges**: Distinguishes between standard base tables (`📄`) and views (`👁`).
- **Object Context Menus (Right-Click)**: Create SQL templates, inspect properties, rename, delete with confirmation, refresh, and generate object SQL. Table, view, and materialized-view nodes also open data and schema views.
- **Table Actions**:
  - `📊 View Data (Top 100)`: Loads table rows into the virtualized data grid.
  - `➕ Add Column...`: Opens modal to add new columns with custom types, default values, and constraints.
  - `📋 View Table Schema`: Opens column definitions viewer.
  - `⚡ Query in SQL Editor`: Generates a query template in the SQL editor.
  - `🧹 Truncate Table...`: Safely empties table records with confirmation.
  - `🗑️ Drop Table...`: Permanently removes table with confirmation.

### 3. Add Column & Table Creation Modals
- **Add Column Modal**:
  - Target table selection (`schema.table`).
  - Column Name input.
  - Data Type selector (`VARCHAR`, `TEXT`, `INTEGER`, `BIGINT`, `BOOLEAN`, `TIMESTAMP`, `TIMESTAMPTZ`, `NUMERIC`, `UUID`, `JSONB`, `BYTEA`, or custom).
  - Nullable toggle (`NULL` / `NOT NULL`).
  - Default Value input (e.g. `'active'`, `0`, `now()`).
  - Primary Key checkbox.
  - **Live SQL Preview**: Inspect the exact `ALTER TABLE ... ADD COLUMN ...` statement in real time.
- **Create Table Modal**:
  - Schema and Table Name inputs.
  - Automatic `id SERIAL PRIMARY KEY` generation toggle.
  - Live `CREATE TABLE` SQL preview.

### 4. Virtualized Data Grid (Table Viewer)
- **High Performance**: Uses `egui_extras::TableBuilder` with virtualized row rendering to smoothly browse through thousands of rows.
- **Smart Formatting**: 
  - Numbers are right-aligned with distinct accent colors.
  - SQL `NULL` values are styled in subtle dimmed italics.
- **Interactive Cells**:
  - **Click to Copy**: Click any cell to copy its content to the system clipboard.
  - **Hover Tooltips**: Inspect full cell contents without layout truncation.
- **In-Memory Row Filter**: Real-time substring filter on loaded rows.
- **Pagination**: Configurable page sizes (50, 100, 250, 500, 1,000 rows) with Previous/Next controls.
- **Table Row Editing**: Edit cells and save or revert a row, insert rows with per-column `NULL`/`DEFAULT` choices, and delete rows after confirmation. Updating and deleting are enabled for base tables with a primary key.
- **CSV Export**: One-click RFC 4180 compliant CSV file export with file-save dialog.

### 5. Multi-Line SQL Query Editor
- **SQL Syntax Highlighting**: Highlights SQL keywords, strings, numbers, and line comments in the editor.
- **Schema-Aware Completion**: Press `Ctrl + Space` to complete SQL keywords, loaded table names, qualified schema/table names, and columns from the selected table.
- **Keyboard Execution**: Run queries with `Ctrl + Enter` (or `F5`).
- **Performance Timer**: Displays query execution time in milliseconds and row counts (`⚡ 3.4 ms | 120 rows returned`).
- **Quick Snippets**: Dropdown with ready-to-use SQL templates (Server Info, Database Sizes, Table Counts, etc.).
- **pgAdmin-Style Admin Insights**: One-click query plans (`EXPLAIN`, without executing the query), active sessions, blocked sessions, database statistics, and database roles in the normal result grid.
- **Users & Roles Management**: Create, edit, filter, and drop PostgreSQL login roles (users) and non-login group roles; configure passwords, role attributes, group-role membership, connection limits, and validity timestamps. PostgreSQL still enforces the connected account's role-administration privileges.
- **Persistent Query History**: Keeps the latest 200 distinct queries in the app configuration, with search, load-into-editor, and copy actions.
- **Error Reporting**: Clear, highlighted PostgreSQL error banners when syntax or constraints fail.

### 6. Table Schema / Column Inspector
- View table metadata:
  - Column Name
  - Data Type
  - Primary Key indicator (`🔑 YES`)
  - Nullability (`YES` / `NO`)
  - Default Values
- Actions:
  - `➕ Add Column...`: Add new columns directly from the schema inspector.
  - `🗑️ Drop Column...`: Drop a column with confirmation dialog.
  - Click column name to copy to clipboard.

### 7. Theming
- Toggle between **Dark Mode** (default) and **Light Mode** at any time.

---

## ⌨️ Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl + Enter` / `F5` | Execute current SQL query in the editor |
| `Ctrl + Space` | Open SQL and schema completion suggestions |
| `Click on Cell` | Copy cell value to clipboard |
| `Hover on Cell` | View full value tooltip |

---

## 📥 Installation & Running

### Prerequisites
- Linux OS (Ubuntu, Debian, Fedora, Arch, etc.)
- Windows 10 or later
- macOS 11 or later (Apple silicon and Intel Macs)
- Wayland or X11 desktop session on Linux
- PostgreSQL client utilities (`pg_dump`, `pg_restore`, and `psql`) installed and available on `PATH` for database backup and restore
- Rust & Cargo (only if building from source)

### 1. Quick Launch
The project comes with a pre-configured launcher script:
```bash
cd /home/allanregistos/apps/postgresql-manager
./run.sh
```

### 2. Run Directly via Binary
You can run the release binary directly:
```bash
./target/release/pg-mini
```

### 3. Build from Source
To compile an optimized release binary from scratch:
```bash
cargo build --release
```
The compiled binary will be placed at `target/release/pg-mini`.

---

## 📦 Build and Install the Ubuntu Package

Build a Debian package (including the desktop launcher and lightning icon):

```bash
./packaging/build-deb.sh
sudo apt install ./target/debian/pg-mini_0.1.6_amd64.deb
```

The package installs the app to `/usr/bin/pg-mini`, registers it in the desktop application launcher, and installs the icon in the hicolor icon theme.

To create a desktop shortcut manually, set `Exec` to the built executable and `Icon` to the absolute path of `packaging/pg-mini.svg` in your local checkout.

---

## 🚀 Build Packages on GitHub

The GitHub Actions workflow builds the Ubuntu `.deb`, a portable Windows `.exe`, a Windows setup installer, and macOS disk images for Apple silicon and Intel Macs. To build them, push a version tag matching `Cargo.toml` (currently `v0.1.6`):

```bash
git tag v0.1.6
git push origin v0.1.6
```

Once all platform builds finish, GitHub publishes a Release with the Linux, Windows, and macOS downloads. To build without publishing a Release, run **Build release packages** from the repository's **Actions** tab; the outputs will be available as workflow artifacts.

On Windows, the app checks whether the current session can create an OpenGL 2.0 context. It uses OpenGL when available and falls back to WGPU when it is not, such as in some Remote Desktop sessions. The app does not require PostgreSQL or a separate database client to be installed. The setup installer additionally installs Microsoft's Visual C++ runtime if it is missing, and can create Start menu and optional desktop shortcuts. The portable executable is also available for users who already have that runtime.

To keep multiple PostgreSQL servers connected, choose **➕ Connect Server** and connect another saved or new profile. Use the server selector in the top bar to switch which connection the object browser and SQL editor use; switching does not disconnect the other servers. **Disconnect** disconnects only the currently selected server.

On macOS, download the matching `arm64` (Apple silicon) or `x64` (Intel) `.dmg` from the GitHub Release, open it, and drag **pg-mini** into **Applications**. The app is currently unsigned and not notarized, so macOS may require you to allow it in **System Settings → Privacy & Security** the first time it is opened.

Backup and restore call the PostgreSQL client utilities (`pg_dump`, `pg_restore`, or `psql`) installed on your computer. Install PostgreSQL command-line tools separately and make sure their `bin` directory is on `PATH`. Backups are custom-format `.dump` files; restore also accepts PostgreSQL custom/tar archives and plain `.sql` files. Restore does not use `pg_restore --clean`, but SQL files and archive contents can still change or drop database objects.

---

## 📁 Project Structure

```text
postgresql-manager/
├── Cargo.toml            # Project dependencies and manifest
├── Cargo.lock            # Locked crate versions
├── run.sh                # Executable launch script
├── packaging/             # Debian package metadata and desktop launcher
│   └── macos/             # macOS app bundle and disk image packaging
├── assets/                # Application icon
├── .github/workflows/     # Automated release builds
├── README.md             # Documentation
└── src/
    ├── main.rs           # Application entry point and window setup
    ├── app.rs            # Main eframe::App implementation and UI layout
    ├── models.rs         # Data models (connections, query results, cells)
    ├── config.rs         # Persistent configuration (~/.config/pg-mini/)
    ├── db.rs             # Background Tokio worker & async tokio-postgres client
    └── ui/
        ├── mod.rs
        ├── connection_modal.rs  # Connection dialog & profile manager
        ├── sidebar.rs           # Schema and table explorer
        ├── data_table.rs        # Virtualized table grid & CSV export
        ├── sql_editor.rs        # SQL query editor with shortcuts & history
        └── columns_view.rs      # Table schema/column metadata inspector
```

---

## 🔧 Technical Details

- **GUI Framework**: [`egui`](https://github.com/emilk/egui) / [`eframe`](https://docs.rs/eframe) (v0.29), using Glow/OpenGL on Linux and macOS, and OpenGL with WGPU fallback on Windows.
- **PostgreSQL Client**: [`tokio-postgres`](https://docs.rs/tokio-postgres) (pure asynchronous wire-protocol implementation in Rust, without external C `libpq` dependencies).
- **Concurrency**: Messages between the GUI and the database worker are exchanged asynchronously over standard channels, ensuring zero UI freezes during query execution.
- **Object Catalogs**: The browser queries PostgreSQL `pg_catalog`; each expanded object group is loaded asynchronously through the existing database worker.
- **Configuration**: JSON-serialized settings stored in standard XDG config directories via `dirs`.

---

## 📄 License

This project is licensed under the MIT License - feel free to use and modify it.
