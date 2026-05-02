use crate::{
    db::find_account,
    models::{Account, Transaction, TransactionBuilder},
};
use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use rusqlite::Connection;
use rusty_money::{Money, iso};

mod csv;
pub mod pdf_text;
mod pdf_youtrip;
mod qif;
pub mod specs;

#[derive(Debug)]
pub struct ImportResult {
    pub account: Account,
    pub imported: usize,
    pub skipped: usize,
}

/// Imports transactions from a CSV file using the ReaderSpec.
pub fn import_csv<P: AsRef<std::path::Path>>(
    conn: &Connection,
    path: P,
    account_number: Option<String>,
) -> Result<ImportResult> {
    let data = csv::parse(path)?;

    let account = match (account_number, data.account_number) {
        (Some(num), Some(file_num)) if num != file_num => {
            anyhow::bail!(
                "account number mismatch: file has '{}', argument is '{}'",
                file_num,
                num
            );
        }
        (Some(num), _) => {
            find_account(conn, &num)?.ok_or_else(|| anyhow!("Account not found: '{num}'"))?
        }
        (None, Some(file_num)) => find_account(conn, &file_num)?
            .ok_or_else(|| anyhow!("Account not found: '{file_num}'"))?,
        (None, None) => {
            anyhow::bail!(
                "Account number must be specified either as a command-line argument or in the file"
            );
        }
    };

    if let Some(file_currency) = data.currency {
        if account.currency != file_currency {
            anyhow::bail!(
                "currency mismatch: file has '{}', but account '{}' has currency '{}'",
                file_currency,
                account.name,
                account.currency
            );
        }
    }

    let currency = account.iso_currency()
        .ok_or_else(|| anyhow!("unknown currency: '{}'", account.currency))?;

    let mut importer = Importer::new(conn, account.clone());
    for row in data.rows {
        match row_into_builder(row, currency) {
            Ok(mut builder) => match builder.account_id(account.id).build() {
                Ok(t) => importer.insert(t)?,
                Err(e) => eprintln!("Warning: failed to build transaction from CSV row: {:?}", e),
            },
            Err(e) => eprintln!("Warning: failed to parse CSV row: {:?}", e),
        }
    }
    Ok(importer.finish())
}

pub fn import_pdf_youtrip<P: AsRef<std::path::Path>>(
    conn: &Connection,
    path: P,
    account: &Account,
) -> Result<ImportResult> {
    let currency = account
        .iso_currency()
        .ok_or_else(|| anyhow!("unknown currency: '{}'", account.currency))?;
    let mut importer = Importer::new(conn, account.clone());
    for mut builder in pdf_youtrip::parse(path, currency)? {
        match builder.account_id(account.id).build() {
            Ok(t) => importer.insert(t)?,
            Err(e) => eprintln!("Warning: failed to build transaction from PDF row: {:?}", e),
        }
    }
    Ok(importer.finish())
}

pub fn import_qif<P: AsRef<std::path::Path>>(
    conn: &Connection,
    path: P,
    account: &Account,
) -> Result<ImportResult> {
    let mut importer = Importer::new(conn, account.clone());
    for mut builder in qif::parse(path)? {
        match builder.account_id(account.id).build() {
            Ok(t) => importer.insert(t)?,
            Err(e) => eprintln!(
                "Warning: failed to build transaction from QIF row, error: {:?}",
                e
            ),
        }
    }
    Ok(importer.finish())
}

fn row_into_builder(row: csv::Row, currency: &iso::Currency) -> Result<TransactionBuilder> {
    let mut builder = TransactionBuilder::default();
    for (field, value) in row {
        match field {
            csv::Field::Date { date_format } => {
                builder.date(
                    NaiveDate::parse_from_str(&value, &date_format)
                        .with_context(|| format!("failed to parse date: '{}'", value))?,
                );
            }
            csv::Field::Debit => {
                builder.debit(parse_amount(&value, currency)?);
            }
            csv::Field::Credit => {
                builder.credit(parse_amount(&value, currency)?);
            }
            csv::Field::Amount { invert } => {
                if let Some(n) = parse_amount(&value, currency)? {
                    builder.amount(if invert.unwrap_or(false) { -n } else { n });
                }
            }
            csv::Field::Code => { builder.code(value); }
            csv::Field::Description => { builder.description(value); }
            csv::Field::Ref1 => { builder.ref1(value); }
            csv::Field::Ref2 => { builder.ref2(value); }
            csv::Field::Ref3 => { builder.ref3(value); }
            csv::Field::Status => { builder.status(value); }
        }
    }
    Ok(builder)
}

