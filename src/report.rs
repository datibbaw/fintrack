use anyhow::Result;
use comfy_table::{presets::UTF8_FULL, Table};
use rusqlite::Connection;

use crate::db;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn period_label(from: Option<&str>, to: Option<&str>) -> String {
    match (from, to) {
        (Some(f), Some(t)) => format!("{f} → {t}"),
        (Some(f), None)    => format!("from {f}"),
        (None, Some(t))    => format!("up to {t}"),
        (None, None)       => "all time".to_string(),
    }
}

// ── Summary report ────────────────────────────────────────────────────────────

pub fn summary(
    conn: &Connection,
    from: Option<&str>,
    to: Option<&str>,
    account: Option<&str>,
) -> Result<()> {
    let (filter_clause, vals) = db::build_filters(from, to, account);

    let sql = format!(
        "SELECT \
           COALESCE(c.name, 'Uncategorized') AS category, \
           SUM(COALESCE(t.debit,  0)) AS total_debit, \
           SUM(COALESCE(t.credit, 0)) AS total_credit, \
           COUNT(*) AS tx_count \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         JOIN  accounts a ON t.account_id = a.id \
         WHERE 1=1{filter_clause} \
         GROUP BY c.name \
         ORDER BY total_debit DESC"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<(String, f64, f64, i64)> = stmt
        .query_map(rusqlite::params_from_iter(vals.iter()), |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    if rows.is_empty() {
        println!("No transactions found.");
        return Ok(());
    }

    println!("Period: {}\n", period_label(from, to));

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(["Category", "Debit", "Credit", "Net", "Txns"]);

    let mut total_debit = 0f64;
    let mut total_credit = 0f64;

    for (cat, debit, credit, count) in &rows {
        let net = credit - debit;
        total_debit += debit;
        total_credit += credit;
        table.add_row([
            cat.as_str(),
            &format!("{debit:.2}"),
            &format!("{credit:.2}"),
            &format!("{net:+.2}"),
            &count.to_string(),
        ]);
    }

    let total_net = total_credit - total_debit;
    table.add_row([
        "TOTAL",
        &format!("{total_debit:.2}"),
        &format!("{total_credit:.2}"),
        &format!("{total_net:+.2}"),
        "",
    ]);

    println!("{table}");
    Ok(())
}

// ── Transaction listing ───────────────────────────────────────────────────────

pub fn transactions(
    conn: &Connection,
    from: Option<&str>,
    to: Option<&str>,
    category: Option<&str>,
    account: Option<&str>,
    uncategorized: bool,
) -> Result<()> {
    let (mut filter_clause, mut vals) = db::build_filters(from, to, account);

    if uncategorized {
        filter_clause.push_str(" AND t.category_id IS NULL");
    } else if let Some(cat) = category {
        filter_clause.push_str(" AND c.name = ?");
        vals.push(cat.to_string());
    }

    let sql = format!(
        "SELECT t.date, t.code, t.description, t.ref2, \
                COALESCE(c.name, 'Uncategorized') AS category, \
                t.debit, t.credit, a.name AS acct \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         JOIN  accounts a ON t.account_id = a.id \
         WHERE 1=1{filter_clause} \
         ORDER BY t.date DESC, t.id DESC"
    );

    let mut stmt = conn.prepare(&sql)?;

    type Row = (String, String, String, String, String, Option<f64>, Option<f64>, String);
    let rows: Vec<Row> = stmt
        .query_map(rusqlite::params_from_iter(vals.iter()), |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    if rows.is_empty() {
        println!("No transactions found.");
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(["Date", "Code", "Description", "Ref", "Category", "Debit", "Credit", "Account"]);

    for (date, code, desc, ref2, cat, debit, credit, acct) in &rows {
        let desc_short = if desc.len() > 42 { &desc[..42] } else { desc.as_str() };
        table.add_row([
            date.as_str(),
            code.as_str(),
            desc_short,
            ref2.as_str(),
            cat.as_str(),
            &debit.map(|v| format!("{v:.2}")).unwrap_or_default(),
            &credit.map(|v| format!("{v:.2}")).unwrap_or_default(),
            acct.as_str(),
        ]);
    }

    println!("{table}");
    println!("({} transactions)", rows.len());
    Ok(())
}
