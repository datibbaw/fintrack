use anyhow::Result;
use rusqlite::Connection;
use serde::Deserialize;
use serde_rusqlite::from_rows;
use tabled::{Table, Tabled};

use crate::{db, models::Account};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn period_label(from: Option<&str>, to: Option<&str>) -> String {
    match (from, to) {
        (Some(f), Some(t)) => format!("{f} → {t}"),
        (Some(f), None) => format!("from {f}"),
        (None, Some(t)) => format!("up to {t}"),
        (None, None) => "all time".to_string(),
    }
}

// ── Summary report ────────────────────────────────────────────────────────────
#[derive(Deserialize, Tabled)]
#[tabled(rename_all = "PascalCase")]
struct SummaryRow {
    category: String,
    #[tabled(rename = "Debit")]
    total_debit: f64,
    #[tabled(rename = "Credit")]
    total_credit: f64,
    #[tabled(format("{0:+.2}"))]
    net: f64,
    #[tabled(rename = "Transactions")]
    tx_count: i64,
}

pub fn summary(
    conn: &Connection,
    from: Option<&str>,
    to: Option<&str>,
    account: &Account,
) -> Result<()> {
    let (mut filter_clause, mut vals) = db::build_filters(from, to);

    filter_clause.push_str(" AND t.account_id = ?");
    vals.push(account.id.to_string());

    let factor = account.currency_factor() as f64;

    let sql = format!(
        "SELECT \
           COALESCE(c.name, 'Uncategorized') AS category, \
           SUM(COALESCE(t.debit,  0)) / CAST({factor} AS REAL) AS total_debit, \
           SUM(COALESCE(t.credit, 0)) / CAST({factor} AS REAL) AS total_credit, \
           (SUM(COALESCE(t.credit, 0)) - SUM(COALESCE(t.debit, 0))) / CAST({factor} AS REAL) AS net, \
           COUNT(*) AS tx_count \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         WHERE 1=1{filter_clause} \
         GROUP BY c.name \
         ORDER BY total_debit DESC"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = from_rows::<SummaryRow>(stmt.query(rusqlite::params_from_iter(vals.iter()))?)
        .collect::<serde_rusqlite::Result<Vec<_>>>()?;

    if rows.is_empty() {
        println!("No transactions found.");
        return Ok(());
    }

    println!("Period: {}\n", period_label(from, to));

    let mut table_builder = Table::builder(&rows);

    let (total_debit, total_credit, total_txn) = rows.iter().fold((0f64, 0f64, 0i64), |acc, r| {
        (
            acc.0 + r.total_debit,
            acc.1 + r.total_credit,
            acc.2 + r.tx_count,
        )
    });

    table_builder.push_record([
        "Total",
        &format!("{total_debit:.2}"),
        &format!("{total_credit:.2}"),
        &format!("{:+.2}", total_credit - total_debit),
        &total_txn.to_string(),
    ]);

    println!("{}", table_builder.build());
    Ok(())
}

// ── Transaction listing ───────────────────────────────────────────────────────

#[derive(Deserialize, Tabled)]
#[tabled(rename_all = "PascalCase")]
struct TransactionRow {
    date: String,
    code: String,
    #[tabled(display = "short_description")]
    description: String,
    ref2: String,
    category: String,
    #[tabled(display = "display_amount")]
    debit: Option<f64>,
    #[tabled(display = "display_amount")]
    credit: Option<f64>,
    account: String,
}

fn short_description(desc: &str) -> &str {
    if desc.len() > 42 {
        &desc[..42]
    } else {
        desc
    }
}

fn display_amount(v: &Option<f64>) -> String {
    v.map(|a| format!("{a:.2}")).unwrap_or_default()
}

pub fn transactions(
    conn: &Connection,
    from: Option<&str>,
    to: Option<&str>,
    category: Option<&str>,
    account: &Account,
    uncategorized: bool,
) -> Result<()> {
    let (mut filter_clause, mut vals) = db::build_filters(from, to);

    filter_clause.push_str(" AND t.account_id = ?");
    vals.push(account.id.to_string());

    if uncategorized {
        filter_clause.push_str(" AND t.category_id IS NULL");
    } else if let Some(cat) = category {
        filter_clause.push_str(" AND c.name = ?");
        vals.push(cat.to_string());
    }

    let factor = account.currency_factor();
    let sql = format!(
        "SELECT t.date, t.code, t.description, t.ref2, \
                COALESCE(c.name, 'Uncategorized') AS category, \
                t.debit / CAST({factor} AS REAL) AS debit, \
                t.credit / CAST({factor} AS REAL) AS credit, \
                a.name AS account \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         JOIN  accounts a ON t.account_id = a.id \
         WHERE 1=1{filter_clause} \
         ORDER BY t.date DESC, t.id DESC"
    );

    let mut stmt = conn.prepare(&sql)?;

    let rows = from_rows::<TransactionRow>(stmt.query(rusqlite::params_from_iter(vals.iter()))?)
        .collect::<serde_rusqlite::Result<Vec<_>>>()?;

    if rows.is_empty() {
        println!("No transactions found.");
        return Ok(());
    }

    let table = Table::new(&rows);

    println!("{table}");
    println!("({} transactions)", rows.len());
    Ok(())
}
