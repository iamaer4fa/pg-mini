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
- **Sidebar Databases Tree**: Lists all active databases on your PostgreSQL server  with real-time size indicators.
- **1-Click Database Switching**: Click any database on the sidebar to instantly switch your active connection.
- **Database Context Menu (Right-Click)**:
  - `⚡ Open Query Tool`: Opens SQL Editor targeting this database.
  - `➕ Create Table...`: Launches a quick table creation modal.
  - `🔄 Refresh Tables`: Reloads schema objects.
  - `Backup...` / `Restore...`: Save custom-format database backups and restore PostgreSQL archives or plain SQL scripts.
  - `🗑️ Drop Database...`: Drops the database with confirmation (protected against dropping active DB).
- **Multiple Live Servers**: Connect to several PostgreSQL servers at once, switch the active server from the top bar, and keep each server connected while working with another.
- **Profile Management**: Save and switch between multiple PostgreSQL connection profiles (Host, Port, Database, Username, Password).
- **TLS Connections**: Disable TLS, prefer TLS with an explicit plaintext fallback, or require verified TLS. Verification uses the operating system trust store and optionally an additional PEM CA certificate.
- **Portable Profiles**: Import and export connection profiles as versioned JSON. Passwords are excluded by default; enabling password export stores them as plaintext in the JSON file.
- **Persistent Storage**: Saved connections and settings are automatically stored in `~/.config/pg-mini/config.json`.
- **Show/Hide Password**: Quick toggle to verify database passwords.
- **Connection Status Bar**: Displays current active database, host, port, and PostgreSQL server version.

### 2. Schema & Table Explorer
- **Hierarchical Object Browser**: Servers → Databases → Schemas, with Tables, Views, Materialized Views, Sequences, Functions, Procedures, Types, and Extensions.
- **Lazy Catalog Loading**: Schemas and object categories load from PostgreSQL catalogs only when their tree nodes are expanded. Refresh and retry are available per node.
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
- **Schema DDL Forms**: Create indexes, foreign keys, CHECK/UNIQUE/PRIMARY KEY constraints, views, and sequences from object-browser context menus; inspect generated SQL previews before execution.
- **Table Grants**: Grant or revoke table privileges for existing roles or `PUBLIC`, including grant option and revoke cascade controls, with a generated SQL preview.
- **Activity Dashboard**: Monitor visible sessions, queries running at least five seconds, blocked sessions and lock waits, and the largest databases and tables. Refresh manually or automatically every 15 seconds while the dashboard is open.
- **System Stats**: View host-level OS, CPU, memory, disk, load, process, and network metrics from the optional PostgreSQL `system_stats` extension in a dedicated tab. The extension must be installed on the PostgreSQL server and enabled in the selected database; pg-mini does not install server-side extensions. Access is controlled by the extension's `monitor_system_stats` role, and values may be `NULL` or unavailable depending on platform and permissions.

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
cd ~/pg-mini
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

## 📦 Build and Install Linux Packages

### Ubuntu / Debian

Build a Debian package (including the desktop launcher and lightning icon):

```bash
./packaging/build-deb.sh
sudo apt install ./target/debian/pg-mini_0.1.13_amd64.deb
```

The package installs the app to `/usr/bin/pg-mini`, registers it in the desktop application launcher, and installs the icon in the hicolor icon theme.

To create a desktop shortcut manually, set `Exec` to the built executable and `Icon` to the absolute path of `packaging/pg-mini.svg` in your local checkout.

### RHEL-compatible distributions

RPM packages are built on Rocky Linux 9 for x86_64 and target RHEL 9-compatible distributions, including Rocky Linux 9, AlmaLinux 9, RHEL 9, and CentOS Stream 9. CentOS Linux 7/8 and other older releases are not supported. The package requires the system GUI libraries listed in its RPM metadata.

On a supported distribution, install the downloaded package with:

```bash
sudo dnf install ./pg-mini-0.1.13-1.el9.x86_64.rpm
```

To build the RPM locally on a Rocky Linux 9-compatible build environment, install Rust, the Linux GUI development libraries, and `rpm-build`, then run:

```bash
./packaging/build-rpm.sh
```

