use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use rusty_money::iso;
use std::io::{self, BufRead, Write};

use crate::models::{Condition, Criteria, Field, Matcher, Value};

mod categorize;
mod db;
mod import;
mod models;
mod money;
mod report;
mod server;

#[cfg(test)]
mod test_util;

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

    /// Import transactions from a CSV, QIF or PDF statement
    Import {
        file: String,
        /// Account number; required for QIF and formats without embedded account number (e.g. Amex, YouTrip)
        #[arg(long)]
        account: Option<String>,
        /// Importer to use; `auto` picks by file type and tries every matching parser
        #[arg(long, value_enum, default_value_t = import::Format::Auto)]
        format: import::Format,
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
    /// Remove an account and all its transactions
    Remove {
        /// Account name or number
        account: String,
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
    ///   fintrack rule add "Food" --field description --pattern "(?i)grab|foodpanda|mcdonalds"
    ///   fintrack rule add "Utilities" --field ref2 --pattern "(?i)sp group|singapore power" --priority 10
    ///   fintrack rule add "Food" --criteria '{"and":[{"field":"code","equals":"POS"}],"or":[{"field":"description","includes":"GRAB"},{"field":"description","includes":"KOPI"}]}'
    Add {
        /// Category to assign when the rule matches
        category: String,
        /// Field to match against: description | ref1 | ref2 | ref3 | code | amount
        ///
        /// To match several fields, use `--criteria` with an `or` group.
        ///
        /// `amount` matches an exact integer in minor units (e.g. cents):
        /// negative matches debit, positive matches credit (`-1250` = debit of 12.50).
        #[arg(long, requires = "pattern", conflicts_with = "criteria")]
        field: Option<String>,
        /// Regex pattern (case-insensitive flag: add (?i) prefix), or an
        /// integer for `amount`
        #[arg(
            long,
            allow_hyphen_values = true,
            requires = "field",
            required_unless_present = "criteria",
            conflicts_with = "criteria"
        )]
        pattern: Option<String>,
        /// Full JSON criteria: {"and": [...], "or": [...]}, each condition being
        /// {"field": <field>, "equals"|"includes"|"matches": <value>}.
        /// Every `and` condition must match, plus at least one `or` (if any).
        /// `amount` only supports `equals` with an integer value.
        #[arg(long)]
        criteria: Option<String>,
        /// Only apply the rule to transactions of this account (account number)
        #[arg(long)]
        account: Option<String>,
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
        /// Account number
        #[arg(long)]
        account: String,
    },
    /// List individual transactions
    Transactions {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        /// Filter to a category (includes its sub-categories)
        #[arg(long)]
        category: Option<String>,
        /// Account number
        #[arg(long)]
        account: String,
        /// Show only transactions that haven't been categorized yet
        #[arg(long)]
        uncategorized: bool,
    },
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = shellexpand::tilde(&cli.db).into_owned();
    let conn = db::open(&db_path)?;

    match cli.command {
        // ── Accounts ──────────────────────────────────────────────────────────
        Commands::Account(cmd) => match cmd {
            AccountCmd::List => {
                let accounts = db::list_accounts(&conn)?;
                if accounts.is_empty() {
                    println!(
                        "No accounts yet. Add one with:\n  fintrack account add --name <name> --number <number>"
                    );
                } else {
                    let table = tabled::Table::new(&accounts);
                    println!("{table}");
                }
            }
            AccountCmd::Add {
                name,
                number,
                bank,
                currency,
            } => {
                let currency = iso::find(&currency)
                    .ok_or_else(|| anyhow!("unknown currency: '{}'", currency))?;
                let id = db::add_account(&conn, &name, &number, &bank, currency)?;
                println!("Added account #{id}: {name} ({number})");
            }
            AccountCmd::Remove { account } => {
                let acc = db::find_account(&conn, &account)?
                    .ok_or_else(|| anyhow!("Account not found: '{account}'"))?;
                let tx_count = db::count_transactions_for_account(&conn, acc.id)?;
                println!(
                    "This will permanently delete account '{}' ({}) and its {tx_count} transaction(s).",
                    acc.name, acc.number
                );
                print!("Confirm? [y/N] ");
                io::stdout().flush()?;
                let mut line = String::new();
                io::stdin().lock().read_line(&mut line)?;
                if line.trim().eq_ignore_ascii_case("y") {
                    db::remove_account(&conn, acc.id)?;
                    println!(
                        "Removed account '{}' ({}) and {tx_count} transaction(s).",
                        acc.name, acc.number
                    );
                } else {
                    println!("Aborted.");
                }
            }
        },

        // ── Import ────────────────────────────────────────────────────────────
        Commands::Import {
            file,
            account,
            format,
        } => {
            let result = import::import_file(&conn, &file, format, account)?;
            println!(
                "Account : {} ({})\nImported: {}  |  Skipped (duplicates): {}  |  Errors: {}",
                result.account.name,
                result.account.number,
                result.imported,
                result.skipped,
                result.errored,
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
                    let table = tabled::Table::new(&cats);
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
                    let table = tabled::Table::new(&rules);
                    println!("{table}");
                }
            }
            RuleCmd::Add {
                category,
                field,
                pattern,
                criteria,
                account,
                priority,
            } => {
                let criteria = match (criteria, field, pattern) {
                    (Some(json), _, _) => serde_json::from_str::<Criteria>(&json)
                        .map_err(|e| anyhow!("Invalid criteria: {e}"))?,
                    (None, Some(field), Some(pattern)) => {
                        let field: Field = field.parse()?;
                        // integer fields match exactly; everything else is a regex.
                        let matcher =
                            if field.is_integer() {
                                Matcher::Equals(Value::Integer(pattern.parse().map_err(|_| {
                                    anyhow!("`{field}` expects an integer, got '{pattern}'")
                                })?))
                            } else {
                                Matcher::Matches(regex::Regex::new(&pattern).map_err(|e| {
                                    anyhow!("Invalid regex pattern '{pattern}': {e}")
                                })?)
                            };
                        Criteria::single(Condition::new(field, matcher).map_err(|e| anyhow!(e))?)
                    }
                    _ => unreachable!("clap requires --field/--pattern or --criteria"),
                };

                let cat = db::find_category(&conn, &category)?
                    .ok_or_else(|| anyhow!("Category not found: '{category}'"))?;
                let account = account
                    .map(|a| {
                        db::find_account(&conn, &a)?
                            .ok_or_else(|| anyhow!("Account not found: '{a}'"))
                    })
                    .transpose()?;
                let id = db::add_rule(
                    &conn,
                    cat.id,
                    account.as_ref().map(|a| a.id),
                    &criteria,
                    priority,
                )?;
                let scope = account
                    .map(|a| format!(" [account {}]", a.number))
                    .unwrap_or_default();
                println!("Added rule #{id}: {criteria}{scope} → {category} (priority {priority})");
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
                    println!(
                        "No transactions found for account '{}' ({}).",
                        acc.name, acc.number
                    );
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
                let account = db::find_account(&conn, &account)?
                    .ok_or_else(|| anyhow!("Account not found: '{account}'"))?;
                report::summary(&conn, from.as_deref(), to.as_deref(), &account)?;
            }
            ReportCmd::Transactions {
                from,
                to,
                category,
                account,
                uncategorized,
            } => {
                let account = db::find_account(&conn, &account)?
                    .ok_or_else(|| anyhow!("Account not found: '{account}'"))?;
                report::transactions(
                    &conn,
                    from.as_deref(),
                    to.as_deref(),
                    category.as_deref(),
                    &account,
                    uncategorized,
                )?;
            }
        },

        Commands::Server { port, no_open } => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            return rt.block_on(server::serve(conn, port, !no_open));
        }
    }

    Ok(())
}
