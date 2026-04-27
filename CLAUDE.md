# CLAUDE.md

Guidance to Claude Code (claude.ai/code) for this repo.

# fintrack — Claude context

Personal finance tracker. Imports bank CSVs (DBS and others via YAML DSL) and QIF files, categorises transactions with regex rules, reports spending. Local web UI via `fintrack server`.

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

`build.rs` tells Cargo to recompile when `web/dist/` changes — two-step above is all needed.

### All-in-one binary (for use outside development)

Self-contained binary with all web assets baked in (no separate `web/` at runtime):

```bash
cd web && npm run build   # compile frontend → web/dist/
cd ..
cargo install --path .    # embeds web/dist/ and installs to ~/.cargo/bin/fintrack
```

`~/.cargo/bin/fintrack` includes full web UI via `rust-embed`. No runtime deps beyond `~/.fintrack.db`. Copy to any compatible machine and run directly.

## Project layout

```
src/
  main.rs           — CLI (clap): Account, Import (--account required), Category, Rule, Transaction, Categorize, Report, Server
  models/           — Plain structs: Account, Category, Rule, Transaction, TransactionBuilder
  db.rs             — All SQLite reads/writes (rusqlite); calls rusqlite_migration on open
  migrations/
    01_initial_schema.sql — Full schema DDL (accounts, categories, transactions, rules + indexes)
  readers/
    csv/reader.rs   — CSV format DSL loader/parser; rust-embeds specs/csv.yaml
  import.rs         — CSV/QIF parsing and dedup-import (uses readers/csv for CSV)
  qif.rs            — QIF file parsing
  categorize.rs     — Applies regex rules to every transaction; highest priority wins
  report.rs         — CLI table output for summary and transaction listing
  server.rs         — Axum HTTP server: JSON API + rust-embed static file serving
  build.rs          — Tells Cargo to watch web/dist/ for changes

specs/
  csv.yaml          — All supported CSV bank formats as a YAML list (rust-embedded)

tests/
  fixtures/         — Sample CSV/QIF files used by unit tests in readers/csv

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

- **SQLite only** — single `~/.fintrack.db` (overridable via `--db` or `FINTRACK_DB`); WAL mode; foreign keys on. No ORM.
- **rust-embed** — entire `web/dist/` baked into binary at compile time. No separate asset deployment. `specs/csv.yaml` also rust-embedded into `readers/csv/reader.rs` via `Specs` struct.
- **Schema migrations** — managed by `rusqlite_migration`; SQL in `src/migrations/`. Add `M::up(include_str!(...))` to `db::migrations()` for new migration. `rusqlite_migration` temporarily disables foreign keys during migration; re-enabled explicitly after.
- **CSV format DSL** — `specs/csv.yaml` is YAML list of named format specs. Each entry has `date_format`, optional `invert_amount_sign`, and `columns` (maps spreadsheet-style cell ref e.g. `A`/`C` plus header-matching regex `expression` to transaction `field`). `ReaderSpec` scans rows until record matches all column expressions, then uses that format. To add new bank: add entry to `specs/csv.yaml` and fixture CSV in `tests/fixtures/`.
- **Async only for server** — all other CLI commands synchronous. `tokio` runtime created on demand inside `Server` command branch; `main()` stays `fn main()`.
- **`Arc<Mutex<Connection>>`** — rusqlite `Connection` is `Send` not `Sync`. Wrapped in `Mutex` for sharing across async Axum handlers; DB work runs inside `tokio::task::spawn_blocking`.
- **Preact signals** — global filter signals in `store.ts` (not `App.tsx`) to avoid circular import: `App` imports view components, components need signals, so signals must be in file neither side imports. Components subscribe implicitly via `.value`. `effect()` (not `useEffect`) for data-fetching side effects — dependency tracking automatic. Use `useSignal` (not `signal`) for signals created inside component body.
- **No router** — tab switching is signal. One `<main>` swaps between `<Summary>` and `<Transactions>`.
- **Client-side search** — description/ref search in Transactions applied client-side on current page; category/date/account filters are server-side query params.

## API endpoints

All under `/api`:

| Method | Path | Query params |
|--------|------|--------------|
| GET | `/api/accounts` | — |
| GET | `/api/categories` | — |
| GET | `/api/summary` | `from`, `to`, `account` |
| GET | `/api/transactions` | `from`, `to`, `category`, `account`, `uncategorized`, `limit`, `offset` |

Date params: `YYYY-MM-DD`. `account` matches by name or account number. `limit` defaults to 100 for summary, 50 for transactions.

## Development workflow

```bash
# Terminal 1 — API backend (auto-reloads on cargo run)
cargo run -- server --no-open

# Terminal 2 — Frontend with HMR
cd web && npm run dev
# Open http://localhost:3000 (Vite proxies /api → :7878)
```

UI-only iteration: no need to restart Rust server.

## Database schema (summary)

```sql
accounts      (id, name, number, bank, currency)
categories    (id, name, parent_id → categories)
transactions  (id, account_id, date, code, description, ref1, ref2, ref3,
               status, debit, credit, hash UNIQUE, category_id → categories)
rules         (id, category_id, field, pattern, priority)
```

`field` one of: `description`, `ref1`, `ref2`, `ref3`, `code`, `any`.

## Things not yet built (potential next steps)

- Admin UI (account/category/rule management) — intentionally deferred
- Month-over-month comparison in summary view
- CSV export from transactions view
- Charts (spending trend over time)