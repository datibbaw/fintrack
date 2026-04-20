use std::{fs, path::Path};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use qif_parser::transaction::QifTransaction;

use crate::models::TransactionBuilder;

const QIF_INPUT_DATE_FORMAT: &str = "%d/%m/%Y";
const QIF_OUTPUT_DATE_FORMAT: &str = "%Y-%m-%d";

pub(super) fn parse<P: AsRef<Path>>(path: P) -> Result<Vec<TransactionBuilder>> {
    let content = fs::read_to_string(path.as_ref())
        .with_context(|| format!("cannot read file: {}", path.as_ref().to_string_lossy()))?;
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
    let qif = qif_parser::parse(content, QIF_INPUT_DATE_FORMAT)
        .map_err(|e| anyhow::anyhow!("QIF parse error: {e}"))?;
    Ok(qif.transactions.into_iter().map(into_builder).collect())
}

fn into_builder(tx: QifTransaction) -> TransactionBuilder {
    let mut builder = TransactionBuilder::default();

    // Collapse runs of whitespace in the payee string (DBS pads merchant
    // names to fixed-width columns).
    let description = tx.payee.split_whitespace().collect::<Vec<_>>().join(" ");
    // Safe to unwrap because qif_parser has already validated it
    let date = NaiveDate::parse_from_str(&tx.date, QIF_OUTPUT_DATE_FORMAT)
        .with_context(|| format!("failed to parse date: {}", tx.date))
        .unwrap();

    builder
        .amount(tx.amount)
        .date(date)
        .code(tx.number_of_the_check.to_string())
        .description(description)
        .ref1(tx.memo.to_string())
        .status(tx.cleared_status.to_string());

    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_PATH: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");

    #[test]
    fn parse_row_count() {
        let parsed = parse(FIXTURE_PATH).unwrap();
        assert_eq!(parsed.len(), 4);
    }

    #[test]
    fn parse_debit_transaction() {
        let mut parsed = parse(FIXTURE_PATH).unwrap();
        // First row: T-39.00 → debit
        let tx = parsed.remove(0).account_id(1).build().unwrap();
        assert_eq!(tx.date.to_string(), "2026-04-02");
        assert_eq!(tx.description, "SASCO SENIOR CITIZENS SINGAPORE SG");
        assert_eq!(tx.debit, Some(39.00));
        assert_eq!(tx.credit, None);
        assert_eq!(tx.status, "*");
    }

    #[test]
    fn parse_credit_transaction() {
        let mut parsed = parse(FIXTURE_PATH).unwrap();
        // Second row: T39.00 → credit (inbound payment)
        let tx = parsed.remove(1).account_id(1).build().unwrap();
        assert_eq!(tx.date.to_string(), "2026-03-26");
        assert_eq!(tx.description, "INBOUND FT PYMT");
        assert_eq!(tx.debit, None);
        assert_eq!(tx.credit, Some(39.00));
    }

    #[test]
    fn parse_memo_and_number_fields() {
        let mut parsed = parse(FIXTURE_PATH).unwrap();
        // Third row has a memo and number
        let tx = parsed.remove(2).account_id(1).build().unwrap();
        assert_eq!(tx.code, "REF001");
        assert_eq!(tx.ref1, "Insurance payment");
    }

    #[test]
    fn parse_amount_with_comma_separator() {
        let mut parsed = parse(FIXTURE_PATH).unwrap();
        // Fourth row: T-1,234.56
        let tx = parsed.remove(3).account_id(1).build().unwrap();
        assert_eq!(tx.debit, Some(1234.56));
    }
}
