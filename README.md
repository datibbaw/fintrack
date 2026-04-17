# fintrack

A personal finance tracker. Import transaction CSVs from DBS/POSB, American Express, and other banks, categorise spending with regex rules, and explore your finances through a local web UI or CLI reports.

All data stays on your machine — fintrack stores everything in a single SQLite file (`~/.fintrack.db`) and serves the web UI on localhost only.

---

## Features

- **Import** transaction CSVs from DBS/POSB (multiple export layouts) and American Express, with a YAML DSL for adding new banks
- **Categorise** transactions automatically using regex rules with priority tie-breaking
- **Hierarchical categories** — roll up subcategory totals into parent categories
- **Web UI** — interactive summary and transaction browser with date range, account, and category filters
- **CLI reports** — spending summaries and transaction listings in your terminal
- **Idempotent imports** — re-importing the same CSV is safe; duplicates are silently skipped

---

## Installation

### Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)
- [Node.js](https://nodejs.org/) (v18 or later) — only needed if you want to build the frontend yourself

### Install from source

```bash
git clone https://github.com/datibbaw/fintrack.git
cd fintrack

# Build the frontend first (pre-built assets are committed, so this is optional)
cd web && npm install && npm run build && cd ..

# Install the binary to ~/.cargo/bin/
cargo install --path .
```

The resulting `fintrack` binary is self-contained — the entire web UI is embedded in it via [`rust-embed`](https://github.com/pyros2097/rust-embed). No separate web server, no runtime dependencies beyond the SQLite database.

---

## Quick start

```bash
# 1. Import a DBS CSV export (format auto-detected; account auto-created)
fintrack import --format dbs ~/Downloads/statement.csv

# Or import an Amex CSV (no account number in file, so --account is required)
fintrack import --format amex --account "Amex Platinum" ~/Downloads/activity.csv

# 2. Add some spending categories
fintrack category add Food
fintrack category add "Dining Out" --parent Food
fintrack category add Transport
fintrack category add Utilities

# 3. Add categorization rules (regex patterns)
fintrack rule add "Dining Out" --pattern "(?i)grab food|foodpanda|mcdonalds|starbucks"
fintrack rule add "Transport"  --pattern "(?i)grab|comfort delgro|ez-link|bus|mrt"
fintrack rule add "Utilities"  --pattern "(?i)sp group|singapore power|m1|singtel|starhub"

# 4. Open the web UI
fintrack server
```

---

## Importing transactions

### Supported formats

| `--format` | Bank | Notes |
|---|---|---|
| `dbs` | DBS / POSB | Auto-detects 8-, 9-, and 12-column export layouts; extracts account number and currency from the file |
| `amex` | American Express | Single-column signed amount (positive = charge, negative = payment); requires `--account` |

### DBS / POSB

Export your statement from DBS iBanking as a CSV. The format is detected automatically — the same `dbs` format handles savings, current, and credit card exports.

```bash
fintrack import --format dbs ~/Downloads/statement.csv
```

If the account is new, it is created automatically using the name and number embedded in the CSV header. Re-importing the same file is safe; duplicate rows are silently skipped.

### American Express

Download your activity CSV from the Amex website (Activity > Download). Because Amex CSVs do not contain a card number, you must specify the account with `--account`. If the account doesn't exist yet, create it first:

```bash
fintrack account add "Amex Platinum" --number 3782-822463-10005 --bank Amex --currency SGD
fintrack import --format amex --account "Amex Platinum" ~/Downloads/activity.csv
```

Amex uses a single signed `Amount` column — positive values are stored as debits (charges), negative values as credits (payments and cashback).

### QIF files

QIF files (exported by some banks and accounting tools) do not contain account information, so `--account` is always required:

```bash
fintrack import --account "My Card" ~/Downloads/export.qif
```

### Adding a new bank

Each bank format is a small YAML file in `formats/`. The DSL lets you specify:

- How to locate the account number and currency in the file header (optional)
- Which row contains the column headers, and which column maps to each transaction field

Fields available for column mappings: `date`, `description`, `code`, `ref1`, `ref2`, `ref3`, `status`, `debit`, `credit`, and `amount` (a signed column where positive → debit, negative → credit).

See `formats/dbs.yaml` and `formats/amex.yaml` for worked examples.

---

## CLI reference

```
fintrack [--db <path>] <command>

Global options:
  --db <path>    Path to the SQLite database (default: ~/.fintrack.db)
                 Can also be set via the FINTRACK_DB environment variable.
```

### Commands

| Command | Description |
|---|---|
| `fintrack import <file>` | Import a CSV or QIF file |
| `fintrack categorize` | Re-apply all rules to every transaction |
| `fintrack server` | Start the web UI (opens browser automatically) |
| `fintrack account list` | List registered accounts |
| `fintrack account add` | Register an account manually |
| `fintrack category list` | List all categories |
| `fintrack category add <name>` | Add a category (use `--parent` for subcategories) |
| `fintrack category remove <name>` | Remove a category and its rules |
| `fintrack rule list` | List all rules |
| `fintrack rule add <category>` | Add a categorisation rule |
| `fintrack rule remove <id>` | Remove a rule by ID |
| `fintrack transaction purge <account>` | Delete all transactions for an account |
| `fintrack report summary` | Print a spending summary grouped by category |
| `fintrack report transactions` | List individual transactions |

Use `--help` on any command or subcommand for full options:

```bash
fintrack import --help
fintrack rule add --help
fintrack report summary --help
```

### Categorisation rules

Rules match transaction fields against a regex pattern. The highest-priority matching rule wins.

```bash
# Match any field (description, ref1, ref2, ref3, code)
fintrack rule add "Food" --pattern "(?i)fairprice|cold storage|giant"

# Match a specific field only
fintrack rule add "Utilities" --field ref2 --pattern "(?i)sp group" --priority 10

# Valid fields: description | ref1 | ref2 | ref3 | code | any (default)
```

After adding or modifying rules, run `fintrack categorize` to re-apply them to all existing transactions.

---

## Web UI

```bash
fintrack server            # Opens http://localhost:7878 automatically
fintrack server --port 9000 --no-open   # Custom port, no auto-open
```

The UI has two views:

- **Summary** — spending totals per category for the selected period, with inline bar charts and parent category rollup
- **Transactions** — paginated transaction list with server-side filtering (date, account, category) and client-side description search

---

## Development

### Project layout

```
src/
  main.rs        — CLI (clap): all commands and subcommands
  models.rs      — Plain structs: Account, Category, Rule
  db.rs          — SQLite reads/writes (rusqlite); schema migrations
  import.rs      — CSV/QIF parsing and dedup-import
  categorize.rs  — Applies regex rules; highest priority wins
  report.rs      — CLI table output (summary + transactions)
  server.rs      — Axum HTTP server: JSON API + embedded static files
  build.rs       — Tells Cargo to watch web/dist/ for changes

web/
  src/           — TypeScript + Preact source
    App.tsx      — Root component; tab state
    store.ts     — Global filter signals (date range, account)
    api.ts       — Typed fetch wrappers for all API endpoints
    types.ts     — Shared TypeScript interfaces
    app.css      — Design tokens, light/dark theme, all component styles
    components/
      FilterBar.tsx    — Date range picker, quick presets, account dropdown
      Summary.tsx      — Category totals cards + bar chart table
      Transactions.tsx — Searchable, filterable, paginated transaction list
      Categories.tsx   — Category and rule management panel
  dist/          — Pre-built assets (committed; overwritten by npm run build)
```

### Running locally

```bash
# Terminal 1 — Rust API server (port 7878)
cargo run -- server --no-open

# Terminal 2 — Vite dev server with HMR (port 3000, proxies /api → 7878)
cd web && npm run dev
```

Open [http://localhost:3000](http://localhost:3000). The Vite proxy means you only need to restart the Rust server when changing Rust code; frontend changes reflect instantly via HMR.

### Building

```bash
# After changing Rust code only
cargo build

# After changing frontend code
cd web && npm run build   # outputs to web/dist/
cd .. && cargo build      # re-embeds the updated dist/
```

### Database schema

```sql
accounts      (id, name, number, bank, currency)
categories    (id, name, parent_id → categories)
transactions  (id, account_id, date, code, description, ref1, ref2, ref3,
               status, debit, credit, hash UNIQUE, category_id → categories)
rules         (id, category_id, field, pattern, priority)
```

Schema migrations run automatically on startup — no manual steps needed after pulling new code.

### API endpoints

All under `/api`:

| Method | Path | Query params |
|---|---|---|
| GET | `/api/accounts` | — |
| GET | `/api/categories` | — |
| GET | `/api/summary` | `from`, `to`, `account` |
| GET | `/api/transactions` | `from`, `to`, `category`, `account`, `uncategorized`, `limit`, `offset` |

Date params use `YYYY-MM-DD` format. `account` matches by name or number.

### Key architectural decisions

- **SQLite only** — single file, WAL mode, foreign keys on. No ORM.
- **rust-embed** — `web/dist/` is baked into the binary at compile time.
- **Async only for the server** — CLI commands are synchronous; a Tokio runtime is created on demand for the `server` subcommand only.
- **Preact signals** — global filter state lives in `store.ts` (not `App.tsx`) to avoid a circular import. Components subscribe implicitly by reading `.value`.
- **Client-side search** — description/ref search in the Transactions view runs client-side on the current page; category/date/account filters are server-side query parameters.

---

## Contributing

Contributions are welcome. Please open an issue first for significant changes so we can discuss the approach before you invest time in an implementation.

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-change`)
3. Commit your changes
4. Open a pull request

---

## License

[MIT](LICENSE)