fn parse_amount(s: &str, currency: &iso::Currency) -> Result<Option<i64>> {
    if s.trim().is_empty() {
        return Ok(None);
    }
    Money::from_str(s.trim(), currency)
        .map(|m| Some(m.to_minor_units()))
        .map_err(|e| anyhow!("failed to parse amount '{}': {:?}", s, e))
}

struct Importer<'a> {
    conn: &'a Connection,
    account: Account,
    imported: usize,
    skipped: usize,
}

impl<'a> Importer<'a> {
    fn new(conn: &'a Connection, account: Account) -> Self {
        Self {
            conn,
            account,
            imported: 0,
            skipped: 0,
        }
    }

    fn insert(&mut self, t: Transaction) -> Result<()> {
        let params = serde_rusqlite::to_params_named(&t)?;
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO transactions \
             (account_id, date, code, description, ref1, ref2, ref3, status, debit, credit, hash) \
             VALUES (:account_id, :date, :code, :description, :ref1, :ref2, :ref3, :status, :debit, :credit, :hash)",
            params.to_slice().as_slice(),
        )?;
        if n == 1 {
            self.imported += 1;
        } else {
            self.skipped += 1;
        }
        Ok(())
    }

    fn finish(self) -> ImportResult {
        ImportResult {
            account: self.account,
            imported: self.imported,
            skipped: self.skipped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::test_util::{create_account, create_account_with_currency};

    fn tmp_conn() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let conn = db::open(path.to_str().unwrap()).unwrap();
        (dir, conn)
    }

    fn csv(conn: &Connection, path: &str, number: &str) -> Result<ImportResult> {
        import_csv(conn, path, Some(number.to_string()))
    }

    // ── 9-column savings/current ──────────────────────────────────────────────

    #[test]
    fn import_9col_creates_transactions_using_import() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");
        create_account(&conn, "000-11111-1", "Test Savings").unwrap();

        let result = csv(&conn, path, "000-11111-1").unwrap();

        assert_eq!(result.imported, 4);
        assert_eq!(result.skipped, 0);

        let (credit, ref1): (Option<i64>, String) = conn
            .query_row(
                "SELECT credit, ref1 FROM transactions WHERE description LIKE 'EMPLOYER CO%'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(credit, Some(350000));
        assert_eq!(ref1, "EMPLOYER CO");
    }

    #[test]
    fn import_9col_date_and_fields_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");
        create_account(&conn, "000-11111-1", "Test Savings").unwrap();
        csv(&conn, path, "000-11111-1").unwrap();

        let (date, ref1, debit, credit): (String, String, Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT date, ref1, debit, credit FROM transactions WHERE code = 'SAL'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();

        assert_eq!(date, "2024-12-15");
        assert_eq!(ref1, "EMPLOYER CO");
        assert_eq!(debit, None);
        assert_eq!(credit, Some(350000));
    }

    // ── 8-column credit card ──────────────────────────────────────────────────

    #[test]
    fn import_cc_creates_account_and_transactions() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_cc.csv");
        create_account(&conn, "0000-1111-2222-3333", "DBS Test Card").unwrap();

        let result = csv(&conn, path, "0000-1111-2222-3333").unwrap();

        assert_eq!(result.imported, 4);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn import_cc_autopay_credit_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_cc.csv");
        create_account(&conn, "0000-1111-2222-3333", "DBS Test Card").unwrap();
        csv(&conn, path, "0000-1111-2222-3333").unwrap();

        let (credit, ref1): (Option<i64>, String) = conn
            .query_row(
                "SELECT credit, ref1 FROM transactions WHERE description LIKE 'AUTOPAY%'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(credit, Some(45025));
        assert_eq!(ref1, "PAYMENT");
    }

    // ── 12-column statement code format ──────────────────────────────────────

    #[test]
    fn import_12col_creates_account_and_transactions() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_12col.csv");
        create_account(&conn, "000-33333-3", "Test Multiplier").unwrap();

        let result = csv(&conn, path, "000-33333-3").unwrap();

        assert_eq!(result.imported, 3);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn import_12col_fields_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_12col.csv");
        create_account(&conn, "000-33333-3", "Test Multiplier").unwrap();
        csv(&conn, path, "000-33333-3").unwrap();

        let (code, description, ref3): (String, String, String) = conn
            .query_row(
                "SELECT code, description, ref3 FROM transactions WHERE code = 'SAL'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        assert_eq!(code, "SAL");
        assert_eq!(description, "EMPLOYER CO PAYROLL DEC2024");
        assert_eq!(ref3, "REF001");
    }

    // ── Cross-cutting: deduplication ─────────────────────────────────────────

    #[test]
    fn import_dedup_skips_on_reimport() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");
        create_account(&conn, "000-11111-1", "Test Savings").unwrap();

        let first = csv(&conn, path, "000-11111-1").unwrap();
        assert_eq!(first.imported, 4);
        assert_eq!(first.skipped, 0);

        let second = csv(&conn, path, "000-11111-1").unwrap();
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

        let (debit, credit, description): (Option<i64>, Option<i64>, String) = conn
            .query_row(
                "SELECT debit, credit, description FROM transactions \
                 WHERE description = 'SASCO SENIOR CITIZENS SINGAPORE SG' \
                 ORDER BY date DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(debit, Some(3900));
        assert_eq!(credit, None);
        assert!(!description.is_empty());

        let (debit, credit): (Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT debit, credit FROM transactions WHERE description = 'INBOUND FT PYMT' LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(debit, None);
        assert_eq!(credit, Some(3900));
    }

    // ── Amex ──────────────────────────────────────────────────────────────────

    #[test]
    fn import_amex_debit_and_credit_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/amex.csv");
        create_account(&conn, "378282246310005", "Amex Platinum").unwrap();

        let result = csv(&conn, path, "378282246310005").unwrap();
        assert_eq!(result.imported, 3);

        let (debit, credit, date): (Option<i64>, Option<i64>, String) = conn
            .query_row(
                "SELECT debit, credit, date FROM transactions \
                 WHERE description LIKE 'SINGAPOREAIR%'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(debit, Some(612380));
        assert_eq!(credit, None);
        assert_eq!(date, "2026-04-11");

        let (debit, credit): (Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT debit, credit FROM transactions \
                 WHERE description LIKE 'AMT DEBITED%'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(debit, None);
        assert_eq!(credit, Some(60601));
    }

    #[test]
    fn import_amex_dedup_skips_on_reimport() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/amex.csv");
        create_account(&conn, "378282246310005", "Amex Platinum").unwrap();

        let first = csv(&conn, path, "378282246310005").unwrap();
        assert_eq!(first.imported, 3);
        assert_eq!(first.skipped, 0);

        let second = csv(&conn, path, "378282246310005").unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 3);
    }

    // ── Conflict checks ───────────────────────────────────────────────────────

    #[test]
    fn import_csv_fails_on_account_number_mismatch() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");
        // file has account number 000-11111-1; pass a different number
        let err = import_csv(&conn, path, Some("wrong-number".to_string())).unwrap_err();
        assert!(err.to_string().contains("account number mismatch"), "{err}");
    }

    #[test]
    fn import_csv_fails_on_currency_mismatch() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");
        // file has currency SGD; account has USD
        create_account_with_currency(&conn, "000-11111-1", "Test", "USD").unwrap();
        let err = import_csv(&conn, path, Some("000-11111-1".to_string())).unwrap_err();
        assert!(err.to_string().contains("currency mismatch"), "{err}");
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
