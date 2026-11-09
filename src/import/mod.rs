use crate::{
    db::find_account,
    models::{Account, Transaction, TransactionBuilder},
    money::HasCurrency,
};
use anyhow::{Context, Result, anyhow, bail};
use rusqlite::Connection;
use rusty_money::{Money, iso};
use std::path::Path;

mod csv;
mod pdf_citibank;
mod pdf_youtrip;
mod qif;
pub mod specs;

#[derive(Debug)]
pub struct ImportResult {
    pub account: Account,
    pub imported: usize,
    pub skipped: usize,
    pub errored: usize,
}

#[derive(Default)]
pub(crate) struct ParseData {
    pub account_number: Option<String>,
    pub currency: Option<String>,
    pub records: Vec<Box<dyn AsTransactionBuilder>>,
}

pub(crate) trait AsTransactionBuilder {
    fn as_transaction_builder(
        &self,
        currency: &'static iso::Currency,
    ) -> Result<TransactionBuilder>;
}

/// Selects which parser reads an import file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    /// Choose by file type only: `.qif` → QIF, `.pdf` → first PDF parser that
    /// recognises the statement, anything else → CSV (spec auto-detection).
    #[default]
    Auto,
    Csv,
    Qif,
    PdfCitibank,
    PdfYoutrip,
}

type PdfParser = fn(&str) -> Result<ParseData>;

/// PDF parsers tried in order by [`Format::Auto`]. Stricter parsers (those that
/// fail fast on unrecognised statements) should come first.
const PDF_PARSERS: &[(&str, PdfParser)] = &[
    ("citibank", pdf_citibank::parse),
    ("youtrip", pdf_youtrip::parse),
];

/// Parses `path` with the given format and imports the transactions.
pub fn import_file<P: AsRef<Path>>(
    conn: &Connection,
    path: P,
    format: Format,
    account_number: Option<String>,
) -> Result<ImportResult> {
    let data = parse_file(path.as_ref(), format)?;
    Importer::new(conn, account_number).import_data(data)
}

fn parse_file(path: &Path, format: Format) -> Result<ParseData> {
    match format {
        Format::Auto => parse_auto(path),
        Format::Csv => csv::parse(path),
        Format::Qif => qif::parse(path),
        Format::PdfCitibank => pdf_citibank::parse(&pdf_text(path)?),
        Format::PdfYoutrip => pdf_youtrip::parse(&pdf_text(path)?),
    }
}

fn parse_auto(path: &Path) -> Result<ParseData> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("qif") => qif::parse(path),
        Some("pdf") => parse_pdf_auto(&pdf_text(path)?),
        _ => csv::parse(path),
    }
}

fn pdf_text(path: &Path) -> Result<String> {
    pdf_extract::extract_text(path)
        .with_context(|| format!("cannot extract text from PDF: {}", path.display()))
}

/// Tries each PDF parser in turn; the first one that succeeds and yields at
/// least one transaction wins.
fn parse_pdf_auto(text: &str) -> Result<ParseData> {
    let mut failures = Vec::new();
    for (name, parse) in PDF_PARSERS {
        match parse(text) {
            Ok(data) if !data.records.is_empty() => return Ok(data),
            Ok(_) => failures.push(format!("{name}: no transactions found")),
            Err(e) => failures.push(format!("{name}: {e:#}")),
        }
    }
    bail!(
        "no PDF importer recognised the file ({})",
        failures.join("; ")
    )
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
    account_number: Option<String>,
}

impl<'a> Importer<'a> {
    fn new(conn: &'a Connection, account_number: Option<String>) -> Self {
        Self {
            conn,
            account_number,
        }
    }

    fn import_data(&self, data: ParseData) -> Result<ImportResult> {
        use InsertResult::*;

        let account = self.resolve_account(data.account_number)?;
        let currency = reconcile_currency(&account, data.currency.as_deref())?;

        let mut imported: usize = 0;
        let mut skipped: usize = 0;
        let mut errored: usize = 0;

        for record in &data.records {
            let transaction = record
                .as_transaction_builder(currency)?
                .account_id(account.id)
                .build()?;

            match self.insert(transaction) {
                Ok(Inserted) => imported += 1,
                Ok(Skipped) => skipped += 1,
                Err(e) => {
                    eprintln!("Warning: failed to insert transaction: {:?}", e);
                    errored += 1;
                }
            }
        }

        Ok(ImportResult {
            account,
            imported,
            skipped,
            errored,
        })
    }

    fn resolve_account(&self, account_number: Option<String>) -> Result<Account> {
        match (&self.account_number, &account_number) {
            (Some(given_num), Some(parsed_num)) if *given_num != *parsed_num => {
                anyhow::bail!(
                    "account number mismatch: file has '{parsed_num}', argument is '{given_num}'",
                );
            }
            (Some(given_num), _) => find_account(self.conn, given_num)?
                .ok_or_else(|| anyhow!("Account not found: '{given_num}'")),
            (None, Some(parsed_num)) => find_account(self.conn, parsed_num)?
                .ok_or_else(|| anyhow!("Account not found: '{parsed_num}'")),
            (None, None) => {
                anyhow::bail!(
                    "Account number must be specified either as a command-line argument or in the file"
                );
            }
        }
    }

