use anyhow::Result;
use rusqlite::Connection;
use serde::Deserialize;
use serde_rusqlite::from_rows;
use tabled::{Table, Tabled};

use crate::{
    db,
    models::Account,
    money::{self, display_amount},
};

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
    #[tabled(rename = "Debit", display("display_amount", self))]
    total_debit: i64,
    #[tabled(rename = "Credit", display("display_amount", self))]
    total_credit: i64,
    #[tabled(display("display_amount", self))]
    net: i64,
    #[tabled(rename = "Transactions")]
    tx_count: i64,
    #[tabled(skip)]
    currency: money::CurrencyCode,
}

crate::impl_has_currency!(SummaryRow);

pub fn summary(
    conn: &Connection,
    from: Option<&str>,
    to: Option<&str>,
    account: &Account,
) -> Result<()> {
    let (mut filter_clause, mut vals) = db::build_filters(from, to);

    filter_clause.push_str(" AND t.account_id = ?");
    vals.push(account.id.to_string());

    let sql = format!(
        "SELECT \
           COALESCE(c.name, 'Uncategorized') AS category, \
           SUM(COALESCE(t.debit,  0)) AS total_debit, \
           SUM(COALESCE(t.credit, 0)) AS total_credit, \
           (SUM(COALESCE(t.credit, 0)) - SUM(COALESCE(t.debit, 0))) AS net, \
           COUNT(*) AS tx_count, \
           a.currency AS currency \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         JOIN  accounts a ON t.account_id = a.id \
         WHERE 1=1{filter_clause} \
         GROUP BY c.name, a.currency \
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

    let (total_debit, total_credit, total_txn) = rows.iter().fold((0i64, 0i64, 0i64), |acc, r| {
        (
            acc.0 + r.total_debit,
            acc.1 + r.total_credit,
            acc.2 + r.tx_count,
        )
    });

    let mut table_builder = Table::builder(&rows);
    table_builder.push_record([
        "Total",
        &account.amount_from_minor(total_debit).to_string(),
        &account.amount_from_minor(total_credit).to_string(),
        &account
            .amount_from_minor(total_credit - total_debit)
            .to_string(),
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
    #[tabled(display("display_amount", self))]
    debit: Option<i64>,
    #[tabled(display("display_amount", self))]
    credit: Option<i64>,
    #[tabled(skip)]
    currency: money::CurrencyCode,
}

crate::impl_has_currency!(TransactionRow);

fn short_description(desc: &str) -> &str {
    match desc.char_indices().nth(42) {
        Some((i, _)) => &desc[..i],
        None => desc,
    }
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

    let sql = format!(
        "SELECT t.date, t.code, t.description, t.ref2, \
                COALESCE(c.name, 'Uncategorized') AS category, \
                t.debit, \
                t.credit, \
                a.currency \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         JOIN  accounts a ON t.account_id = a.id \
         WHERE 1=1{filter_clause} \
         ORDER BY t.date DESC, t.id DESC"
    );

    let mut stmt = conn.prepare(&sql)?;

    let rows: Vec<TransactionRow> =
        from_rows::<TransactionRow>(stmt.query(rusqlite::params_from_iter(vals.iter()))?)
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
