use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use comfy_table::Table;
use std::io::{self, BufRead, Write};

mod categorize;
mod db;
mod format;
mod import;
mod models;
mod report;
mod server;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name    = "fintrack",
    version,
    about   = "Personal finance tracker — import bank CSVs, categorize transactions, report by period",
    long_about = None,
)]
struct Cli {
    /// SQLite database file (created on first run)
    #[arg(long, default_value = "~/.fintrack.db", env = "FINTRACK_DB")]
    db: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage bank accounts
    #[command(subcommand)]
    Account(AccountCmd),

    /// Import transactions from a CSV export
    Import {
        /// Path to the CSV file
        file: String,
        /// CSV format name (must match a file in the built-in formats library)
        #[arg(long, default_value = "dbs")]
        format: String,
        /// Account number or name (auto-detected from the CSV if omitted)
        #[arg(long)]
        account: Option<String>,
        /// Bank name, used only when auto-creating a new account
        #[arg(long, default_value = "DBS")]
        bank: String,
        /// Currency fallback, used when the format cannot detect it from the CSV
        #[arg(long, default_value = "SGD")]
        currency: String,
    },

    /// Manage spending categories
    #[command(subcommand)]
    Category(CategoryCmd),

    /// Manage categorization rules (regex-based)
    #[command(subcommand)]
    Rule(RuleCmd),

    /// Manage transactions
    #[command(subcommand)]
    Transaction(TransactionCmd),

    /// Re-apply all rules to every transaction
    Categorize,

    /// Generate reports
    #[command(subcommand)]
    Report(ReportCmd),

    /// Start the web reporting interface
    Server {
        /// Port to listen on
        #[arg(long, short, default_value_t = 7878)]
        port: u16,
        /// Skip opening the browser automatically
        #[arg(long)]
        no_open: bool,
    },
}

#[derive(Subcommand)]
enum AccountCmd {
    /// List all accounts
    List,
    /// Add a bank account
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        number: String,
        #[arg(long, default_value = "DBS")]
        bank: String,
        #[arg(long, default_value = "SGD")]
        currency: String,
    },
}

#[derive(Subcommand)]
enum CategoryCmd {
    /// List all categories (with hierarchy)
    List,
    /// Add a category
    Add {
        name: String,
        /// Optional parent category name
        #[arg(long)]
        parent: Option<String>,
    },
    /// Remove a category (rules referencing it are also deleted)
    Remove { name: String },
}

#[derive(Subcommand)]
enum RuleCmd {
    /// List rules, optionally filtered by category
    List {
        #[arg(long)]
        category: Option<String>,
    },
    /// Add a categorization rule
    ///
    /// Examples:
    ///   fintrack rule add "Food" --pattern "(?i)grab|foodpanda|mcdonalds"
    ///   fintrack rule add "Utilities" --field ref2 --pattern "(?i)sp group|singapore power" --priority 10
    Add {
        /// Category to assign when the rule matches
        category: String,
        /// Field to match against: description | ref1 | ref2 | ref3 | code | any
        #[arg(long, default_value = "any")]
        field: String,
        /// Regex pattern (case-insensitive flag: add (?i) prefix)
        #[arg(long)]
        pattern: String,
        /// Tie-breaking priority — higher value wins when multiple rules match
        #[arg(long, default_value_t = 0)]
        priority: i64,
    },
    /// Remove a rule by its ID
    Remove { id: i64 },
}

#[derive(Subcommand)]
enum TransactionCmd {
    /// Delete all transactions for an account
    Purge {
        /// Account name or number
        account: String,
    },
}

#[derive(Subcommand)]
enum ReportCmd {
    /// Spending summary grouped by category
    Summary {
        /// Start date (YYYY-MM-DD), inclusive
        #[arg(long)]
        from: Option<String>,
        /// End date (YYYY-MM-DD), inclusive
        #[arg(long)]
        to: Option<String>,
        /// Filter to one account (number or name)
        #[arg(long)]
        account: Option<String>,
    },
    /// List individual transactions
    Transactions {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        /// Filter to a specific category
        #[arg(long)]
        category: Option<String>,
        /// Filter to one account (number or name)
        #[arg(long)]
        account: Option<String>,
        /// Show only transactions that haven't been categorized yet
        #[arg(long)]
        uncategorized: bool,
    },
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = shellexpand::tilde(&cli.db).into_owned();

    // The server command opens its own connection inside the async runtime.
    if let Commands::Server { port, no_open } = cli.command {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        return rt.block_on(server::serve(&db_path, port, !no_open));
    }

    let conn = db::open(&db_path)?;