    fn insert(&self, t: Transaction) -> Result<InsertResult> {
        use InsertResult::*;

        let params = serde_rusqlite::to_params_named(&t)?;
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO transactions \
             (account_id, date, code, description, ref1, ref2, ref3, status, debit, credit, hash) \
             VALUES (:account_id, :date, :code, :description, :ref1, :ref2, :ref3, :status, :debit, :credit, :hash)",
            params.to_slice().as_slice(),
        )?;

        if n == 1 { Ok(Inserted) } else { Ok(Skipped) }
    }
}

fn reconcile_currency(
    account: &Account,
    found_currency: Option<&str>,
) -> Result<&'static iso::Currency> {
    let currency = account.currency();

    match found_currency {
        Some(c) if c != currency.iso_alpha_code => {
            anyhow::bail!(
                "currency mismatch: file has '{c}', but account '{}' has currency '{}'",
                account.name,
                currency.iso_alpha_code
            );
        }
        _ => Ok(currency),
    }
}

enum InsertResult {
    Inserted,
    Skipped,
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
        import_file(conn, path, Format::Csv, Some(number.to_string()))
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
        assert_eq!(result.errored, 0);

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
        assert_eq!(result.errored, 0);
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
        assert_eq!(result.errored, 0);
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
        assert_eq!(first.errored, 0);

        let second = csv(&conn, path, "000-11111-1").unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 4);
        assert_eq!(second.errored, 0);
    }

    // ── QIF credit card ───────────────────────────────────────────────────────

    #[test]
    fn import_qif_imports_into_existing_account() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");
        create_account(&conn, "541", "My Card").unwrap();

        let result = import_file(&conn, path, Format::Qif, Some("541".to_string())).unwrap();

        assert_eq!(result.imported, 4);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.errored, 0);
    }

    #[test]
    fn import_qif_debit_and_credit_stored_correctly() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");
        create_account(&conn, "541", "My Card").unwrap();
        import_file(&conn, path, Format::Qif, Some("541".to_string())).unwrap();

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
        assert_eq!(result.errored, 0);

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
        assert_eq!(first.errored, 0);

        let second = csv(&conn, path, "378282246310005").unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 3);
        assert_eq!(second.errored, 0);
    }

    // ── Conflict checks ───────────────────────────────────────────────────────

    #[test]
    fn import_csv_fails_on_account_number_mismatch() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");
        // file has account number 000-11111-1; pass a different number
        let err =
            import_file(&conn, path, Format::Csv, Some("wrong-number".to_string())).unwrap_err();
        assert!(err.to_string().contains("account number mismatch"), "{err}");
    }

    #[test]
    fn import_csv_fails_on_currency_mismatch() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");
        // file has currency SGD; account has USD
        create_account_with_currency(&conn, "000-11111-1", "Test", "USD").unwrap();
        let err =
            import_file(&conn, path, Format::Csv, Some("000-11111-1".to_string())).unwrap_err();
        assert!(err.to_string().contains("currency mismatch"), "{err}");
    }

    // ── PDF auto-detection ────────────────────────────────────────────────────

    const CITIBANK_TEXT: &str = "\
Statement Date March 15, 2026
1234 5678 9012 3456 Payment Due Date: April 5, 2026
02 MAR COFFEE SHOP SINGAPORE 5.50
10 MAR PAYMENT RECEIVED (100.00)
";

    #[test]
    fn pdf_auto_picks_first_matching_parser() {
        let data = parse_pdf_auto(CITIBANK_TEXT).unwrap();
        assert_eq!(data.account_number.as_deref(), Some("1234 5678 9012 3456"));
        assert_eq!(data.records.len(), 2);
    }

    #[test]
    fn pdf_auto_reports_every_parser_when_none_match() {
        let Err(err) = parse_pdf_auto("nothing recognisable here") else {
            panic!("expected no parser to match");
        };
        let msg = err.to_string();
        assert!(msg.contains("no PDF importer recognised"), "{msg}");
        assert!(
            msg.contains("citibank: could not find statement date"),
            "{msg}"
        );
        assert!(msg.contains("youtrip: no transactions found"), "{msg}");
    }

    #[test]
    fn import_qif_dedup_skips_on_reimport() {
        let (_dir, conn) = tmp_conn();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");
        create_account(&conn, "541", "My Card").unwrap();

        let first = import_file(&conn, path, Format::Qif, Some("541".to_string())).unwrap();
        assert_eq!(first.imported, 4);
        assert_eq!(first.skipped, 0);
        assert_eq!(first.errored, 0);

        let second = import_file(&conn, path, Format::Qif, Some("541".to_string())).unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 4);
        assert_eq!(second.errored, 0);
    }
}
