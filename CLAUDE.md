# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# fintrack — Claude context

Personal finance tracker. Imports bank CSVs (DBS and others via a YAML format DSL) and QIF files, categorises transactions with regex rules, and reports spending. A local web reporting UI is served via `fintrack server`.

## Build, test & install

```bash
# Run tests
cargo test

# After changing Rust code only
cargo build

# After changing web UI (web/src/**)
cd web && npm run build   # outputs to web/dist/ (embedded by rust-embed)
cd ..  && cargo build     # re-embeds the updated dist/

# Install to ~/.cargo/bin (makes `fintrack` available in PATH)
cargo install --path .
```

The `build.rs` script tells Cargo to recompile whenever `web/dist/` changes, so the two-step above is all that's needed.

### All-in-one binary (for use outside development)

To produce a self-contained binary with all web assets baked in (no separate `web/` directory needed at runtime):

```bash
cd web && npm run build   # compile frontend → web/dist/
cd ..
cargo install --path .    # embeds web/dist/ and installs to ~/.cargo/bin/fintrack
```

The resulting `~/.cargo/bin/fintrack` binary includes the full web UI via `rust-embed` and has no runtime dependencies beyond the SQLite database file (`~/.fintrack.db`). It can be copied to any machine with a compatible OS/architecture and run directly.

## Project layout

```
src/
  main.rs           — CLI (clap): Account, Import, Category, Rule, Transaction, Categorize, Report, Server
  models.rs         — Plain structs: Account, Category, Rule
  db.rs             — All SQLite reads/writes (rusqlite); calls rusqlite_migration on open
  migrations/
    01_initial_schema.sql — Full schema DDL (accounts, categories, transactions, rules + indexes)
  format.rs         — YAML format DSL loader/parser; rust-embeds formats/*.yaml
  import.rs         — CSV/QIF parsing and dedup-import (uses format.rs for CSV)
  qif.rs            — QIF file parsing
  categorize.rs     — Applies regex rules to every transaction; highest priority wins
  report.rs         — CLI table output for summary and transaction listing
  server.rs         — Axum HTTP server: JSON API + rust-embed static file serving
  build.rs          — Tells Cargo to watch web/dist/ for changes

formats/
  dbs.yaml          — CSV format definition for DBS bank exports (supports 3 column layouts)

tests/
  fixtures/         — Sample CSV/QIF files used by unit tests in format.rs

web/
  src/           — TypeScript + Preact source
    App.tsx      — Root component; tab state
    store.ts     — Global filter signals (filterFrom, filterTo, filterAccount) — kept separate to avoid circular imports (App imports components, components import signals)
    api.ts       — Typed fetch wrappers for all four API endpoints
    types.ts     — Shared TypeScript interfaces (Account, Category, Transaction, etc.)
    app.css      — Design tokens (CSS vars), light/dark theme, all component styles
    components/
      FilterBar.tsx    — Date range inputs, quick presets, account dropdown
      Summary.tsx      — Totals cards + category table with inline bar chart
      Transactions.tsx — Searchable/filterable/paginated transaction list
  dist/          — Built assets (committed; overwritten by npm run build)
  package.json
  vite.config.ts — Dev server on :3000, proxies /api to :7878
```

## Key architectural decisions

- **SQLite only** — single `~/.fintrack.db` file (overridable via `--db` flag or `FINTRACK_DB` env var); WAL mode; foreign keys on. No ORM.
- **rust-embed** — the entire `web/dist/` is baked into the binary at compile time. No separate asset deployment needed. `formats/*.yaml` are also rust-embedded (into `format.rs` via the `FormatAssets` struct).
- **Schema migrations** — managed by `rusqlite_migration`; SQL lives in `src/migrations/`. Add a new `M::up(include_str!(...))` entry to `db::migrations()` to add a migration. `rusqlite_migration` temporarily disables foreign keys during migration, so they are re-enabled explicitly after.
- **CSV format DSL** — `formats/*.yaml` files describe how to parse a bank's CSV: which cell holds the account number, which row is the header, and which columns map to which transaction fields. `format::load(name)` resolves by filename. To add a new bank, add a `formats/<bank>.yaml` and a fixture CSV in `tests/fixtures/`.
- **Async only for the server** — all other CLI commands are synchronous. A `tokio` runtime is created on demand inside the `Server` command branch; `main()` stays `fn main()`.
- **`Arc<Mutex<Connection>>`** — rusqlite `Connection` is `Send` but not `Sync`. Wrapped in a `Mutex` for sharing across async Axum handlers; DB work runs inside `tokio::task::spawn_blocking`.
- **Preact signals** — global filter signals live in `store.ts` (not `App.tsx`) to avoid a circular import: `App` imports the view components, which need the signals, so the signals must be in a file neither side imports. Components subscribe implicitly by reading `.value`. `effect()` (not `useEffect`) is used for data-fetching side effects so dependency tracking is automatic. Use `useSignal` (not `signal`) for signals created inside a component body.
- **No router** — tab switching is a signal. One `<main>` swaps between `<Summary>` and `<Transactions>`.
- **Client-side search** — the description/ref search in Transactions is applied client-side on the current page of results; category/date/account filters are server-side query params.

## API endpoints

All under `/api`:

| Method | Path | Query params |
|--------|------|--------------|
| GET | `/api/accounts` | — |
| GET | `/api/categories` | — |
| GET | `/api/summary` | `from`, `to`, `account` |
| GET | `/api/transactions` | `from`, `to`, `category`, `account`, `uncategorized`, `limit`, `offset` |

Date params are `YYYY-MM-DD` strings; `account` matches by name or number; `limit` defaults to 100 for summary, 50 for transactions.

## Development workflow

```bash
# Terminal 1 — API backend (auto-reloads on cargo run)
cargo run -- server --no-open

# Terminal 2 — Frontend with HMR
cd web && npm run dev
# Open http://localhost:3000 (Vite proxies /api → :7878)
```

When iterating on UI only, there's no need to restart the Rust server.

## Database schema (summary)

```sql
accounts      (id, name, number, bank, currency)
categories    (id, name, parent_id → categories)
transactions  (id, account_id, date, code, description, ref1, ref2, ref3,
               status, debit, credit, hash UNIQUE, category_id → categories)
rules         (id, category_id, field, pattern, priority)
```

`field` is one of: `description`, `ref1`, `ref2`, `ref3`, `code`, `any`.

## Things not yet built (potential next steps)

- Admin UI (account/category/rule management) — intentionally deferred
- Month-over-month comparison in the summary view
- CSV export from the transactions view
- Charts (spending trend over time)
