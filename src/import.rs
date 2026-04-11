use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;

use crate::db;

pub struct ImportResult {
    pub account_name: String,
    pub account_number: String,
    pub imported: usize,
    pub skipped: usize,
}

// ── Column map ────────────────────────────────────────────────────────────────

/// Resolved indices into a CSV row for the fields we care about.
struct ColumnMap {
    date: usize,
    code: usize,
    description: usize,
    ref1: usize,
    ref2: usize,
    ref3: usize,
    status: usize,
    debit: usize,
    credit: usize,
}

/// Each format is a list of (field, expected_header_label) pairs.
/// Only the fields we need are listed; extra columns are ignored.
const KNOWN_FORMATS: &[&[(&str, &str)]] = &[
    // Format A — 9-column export ("Transaction Code", "Transaction Ref*")
    &[
        ("date",        "Transaction Date"),
        ("code",        "Transaction Code"),
        ("description", "Description"),
        ("ref1",        "Transaction Ref1"),
        ("ref2",        "Transaction Ref2"),
        ("ref3",        "Transaction Ref3"),
        ("status",      "Status"),
        ("debit",       "Debit Amount"),
        ("credit",      "Credit Amount"),
    ],
    // Format B — 12-column export ("Statement Code", "Supplementary Code", etc.)
    &[
        ("date",        "Transaction Date"),
        ("code",        "Statement Code"),
        ("description", "Description"),
        ("ref1",        "Supplementary Code"),
        ("ref2",        "Supplementary Code Description"),
        ("ref3",        "Client Reference"),
        ("status",      "Status"),
        ("debit",       "Debit Amount"),
        ("credit",      "Credit Amount"),
    ],
];

fn resolve_column_map(headers: &[&str]) -> Option<ColumnMap> {
    let index: HashMap<&str, usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| (*h, i))
        .collect();

    for format in KNOWN_FORMATS {
        let mut m: HashMap<&str, usize> = HashMap::new();
        let matched = format.iter().all(|(field, label)| {
            if let Some(&i) = index.get(label) {
                m.insert(field, i);
                true
            } else {
                false
            }
        });

        if matched {
            return Some(ColumnMap {
                date:        m["date"],
                code:        m["code"],
                description: m["description"],
                ref1:        m["ref1"],
                ref2:        m["ref2"],
                ref3:        m["ref3"],
                status:      m["status"],
                debit:       m["debit"],
                credit:      m["credit"],
            });
        }
    }
    None
}

// ── DBS CSV parsing ───────────────────────────────────────────────────────────

/// Parse a DBS/POSB CSV export. Returns (account_name, account_number, data_rows).
/// Data rows are arrays of 9 strings: [date, code, description, ref1, ref2, ref3, status, debit, credit].
fn parse_dbs_csv(content: &str) -> Result<(String, String, Vec<[String; 9]>)> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(content.as_bytes());

    let mut account_name = String::new();
    let mut account_number = String::new();
    let mut col_map: Option<ColumnMap> = None;
    let mut rows: Vec<[String; 9]> = Vec::new();

    for result in rdr.records() {
        let record = result?;

        if col_map.is_none() {
            let first = record.get(0).map(|s| s.trim()).unwrap_or("");

            // "Account Details For:" row carries the account name + number
            if first == "Account Details For:" {
                if let Some(details) = record.get(1) {
                    let details = details.trim();
                    // Format: "Household 000-00000-0" — split on the last space
                    if let Some(pos) = details.rfind(' ') {
                        account_name = details[..pos].trim().to_string();
                        account_number = details[pos + 1..].trim().to_string();
                    } else {
                        account_number = details.to_string();
                    }
                }
            }

            // Column header row: resolve which format this file uses
            if first == "Transaction Date" {
                let headers: Vec<&str> = record.iter().map(|s| s.trim()).collect();
                col_map = Some(
                    resolve_column_map(&headers).ok_or_else(|| {
                        anyhow::anyhow!(
                            "Unrecognised CSV column layout. Headers found: {:?}",
                            headers
                        )
                    })?,
                );
            }
            continue;
        }

        let cm = col_map.as_ref().unwrap();

        let date = record.get(cm.date).map(|s| s.trim()).unwrap_or("").to_string();
        if date.is_empty() {
            continue;
        }

        let get = |idx: usize| record.get(idx).map(|s| s.trim().to_string()).unwrap_or_default();

        rows.push([
            date,
            get(cm.code),
            get(cm.description),
            get(cm.ref1),
            get(cm.ref2),
            get(cm.ref3),
            get(cm.status),
            get(cm.debit),
            get(cm.credit),
        ]);
    }

    if col_map.is_none() {
        anyhow::bail!("No transaction header row found in CSV");
    }

    Ok((account_name, account_number, rows))
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%d %b %Y")
        .with_context(|| format!("Unrecognised date format: '{s}' (expected e.g. '28 Mar 2026')"))
}

fn parse_amount(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() { None } else { s.parse().ok() }
}

/// Deterministic hash for deduplication. Includes account_id so two accounts
/// can have identical-looking transactions without colliding.
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

// ── Public API ────────────────────────────────────────────────────────────────

pub fn import_csv(
    conn: &Connection,
    path: &str,
    account_hint: Option<&str>,
    bank: &str,
    currency: &str,
) -> Result<ImportResult> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Cannot read file: {path}"))?;

    let (csv_name, csv_number, rows) = parse_dbs_csv(&content)?;

    // Resolve account: prefer explicit hint, then auto-detect from CSV, then create.
    let account = if let Some(hint) = account_hint {
        db::find_account(conn, hint)?
            .ok_or_else(|| anyhow::anyhow!("Account not found: '{hint}'. Add it first with `fintrack account add`."))?
    } else {
        if let Some(a) = db::find_account(conn, &csv_number)? {
            a
        } else {
            let name = if csv_name.is_empty() { &csv_number } else { &csv_name };
            eprintln!("Auto-created account '{name}' ({csv_number})");
            db::add_account(conn, name, &csv_number, bank, currency)?;
            db::find_account(conn, &csv_number)?.unwrap()
        }
    };

    let mut imported = 0usize;
    let mut skipped = 0usize;

    for row in &rows {
        let [date_str, code, description, ref1, ref2, ref3, status, debit_str, credit_str] = row;

        let date = parse_date(date_str)?;
        let date_iso = date.format("%Y-%m-%d").to_string();
        let debit = parse_amount(debit_str);
        let credit = parse_amount(credit_str);

        let hash = make_hash(account.id, &date_iso, code, ref1, ref2, ref3, debit, credit);

        // INSERT OR IGNORE is the idempotency mechanism — the UNIQUE constraint on `hash`
        // silently discards any row that was already imported.
        let n = conn.execute(
            "INSERT OR IGNORE INTO transactions \
             (account_id, date, code, description, ref1, ref2, ref3, status, debit, credit, hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![account.id, date_iso, code, description, ref1, ref2, ref3, status, debit, credit, hash],
        )?;

        if n == 1 { imported += 1; } else { skipped += 1; }
    }

    Ok(ImportResult {
        account_name: account.name,
        account_number: account.number,
        imported,
        skipped,
    })
}
