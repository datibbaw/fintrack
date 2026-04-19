use anyhow::{Context, Result};
use chrono::NaiveDate;
use qif_parser::transaction::QifTransaction;

use crate::models::TransactionBuilder;

const QIF_OUTPUT_DATE_FORMAT: &str = "%Y-%m-%d";

pub fn into_builder(tx: QifTransaction) -> TransactionBuilder {
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

pub fn parse<'a>(content: &'a str, date_format: &str) -> Result<Vec<QifTransaction<'a>>> {
    // Strip a leading UTF-8 BOM if present
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(content);

    let qif = qif_parser::parse(content, date_format)
        .map_err(|e| anyhow::anyhow!("QIF parse error: {e}"))?;

    Ok(qif.transactions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::QIF_INPUT_DATE_FORMAT;

    const FIXTURE: &str = include_str!("../tests/fixtures/qif_ccard.qif");

    #[test]
    fn parse_row_count() {
        let parsed = parse(FIXTURE, QIF_INPUT_DATE_FORMAT).unwrap();
        assert_eq!(parsed.len(), 4);
    }

    #[test]
    fn parse_debit_transaction() {
        let mut parsed = parse(FIXTURE, QIF_INPUT_DATE_FORMAT).unwrap();
        // First row: T-39.00 → debit
        let row = parsed.remove(0);
        assert_eq!(row.date, "2026-04-02");

        let transaction = into_builder(row).account_id(1).build().unwrap();
        assert_eq!(
            transaction.description,
            "SASCO SENIOR CITIZENS SINGAPORE SG"
        );
        assert_eq!(transaction.debit, Some(39.00));
        assert_eq!(transaction.credit, None);
        assert_eq!(transaction.status, "*");
    }

    #[test]
    fn parse_credit_transaction() {
        let mut parsed = parse(FIXTURE, QIF_INPUT_DATE_FORMAT).unwrap();
        // Second row: T39.00 → credit (inbound payment)
        let row = parsed.remove(1);
        assert_eq!(row.date, "2026-03-26");

        let transaction = into_builder(row).account_id(1).build().unwrap();
        assert_eq!(transaction.description, "INBOUND FT PYMT");
        assert_eq!(transaction.debit, None);
        assert_eq!(transaction.credit, Some(39.00));
    }

    #[test]
    fn parse_memo_and_number_fields() {
        let mut parsed = parse(FIXTURE, QIF_INPUT_DATE_FORMAT).unwrap();
        // Third row has a memo and number
        let row = parsed.remove(2);
        let transaction = into_builder(row).account_id(1).build().unwrap();
        assert_eq!(transaction.code, "REF001");
        assert_eq!(transaction.ref1, "Insurance payment");
    }

    #[test]
    fn parse_amount_with_comma_separator() {
        let mut parsed = parse(FIXTURE, QIF_INPUT_DATE_FORMAT).unwrap();
        // Fourth row: T-1,234.56
        let row = parsed.remove(3);
        let transaction = into_builder(row).account_id(1).build().unwrap();
        assert_eq!(transaction.debit, Some(1234.56));
    }
}
