use crate::models::{Account, Transaction};
use anyhow::Result;
use rusqlite::Connection;

mod csv;
mod qif;
mod specs;

#[derive(Debug)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
}

/// Imports transactions from a CSV file using the ReaderSpec.
pub fn import_csv<P: AsRef<std::path::Path>>(
    conn: &Connection,
    account: &Account,
    path: P,
) -> Result<ImportResult> {
    let transactions = csv::parse(&path)?
        .into_iter()
        .filter_map(|mut builder| {
            let res = builder.account_id(account.id).build();
            if let Err(e) = &res {
                eprintln!(
                    "Warning: failed to build transaction from CSV row, error: {:?}",
                    e
                );
            }
            res.ok()
        })
        .collect();

    insert_transactions(conn, transactions)
}

pub fn import_qif<P: AsRef<std::path::Path>>(
    conn: &Connection,
    path: P,
    account: &Account,
) -> Result<ImportResult> {
    let transactions = qif::parse(path)?
        .into_iter()
        .filter_map(|mut builder| {
            let res = builder.account_id(account.id).build();
            if let Err(e) = &res {
                eprintln!(
                    "Warning: failed to build transaction from QIF row, error: {:?}",
                    e
                );
            }
            res.ok()
        })
        .collect();
    insert_transactions(conn, transactions)
}

fn insert_transactions(conn: &Connection, transactions: Vec<Transaction>) -> Result<ImportResult> {
    let mut imported = 0usize;
    let mut skipped = 0usize;

    for t in transactions {
        let params = serde_rusqlite::to_params_named(&t)?;
        let n = conn.execute(
            "INSERT OR IGNORE INTO transactions \
             (account_id, date, code, description, ref1, ref2, ref3, status, debit, credit, hash) \
             VALUES (:account_id, :date, :code, :description, :ref1, :ref2, :ref3, :status, :debit, :credit, :hash)",
             params.to_slice().as_slice(),
        )?;

        if n == 1 {
            imported += 1;
        } else {
            skipped += 1;
        }
    }

    Ok(ImportResult { imported, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::test_util::create_account;

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
    fn import_9col_creates_transactions_using_import() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");

        let account = create_account(&conn, "541", "My Card").unwrap();

        let result = import_csv(&conn, &account, path).unwrap();

        assert_eq!(result.imported, 4);
        assert_eq!(result.skipped, 0);

        let (credit, ref1): (Option<f64>, String) = conn
            .query_row(
                "SELECT credit, ref1 FROM transactions WHERE description LIKE 'EMPLOYER CO%'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(credit, Some(3500.0));
        assert_eq!(ref1, "EMPLOYER CO");
    }

    #[test]
    fn import_9col_date_and_fields_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");
        let account = create_account(&conn, "000-11111-1", "Test Savings").unwrap();
        import_csv(&conn, &account, path).unwrap();

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
        let account = create_account(&conn, "0000-1111-2222-3333", "DBS Test Card").unwrap();

        let result = import_csv(&conn, &account, path).unwrap();

        assert_eq!(result.imported, 4);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn import_cc_autopay_credit_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_cc.csv");
        let account = create_account(&conn, "0000-1111-2222-3333", "DBS Test Card").unwrap();
        import_csv(&conn, &account, path).unwrap();

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
        let account = create_account(&conn, "000-33333-3", "Test Multiplier").unwrap();

        let result = import_csv(&conn, &account, path).unwrap();

        assert_eq!(result.imported, 3);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn import_12col_fields_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_12col.csv");
        let account = create_account(&conn, "000-33333-3", "Test Multiplier").unwrap();
        import_csv(&conn, &account, path).unwrap();

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
        let account = create_account(&conn, "000-11111-1", "Test Savings").unwrap();

        let first = import_csv(&conn, &account, path).unwrap();
        assert_eq!(first.imported, 4);
        assert_eq!(first.skipped, 0);

        let second = import_csv(&conn, &account, path).unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 4);
    }

    // ── QIF credit card ───────────────────────────────────────────────────────

    #[test]
    fn import_qif_imports_into_existing_account() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");

        let account = create_account(&conn, "541", "My Card").unwrap();
        let result = import_qif(&conn, path, &account).unwrap();

        assert_eq!(result.imported, 4);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn import_qif_debit_and_credit_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");
        let account = create_account(&conn, "541", "My Card").unwrap();
        import_qif(&conn, path, &account).unwrap();

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
    fn import_amex_debit_and_credit_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/amex.csv");
        let account = create_account(&conn, "378282246310005", "Amex Platinum").unwrap();
        let result = import_csv(&conn, &account, path).unwrap();
        assert_eq!(result.imported, 3);

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
        let account = create_account(&conn, "378282246310005", "Amex Platinum").unwrap();

        let first = import_csv(&conn, &account, path).unwrap();
        assert_eq!(first.imported, 3);
        assert_eq!(first.skipped, 0);

        let second = import_csv(&conn, &account, path).unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 3);
    }

    #[test]
    fn import_qif_dedup_skips_on_reimport() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");
        let account = create_account(&conn, "541", "My Card").unwrap();

        let first = import_qif(&conn, path, &account).unwrap();
        assert_eq!(first.imported, 4);
        assert_eq!(first.skipped, 0);

        let second = import_qif(&conn, path, &account).unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 4);
    }
}