The generated RPM is placed in `target/rpm/`.

---

## 🚀 Build Packages on GitHub

The GitHub Actions workflow builds the Ubuntu `.deb`, a RHEL-compatible `.rpm`, a portable Windows `.exe`, a Windows setup installer, and macOS disk images for Apple silicon and Intel Macs. To build them, push a version tag matching `Cargo.toml` (currently `v0.1.13`):

```bash
git tag v0.1.13
git push origin v0.1.13
```

Once all platform builds finish, GitHub publishes a Release with the Linux, Windows, and macOS downloads. To build without publishing a Release, run **Build release packages** from the repository's **Actions** tab; the outputs will be available as workflow artifacts.

On Windows, the app checks whether the current session can create an OpenGL 2.0 context. It uses OpenGL when available and falls back to WGPU when it is not, such as in some Remote Desktop sessions. The app does not require PostgreSQL or a separate database client to be installed. The setup installer additionally installs Microsoft's Visual C++ runtime if it is missing, and can create Start menu and optional desktop shortcuts. The portable executable is also available for users who already have that runtime.

To keep multiple PostgreSQL servers connected, choose **➕ Connect Server** and connect another saved or new profile. Use the server selector in the top bar to switch which connection the object browser and SQL editor use; switching does not disconnect the other servers. **Disconnect** disconnects only the currently selected server.

On macOS, download the matching `arm64` (Apple silicon) or `x64` (Intel) `.dmg` from the GitHub Release, open it, and drag **pg-mini** into **Applications**. The app is currently unsigned and not notarized, so macOS may require you to allow it in **System Settings → Privacy & Security** the first time it is opened.

Backup and restore call the PostgreSQL client utilities (`pg_dump`, `pg_restore`, or `psql`) installed on your computer. Install PostgreSQL command-line tools separately and make sure their `bin` directory is on `PATH`. Backups are custom-format `.dump` files; restore also accepts PostgreSQL custom/tar archives and plain `.sql` files. Restore does not use `pg_restore --clean`, but SQL files and archive contents can still change or drop database objects.

TLS mode **Prefer** attempts verified TLS first and falls back to plaintext if TLS negotiation fails; it does not retry in plaintext after the server returns a PostgreSQL error. Choose **Require** to prevent plaintext fallback. Both TLS modes validate the server certificate and hostname using the operating system trust store, augmented by the optional PEM CA certificate. Imported certificate paths are local paths and may need to be updated on the destination computer. Profile JSON exports omit passwords by default; password-inclusive exports contain credentials in plaintext and should be protected accordingly.

The Activity dashboard is refreshed from PostgreSQL's statistics views and shows data visible to the connected database role; PostgreSQL may hide query text or other session details from non-privileged accounts.

---

## System Stats Extension

