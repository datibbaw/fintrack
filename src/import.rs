use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::fs;

use crate::{db, format, qif};

#[derive(Debug)]
pub struct ImportResult {
    pub account_name: String,
    pub account_number: String,
    pub imported: usize,
    pub skipped: usize,
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    let s = s.trim();
    // Try several known formats in order.
    NaiveDate::parse_from_str(s, "%d %b %Y")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .or_else(|_| NaiveDate::parse_from_str(s, "%m/%d/%Y"))
        .with_context(|| {
            format!(
                "unrecognised date format: '{s}' \
                 (expected '28 Mar 2026', '2026-03-28', or '04/11/2026')"
            )
        })
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

    let file_number = parsed.account_number.unwrap_or_default();
    let file_name = parsed.account_name.unwrap_or_default();
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
    } else if let Some(a) = db::find_account(conn, &file_number)? {
        a
    } else {
        if file_number.is_empty() {
            anyhow::bail!(
                "the '{format_name}' format could not detect an account number in '{path}'. \
                 Specify one with --account."
            );
        }
        eprintln!("Auto-created account '{file_name}' ({file_number})");
        db::add_account(conn, &file_name, &file_number, bank, &currency)?;
        db::find_account(conn, &file_number)?.unwrap()
    };

    insert_rows(
        conn,
        &parsed.rows,
        account.id,
        &account.name,
        &account.number,
    )
}

pub fn import_qif(
    conn: &Connection,
    path: &str,
    account_hint: Option<&str>,
) -> Result<ImportResult> {
    let content = fs::read_to_string(path).with_context(|| format!("cannot read file: {path}"))?;
    let parsed = qif::parse(&content)?;

    // QIF files carry no account information — the caller must supply --account.
    let hint = account_hint.ok_or_else(|| {
        anyhow::anyhow!(
            "QIF files do not contain account information. \
             Specify the target account with --account."
        )
    })?;

    let account = db::find_account(conn, hint)?.ok_or_else(|| {
        anyhow::anyhow!("account not found: '{hint}'. Add it first with `fintrack account add`.")
    })?;

    insert_rows(
        conn,
        &parsed.rows,
        account.id,
        &account.name,
        &account.number,
    )
}

