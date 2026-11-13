# pg-mini Roadmap

This roadmap prioritizes practical PostgreSQL operations for DBAs and developers who administer databases. It favors safe, explainable workflows over feature breadth and preserves pg-mini's lightweight desktop design.

## Product direction

Make common PostgreSQL administration tasks safer and faster: diagnose a problem, understand its impact, perform a controlled action, and verify the result.

## Existing foundation

The project already has a SQL editor and query history, schema and data browsing/editing, CSV export, role management, database backup/restore, an activity dashboard with blocked sessions, database/table size insights, and HA/physical replication status and setup. The roadmap builds on these rather than treating them as missing features.

## Prioritized roadmap

### P0 — Safe, controllable SQL operations

**Why first:** DBAs routinely run expensive or state-changing SQL. The editor needs strong operational controls before adding more ways to issue commands.

- Add a **Cancel query** action for in-flight SQL, with clear running/cancelled/completed state.
- Support configurable per-connection `statement_timeout` and `lock_timeout`, with visible effective values and a safe default policy.
- Add an explicit read-only query mode and a clear transaction workflow (begin, commit, rollback); make transaction state visible and warn before closing with an open transaction.
- Bound large result sets and provide an explicit way to fetch more rows, without blocking the UI.
- Keep destructive and high-impact operations behind a review step that shows the target and generated SQL.

**Done when:** a user can cancel a long query, see whether a transaction is open, avoid an unbounded accidental result, and understand the target and impact before confirming a destructive operation.

### P1 — Backup and restore confidence

**Why next:** A backup is only useful if it can be restored. The app already supports `pg_dump`, `pg_restore`, and `psql`; improve operational feedback around that workflow before adding a scheduler.

- Show the command/tool being used, progress where available, completion status, elapsed time, and actionable error output.
- Add preflight checks for required client utilities, destination access, server/database compatibility, and available disk space when it can be determined.
- Offer restore options such as clean-before-restore and object selection where supported by the archive format; show the effect before execution.
- Make overwrite/replace behavior explicit and require confirmation for destructive restore options.
- Document and test a manual backup-and-restore verification procedure.

**Done when:** users can tell whether a backup or restore succeeded, diagnose failures, and review potentially destructive restore behavior before it runs.

### P2 — Performance diagnosis from real server statistics

**Why:** Finding slow or resource-heavy queries is a frequent DBA task. The current query-plan shortcut uses plain `EXPLAIN`; richer diagnostics should be opt-in and explain their costs.

- Add an optional `pg_stat_statements` view when the extension is installed and the connected role has permission; report clearly when unavailable.
- Show top statements by total time, mean time, calls, rows, and block I/O, with time-window/reset caveats.
- Extend query-plan viewing with readable plan structure and key estimates (cost, rows, loops, buffers when present).
- Keep `EXPLAIN ANALYZE` separate from plain `EXPLAIN`, warn that it executes the statement, and require explicit confirmation. Never imply that rollback prevents every possible side effect.
- Link session/activity entries to the relevant query text and plan where possible.

**Done when:** a DBA can identify expensive statements, inspect a plan, and distinguish non-executing estimates from plans that execute the query.

### P3 — Routine table and index maintenance

**Why:** Vacuum health, table growth, and index effectiveness are recurring operational checks and fit naturally beside the existing size and activity views.

- Add per-table estimates for live/dead tuples, last vacuum/autovacuum/analyze/autoanalyze timestamps, and index/table scan statistics when available.
- Surface stale or potentially concerning values as indicators, not definitive diagnoses; account for statistics reset, permissions, and approximate counts.
- Provide carefully confirmed `VACUUM (ANALYZE)` and `ANALYZE` actions for selected tables, with command scope and transaction restrictions explained.
- Show index size and usage statistics. Label low-use indexes as candidates for investigation, not automatic drops.
- Defer automated index-drop or “bloat percentage” recommendations unless the app can support and explain their assumptions reliably.

**Done when:** users can inspect maintenance signals and run a scoped, understood maintenance command without the UI suggesting unsafe automatic cleanup.

### P4 — Replication and WAL risk visibility

**Why:** pg-mini already reports primary/standby status and has guided physical replication setup; the next useful step is operational monitoring rather than more setup automation.

- Show replication-slot state, retained WAL where available, and inactive or lagging slots that may create disk pressure.
- Present send/write/flush/replay lag and receiver status together with collection time and permission/unknown-state handling.
- Add configurable visual warning thresholds; do not claim automatic failover or guarantee recovery.
- Provide a concise primary/standby health summary and explain which checks require elevated PostgreSQL privileges.

**Done when:** users can spot missing receivers, stale slots, and lag risks, and can distinguish a healthy result from unavailable monitoring data.

## Later / not in the initial roadmap

- **Scheduled backups:** valuable, but scheduling, credentials, machine availability, retention, and alert delivery require a dependable background-service design. Prefer external schedulers until that design exists.
- **ER diagrams:** useful for understanding schemas, but less urgent than safe query control, recovery, performance, and maintenance.
- **AI SQL assistance:** optional convenience, not a core DBA operational need; it also introduces data-handling and trust considerations.
- **Schema migration authoring:** useful for development teams, but a separate workflow with versioning and deployment semantics. Do not treat it as a substitute for production change-management tools.
- **Automated tuning or destructive remediation:** avoid until recommendations can be evidence-based, explainable, and explicitly approved.

## Cross-cutting acceptance criteria

- All database work stays asynchronous so long-running operations do not freeze the UI.
- Display PostgreSQL errors and missing permissions clearly; never present unavailable statistics as zero or healthy.
- State the server version, required privileges/extensions, and whether a command executes or changes data.
- Use confirmation for destructive actions and generated-SQL previews for DDL where applicable.
- Test against supported PostgreSQL versions and include cases for restricted roles, missing optional extensions, cancellation, and partial failures.
- Keep new features optional where server support or privileges vary; do not require server-side extensions for core pg-mini operation.

## Suggested delivery order

1. P0 safe, controllable SQL operations.
2. P1 backup/restore feedback and verification.
3. P2 performance diagnosis.
4. P3 table/index maintenance visibility and scoped actions.
5. P4 replication/WAL risk visibility.

This is a capability-based order, not a date commitment. Reassess after each phase using DBA feedback and supported-server testing.