The **System Stats** tab reads the [EnterpriseDB `system_stats` extension](https://github.com/EnterpriseDB/system_stats), which exposes operating-system metrics through PostgreSQL functions. It reports OS and CPU details, CPU usage, memory and swap, disk and I/O, load average, process counts and per-process CPU/memory, and network interfaces. The extension supports Linux, macOS, and Windows; some fields are platform-specific and are returned as `NULL` when unavailable.

The extension is separate from pg-mini and must be installed on the PostgreSQL server using the extension build or package appropriate for that server. Then enable it in each database where host metrics should be monitored:

```sql
CREATE EXTENSION system_stats;
```

By default, the extension restricts function execution to superusers and members of `monitor_system_stats`. For least-privilege monitoring, grant that role to the account used by pg-mini after reviewing the extension's security guidance:

```sql
GRANT monitor_system_stats TO monitoring_user;
```

pg-mini only reads the extension's functions. It does not install, upgrade, or remove PostgreSQL extensions. The System Stats tab detects whether the extension is enabled in the current database, reports per-function permission errors, and preserves `NULL` values rather than presenting them as zero.

---

## High Availability (HA)

pg-mini's HA work is intended to give administrators a clear view of PostgreSQL streaming-replication clusters and, in later phases, a guided way to provision a primary and standby. Provisioning is not implemented yet; pg-mini currently monitors existing servers and does not change PostgreSQL server configuration or perform failover.

### HA monitoring (available)

- Group saved connection profiles into named clusters and view each node's role, reachability, last successful update, and reported replication state.
- Read node role from `pg_is_in_recovery()`. Primary-side status is read from `pg_stat_replication`; standby-side status is read from `pg_stat_wal_receiver`. System identifiers are compared where available, and a standby's reported sender endpoint is matched against configured primary endpoints. Hostnames alone do not establish cluster membership.
- Configure polling and stale-data intervals, optional reported-lag thresholds, and failed-check alerts. Failed checks retain the last successful snapshot and mark it stale after the configured threshold. Recent changes and alerts are kept only in memory while pg-mini is running; this desktop application is not a server-side monitoring or alerting service.
- Treat missing fields and permission-restricted measurements as unknown. A missing WAL receiver row means none was reported at sampling time, not that replication is healthy. Lag values are PostgreSQL-reported measurements, may be `NULL` or unavailable, and are not estimates computed by pg-mini.
- Monitoring queries are implemented for PostgreSQL 9.6 and later, including pre-10 WAL field/function names. This compatibility is not a live-test or certification claim for every release. PostgreSQL 9.6 is end-of-life; the upstream project currently lists PostgreSQL 14–18 as supported. PostgreSQL 10+ offers predefined monitoring roles including `pg_read_all_stats` and `pg_monitor`; grant only the access appropriate to your environment. See the official [monitoring statistics](https://www.postgresql.org/docs/18/monitoring-stats.html), [predefined roles](https://www.postgresql.org/docs/18/predefined-roles.html), and [versioning policy](https://www.postgresql.org/support/versioning/). The optional system identifier requires PostgreSQL 10+ and appropriate permission to read `pg_control_system()`.
- pg-mini does not currently integrate with Patroni or repmgr.

### HA provisioning roadmap (planned)

Initial provisioning scope should be physical streaming replication for one primary and one standby, targeting PostgreSQL 17 and 18. Those two supported major versions keep the first implementation focused on currently maintained releases; older-server monitoring compatibility does not imply provisioning support. Revisit the target as PostgreSQL releases reach end of life.

1. **Preflight and plan** — Confirm both server versions, roles, connectivity, TLS, replication privileges, required settings, target data-directory state, and network reachability. Show a reviewed plan and explicit impact/backup warnings before any change.
2. **Prepare the primary** — Create or validate a least-privilege replication role, check WAL sender/slot capacity and `wal_level`, and prepare an appropriately restricted `pg_hba.conf` rule. Detect whether a reload or restart is required; do not silently make network/firewall changes.
3. **Bootstrap the standby** — Validate the target is safe to initialize, take a base backup, configure the standby connection and recovery signal, and start or reload the service as appropriate. Protect credentials, avoid logging secrets, and make destructive data-directory actions opt-in with clear confirmation.
4. **Verify and monitor** — Confirm the standby connects to the intended primary using PostgreSQL metadata, verify receiver/sender state and progress, and show incomplete or permission-restricted checks as unknown. Provide actionable remediation without declaring success until checks pass.
5. **Operate and recover** — Support safe re-check/retry, adding or removing a standby, and documented cleanup/rollback. Keep automatic failover, multi-standby orchestration, Patroni/repmgr management, and cross-version upgrades out of the initial provisioning scope.

**Execution model to settle before provisioning implementation:** PostgreSQL configuration files, data directories, and service restarts are operating-system-level operations; SQL access alone cannot safely perform all of them on arbitrary hosts. A safer first delivery is a guided workflow that generates and validates commands/configuration for administrator review, then records verification results. Fully automated remote provisioning would need an explicitly scoped and secured host-execution mechanism (for example, an opt-in SSH helper), separate from saved database credentials.

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
    ├── ha/               # HA node roles and status models
    ├── system_stats.rs   # system_stats extension query results
    ├── models.rs         # Data models (connections, query results, cells)
    ├── config.rs         # Persistent configuration (~/.config/pg-mini/)
    ├── db.rs             # Background Tokio worker & async tokio-postgres client
    └── ui/
        ├── mod.rs
        ├── ha_overview.rs      # HA node role overview
        ├── system_stats.rs     # Optional host system statistics view
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