fn insert_rows(
    conn: &Connection,
    rows: &[format::ParsedRow],
    account_id: i64,
    account_name: &str,
    account_number: &str,
) -> Result<ImportResult> {
    let mut imported = 0usize;
    let mut skipped = 0usize;

    for row in rows {
        let date = parse_date(&row.date)?;
        let date_iso = date.format("%Y-%m-%d").to_string();
        let debit = parse_amount(&row.debit);
        let credit = parse_amount(&row.credit);

        let hash = make_hash(
            account_id, &date_iso, &row.code, &row.ref1, &row.ref2, &row.ref3, debit, credit,
        );

        // INSERT OR IGNORE — the UNIQUE constraint on `hash` silently discards duplicates.
        let n = conn.execute(
            "INSERT OR IGNORE INTO transactions \
             (account_id, date, code, description, ref1, ref2, ref3, status, debit, credit, hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                account_id,
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
        account_name: account_name.to_string(),
        account_number: account_number.to_string(),
        imported,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open a fresh SQLite database in a temp directory.
    /// The returned `TempDir` must stay alive for the duration of the test;
    /// dropping it deletes the directory and the database inside it.
    fn tmp_conn() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let conn = db::open(path.to_str().unwrap()).unwrap();
        (dir, conn)
    }

    // ── 9-column savings/current ──────────────────────────────────────────────

    #[test]
    fn import_9col_creates_account_and_transactions() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");

        let result = import_csv(&conn, path, "dbs", None, "DBS", "SGD").unwrap();

        assert_eq!(result.account_number, "000-11111-1");
        assert_eq!(result.account_name, "Test Savings");
        assert_eq!(result.imported, 4);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn import_9col_date_and_fields_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");
        import_csv(&conn, path, "dbs", None, "DBS", "SGD").unwrap();

        let (date, ref1, debit, credit): (String, String, Option<f64>, Option<f64>) = conn
            .query_row(
                "SELECT date, ref1, debit, credit FROM transactions WHERE code = 'SAL'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();

        assert_eq!(date, "2024-12-15"); // "15 Dec 2024" → ISO-8601
        assert_eq!(ref1, "EMPLOYER CO");
        assert_eq!(debit, None);
        assert_eq!(credit, Some(3500.0));
    }

    // ── 8-column credit card ──────────────────────────────────────────────────

    #[test]
    fn import_cc_creates_account_and_transactions() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_cc.csv");

        let result = import_csv(&conn, path, "dbs", None, "DBS", "SGD").unwrap();

        assert_eq!(result.account_number, "0000-1111-2222-3333");
        assert_eq!(result.account_name, "DBS Test Card");
        assert_eq!(result.imported, 4);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn import_cc_autopay_credit_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_cc.csv");
        import_csv(&conn, path, "dbs", None, "DBS", "SGD").unwrap();

        let (credit, ref1): (Option<f64>, String) = conn
            .query_row(
                "SELECT credit, ref1 FROM transactions WHERE description LIKE 'AUTOPAY%'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(credit, Some(450.25));
        assert_eq!(ref1, "PAYMENT"); // Transaction Type maps to ref1
    }

    // ── 12-column statement code format ──────────────────────────────────────

    #[test]
    fn import_12col_creates_account_and_transactions() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_12col.csv");

        let result = import_csv(&conn, path, "dbs", None, "DBS", "SGD").unwrap();

        assert_eq!(result.account_number, "000-33333-3");
        assert_eq!(result.account_name, "Test Multiplier");
        assert_eq!(result.imported, 3);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn import_12col_fields_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_12col.csv");
        import_csv(&conn, path, "dbs", None, "DBS", "SGD").unwrap();

        let (code, description, ref3): (String, String, String) = conn
            .query_row(
                "SELECT code, description, ref3 FROM transactions WHERE code = 'SAL'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        assert_eq!(code, "SAL");
        assert_eq!(description, "EMPLOYER CO PAYROLL DEC2024");
        assert_eq!(ref3, "REF001"); // Client Reference
    }

    // ── Cross-cutting: deduplication and auto-account creation ────────────────

    #[test]
    fn import_dedup_skips_on_reimport() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");

        let first = import_csv(&conn, path, "dbs", None, "DBS", "SGD").unwrap();
        assert_eq!(first.imported, 4);
        assert_eq!(first.skipped, 0);

        let second = import_csv(&conn, path, "dbs", None, "DBS", "SGD").unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 4);
    }

    #[test]
    fn import_auto_creates_account_from_csv_metadata() {
        let (_dir, conn) = tmp_conn();
        assert!(db::list_accounts(&conn).unwrap().is_empty());

        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");
        import_csv(&conn, path, "dbs", None, "DBS", "SGD").unwrap();

        let accounts = db::list_accounts(&conn).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].number, "000-11111-1");
        assert_eq!(accounts[0].name, "Test Savings");
        assert_eq!(accounts[0].bank, "DBS");
        assert_eq!(accounts[0].currency, "SGD");
    }

    // ── QIF credit card ───────────────────────────────────────────────────────

    #[test]
    fn import_qif_requires_account_hint() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");
        let err = import_qif(&conn, path, None).unwrap_err();
        assert!(err.to_string().contains("--account"));
    }

    #[test]
    fn import_qif_errors_if_account_not_found() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");
        let err = import_qif(&conn, path, Some("no-such-account")).unwrap_err();
        assert!(err.to_string().contains("account not found"));
    }

    #[test]
    fn import_qif_imports_into_existing_account() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");

        db::add_account(&conn, "My Card", "541", "DBS", "SGD").unwrap();
        let result = import_qif(&conn, path, Some("541")).unwrap();

        assert_eq!(result.account_number, "541");
        assert_eq!(result.imported, 4);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn import_qif_debit_and_credit_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");
        db::add_account(&conn, "My Card", "541", "DBS", "SGD").unwrap();
        import_qif(&conn, path, Some("541")).unwrap();

        // Debit transaction (T-39.00)
        let (debit, credit, description): (Option<f64>, Option<f64>, String) = conn
            .query_row(
                "SELECT debit, credit, description FROM transactions \
                 WHERE description = 'SASCO SENIOR CITIZENS SINGAPORE SG' \
                 ORDER BY date DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(debit, Some(39.0));
        assert_eq!(credit, None);
        assert!(!description.is_empty());

        // Credit transaction (T39.00)
        let (debit, credit): (Option<f64>, Option<f64>) = conn
            .query_row(
                "SELECT debit, credit FROM transactions WHERE description = 'INBOUND FT PYMT' LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(debit, None);
        assert_eq!(credit, Some(39.0));
    }

    // ── Amex ──────────────────────────────────────────────────────────────────

    #[test]
    fn import_amex_requires_account_hint() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/amex.csv");
        let err = import_csv(&conn, path, "amex", None, "Amex", "SGD").unwrap_err();
        assert!(err.to_string().contains("--account"));
    }

    #[test]
    fn import_amex_debit_and_credit_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/amex.csv");
        db::add_account(&conn, "Amex Platinum", "378282246310005", "Amex", "SGD").unwrap();
        import_csv(&conn, path, "amex", Some("378282246310005"), "Amex", "SGD").unwrap();

        // Charge: positive amount → debit
        let (debit, credit, date): (Option<f64>, Option<f64>, String) = conn
            .query_row(
                "SELECT debit, credit, date FROM transactions \
                 WHERE description LIKE 'SINGAPOREAIR%'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(debit, Some(6123.80));
        assert_eq!(credit, None);
        assert_eq!(date, "2026-04-11"); // MM/DD/YYYY → ISO-8601

        // Payment: negative amount → credit
        let (debit, credit): (Option<f64>, Option<f64>) = conn
            .query_row(
                "SELECT debit, credit FROM transactions \
                 WHERE description LIKE 'AMT DEBITED%'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(debit, None);
        assert_eq!(credit, Some(606.01));
    }

    #[test]
    fn import_amex_dedup_skips_on_reimport() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/amex.csv");
        db::add_account(&conn, "Amex Platinum", "378282246310005", "Amex", "SGD").unwrap();

        let first =
            import_csv(&conn, path, "amex", Some("378282246310005"), "Amex", "SGD").unwrap();
        assert_eq!(first.imported, 3);
        assert_eq!(first.skipped, 0);

        let second =
            import_csv(&conn, path, "amex", Some("378282246310005"), "Amex", "SGD").unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 3);
    }

    #[test]
    fn import_qif_dedup_skips_on_reimport() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");
        db::add_account(&conn, "My Card", "541", "DBS", "SGD").unwrap();

        let first = import_qif(&conn, path, Some("541")).unwrap();
        assert_eq!(first.imported, 4);
        assert_eq!(first.skipped, 0);

        let second = import_qif(&conn, path, Some("541")).unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 4);
    }
}
