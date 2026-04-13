use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::fs;

use crate::{db, format};

pub struct ImportResult {
    pub account_name: String,
    pub account_number: String,
    pub imported: usize,
    pub skipped: usize,
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%d %b %Y")
        .with_context(|| format!("unrecognised date format: '{s}' (expected e.g. '28 Mar 2026')"))
}

fn parse_amount(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        s.parse().ok()
    }
}

/// Deterministic hash for deduplication. Includes account_id so two accounts
/// can have identical-looking transactions without colliding.
#[allow(clippy::too_many_arguments)]
fn make_hash(
    account_id: i64,
    date: &str,
    code: &str,
    ref1: &str,
    ref2: &str,
    ref3: &str,
    debit: Option<f64>,
    credit: Option<f64>,
) -> String {
    let mut h = Sha256::new();
    h.update(format!(
        "{account_id}|{date}|{code}|{ref1}|{ref2}|{ref3}|{}|{}",
        debit.map(|v| v.to_string()).unwrap_or_default(),
        credit.map(|v| v.to_string()).unwrap_or_default(),
    ));
    hex::encode(h.finalize())
}

pub fn import_csv(
    conn: &Connection,
    path: &str,
    format_name: &str,
    account_hint: Option<&str>,
    bank: &str,
    currency_fallback: &str,
) -> Result<ImportResult> {
    let content = fs::read_to_string(path).with_context(|| format!("cannot read file: {path}"))?;

    let fmt = format::load(format_name)?;
    let parsed = format::apply(&fmt, &content)?;

    let csv_number = parsed.account_number.unwrap_or_default();
    let csv_name = parsed.account_name.unwrap_or_default();
    let currency = parsed
        .currency
        .as_deref()
        .unwrap_or(currency_fallback)
        .to_string();

    // Resolve account: prefer explicit hint, then auto-detect from CSV, then create.
    let account = if let Some(hint) = account_hint {
        db::find_account(conn, hint)?.ok_or_else(|| {
            anyhow::anyhow!(
                "account not found: '{hint}'. Add it first with `fintrack account add`."
            )
        })?
    } else if let Some(a) = db::find_account(conn, &csv_number)? {
        a
    } else {
        if csv_number.is_empty() {
            anyhow::bail!(
                "the '{format_name}' format could not detect an account number in '{path}'. \
                 Specify one with --account."
            );
        }
        eprintln!("Auto-created account '{csv_name}' ({csv_number})");
        db::add_account(conn, &csv_name, &csv_number, bank, &currency)?;
        db::find_account(conn, &csv_number)?.unwrap()
    };

    let mut imported = 0usize;
    let mut skipped = 0usize;

    for row in &parsed.rows {
        let date = parse_date(&row.date)?;
        let date_iso = date.format("%Y-%m-%d").to_string();
        let debit = parse_amount(&row.debit);
        let credit = parse_amount(&row.credit);

        let hash = make_hash(
            account.id, &date_iso, &row.code, &row.ref1, &row.ref2, &row.ref3, debit, credit,
        );

        // INSERT OR IGNORE — the UNIQUE constraint on `hash` silently discards duplicates.
        let n = conn.execute(
            "INSERT OR IGNORE INTO transactions \
             (account_id, date, code, description, ref1, ref2, ref3, status, debit, credit, hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                account.id,
                date_iso,
                row.code,
                row.description,
                row.ref1,
                row.ref2,
                row.ref3,
                row.status,
                debit,
                credit,
                hash
            ],
        )?;

        if n == 1 {
            imported += 1;
        } else {
            skipped += 1;
        }
    }

    Ok(ImportResult {
        account_name: account.name,
        account_number: account.number,
        imported,
        skipped,
    })
}