    match cli.command {
        // ── Accounts ──────────────────────────────────────────────────────────
        Commands::Account(cmd) => match cmd {
            AccountCmd::List => {
                let accounts = db::list_accounts(&conn)?;
                if accounts.is_empty() {
                    println!("No accounts yet. Add one with:\n  fintrack account add --name <name> --number <number>");
                } else {
                    let mut table = Table::new();
                    table.set_header(["ID", "Name", "Number", "Bank", "Currency"]);
                    for a in &accounts {
                        table.add_row([&a.id.to_string(), &a.name, &a.number, &a.bank, &a.currency]);
                    }
                    println!("{table}");
                }
            }
            AccountCmd::Add { name, number, bank, currency } => {
                let id = db::add_account(&conn, &name, &number, &bank, &currency)?;
                println!("Added account #{id}: {name} ({number})");
            }
        },

        // ── Import ────────────────────────────────────────────────────────────
        Commands::Import { file, format, account, bank, currency } => {
            let result = import::import_csv(&conn, &file, &format, account.as_deref(), &bank, &currency)?;
            println!(
                "Account : {} ({})\nImported: {}  |  Skipped (duplicates): {}",
                result.account_name, result.account_number, result.imported, result.skipped,
            );
            if result.imported > 0 {
                let categorized = categorize::apply_rules(&conn)?;
                println!("Auto-categorized: {categorized} transactions");
            }
        }

        // ── Categories ────────────────────────────────────────────────────────
        Commands::Category(cmd) => match cmd {
            CategoryCmd::List => {
                let cats = db::list_categories(&conn)?;
                if cats.is_empty() {
                    println!("No categories yet. Add one with:\n  fintrack category add <name>");
                } else {
                    let mut table = Table::new();
                    table.set_header(["ID", "Name", "Parent"]);
                    for c in &cats {
                        let parent = c.parent_id
                            .and_then(|pid| cats.iter().find(|p| p.id == pid))
                            .map(|p| p.name.as_str())
                            .unwrap_or("-");
                        table.add_row([&c.id.to_string(), &c.name, parent]);
                    }
                    println!("{table}");
                }
            }
            CategoryCmd::Add { name, parent } => {
                let parent_id = parent
                    .as_deref()
                    .map(|p| -> Result<i64> {
                        db::find_category(&conn, p)?
                            .ok_or_else(|| anyhow!("Parent category not found: '{p}'"))
                            .map(|c| c.id)
                    })
                    .transpose()?;
                let id = db::add_category(&conn, &name, parent_id)?;
                println!("Added category #{id}: {name}");
            }
            CategoryCmd::Remove { name } => {
                let cat = db::find_category(&conn, &name)?
                    .ok_or_else(|| anyhow!("Category not found: '{name}'"))?;
                db::remove_category(&conn, cat.id)?;
                println!("Removed category '{name}' (and its rules).");
            }
        },

        // ── Rules ─────────────────────────────────────────────────────────────
        Commands::Rule(cmd) => match cmd {
            RuleCmd::List { category } => {
                let rules = db::list_rules(&conn, category.as_deref())?;
                if rules.is_empty() {
                    println!("No rules found.");
                } else {
                    let mut table = Table::new();
                    table.set_header(["ID", "Category", "Field", "Pattern", "Priority"]);
                    for (r, cat_name) in &rules {
                        table.add_row([
                            &r.id.to_string(),
                            cat_name,
                            &r.field,
                            &r.pattern,
                            &r.priority.to_string(),
                        ]);
                    }
                    println!("{table}");
                }
            }
            RuleCmd::Add { category, field, pattern, priority } => {
                const VALID_FIELDS: &[&str] = &["description", "ref1", "ref2", "ref3", "code", "any"];
                if !VALID_FIELDS.contains(&field.as_str()) {
                    anyhow::bail!(
                        "Invalid field '{}'. Must be one of: {}",
                        field, VALID_FIELDS.join(", ")
                    );
                }
                regex::Regex::new(&pattern)
                    .map_err(|e| anyhow!("Invalid regex pattern '{pattern}': {e}"))?;

                let cat = db::find_category(&conn, &category)?
                    .ok_or_else(|| anyhow!("Category not found: '{category}'"))?;
                let id = db::add_rule(&conn, cat.id, &field, &pattern, priority)?;
                println!("Added rule #{id}: [{field}] =~ /{pattern}/ → {category} (priority {priority})");
            }
            RuleCmd::Remove { id } => {
                db::remove_rule(&conn, id)?;
                println!("Removed rule #{id}.");
            }
        },

        // ── Transactions ──────────────────────────────────────────────────────
        Commands::Transaction(cmd) => match cmd {
            TransactionCmd::Purge { account } => {
                let acc = db::find_account(&conn, &account)?
                    .ok_or_else(|| anyhow!("Account not found: '{account}'"))?;
                let count = db::count_transactions_for_account(&conn, acc.id)?;
                if count == 0 {
                    println!("No transactions found for account '{}' ({}).", acc.name, acc.number);
                } else {
                    println!(
                        "This will permanently delete {count} transaction(s) for account '{}' ({}).",
                        acc.name, acc.number
                    );
                    print!("Confirm? [y/N] ");
                    io::stdout().flush()?;
                    let mut line = String::new();
                    io::stdin().lock().read_line(&mut line)?;
                    if line.trim().eq_ignore_ascii_case("y") {
                        let deleted = db::delete_transactions_for_account(&conn, acc.id)?;
                        println!("Deleted {deleted} transaction(s).");
                    } else {
                        println!("Aborted.");
                    }
                }
            }
        },

        // ── Categorize ────────────────────────────────────────────────────────
        Commands::Categorize => {
            let n = categorize::apply_rules(&conn)?;
            println!("Categorized {n} transactions.");
        }

        // ── Reports ───────────────────────────────────────────────────────────
        Commands::Report(cmd) => match cmd {
            ReportCmd::Summary { from, to, account } => {
                report::summary(&conn, from.as_deref(), to.as_deref(), account.as_deref())?;
            }
            ReportCmd::Transactions { from, to, category, account, uncategorized } => {
                report::transactions(
                    &conn,
                    from.as_deref(),
                    to.as_deref(),
                    category.as_deref(),
                    account.as_deref(),
                    uncategorized,
                )?;
            }
        },

        // Handled above before the DB connection is opened
        Commands::Server { .. } => unreachable!(),

    }

    Ok(())
}
