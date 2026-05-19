use std::{fs, path::Path};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use qif_parser::transaction::QifTransaction;
use rusty_money::iso;

use crate::{
    import::{AsTransactionBuilder, ParseData},
    models::TransactionBuilder,
};

const QIF_INPUT_DATE_FORMAT: &str = "%d/%m/%Y";
const QIF_OUTPUT_DATE_FORMAT: &str = "%Y-%m-%d";

struct QifRecord {
    date: NaiveDate,
    description: String,
    code: String,
    ref1: String,
    status: String,
    amount: i64,
}

impl AsTransactionBuilder for QifRecord {
    fn as_transaction_builder(
        &self,
        _currency: &'static iso::Currency,
    ) -> Result<TransactionBuilder> {
        let mut builder = TransactionBuilder::default();
        builder
            .amount(self.amount)
            .date(self.date)
            .code(self.code.clone())
            .description(self.description.clone())
            .ref1(self.ref1.clone())
            .status(self.status.clone());
        Ok(builder)
    }
}

pub(super) fn parse<P: AsRef<Path>>(path: P) -> Result<ParseData> {
    let content = fs::read_to_string(path.as_ref())
        .with_context(|| format!("cannot read file: {}", path.as_ref().to_string_lossy()))?;
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
    let qif = qif_parser::parse(content, QIF_INPUT_DATE_FORMAT)
        .map_err(|e| anyhow::anyhow!("QIF parse error: {e}"))?;
    let mut data = ParseData::default();
    for tx in qif.transactions {
        data.records.push(Box::new(into_record(tx)));
    }
    Ok(data)
}

fn into_record(tx: QifTransaction) -> QifRecord {
    let description = tx.payee.split_whitespace().collect::<Vec<_>>().join(" ");
    // Safe to unwrap because qif_parser has already validated the date format
    let date = NaiveDate::parse_from_str(&tx.date, QIF_OUTPUT_DATE_FORMAT)
        .with_context(|| format!("failed to parse date: {}", tx.date))
        .unwrap();
    QifRecord {
        date,
        description,
        code: tx.number_of_the_check.to_string(),
        ref1: tx.memo.to_string(),
        status: tx.cleared_status.to_string(),
        amount: (tx.amount * 100.0).round() as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/qif_ccard.qif");

    fn build(record: &dyn AsTransactionBuilder) -> crate::models::Transaction {
        record
            .as_transaction_builder(&iso::SGD)
            .unwrap()
            .account_id(1)
            .build()
            .unwrap()
    }

    #[test]
    fn parse_row_count() {
        let parsed = parse(FIXTURE_PATH).unwrap();
        assert_eq!(parsed.records.len(), 4);
    }

    #[test]
    fn parse_debit_transaction() {
        let parsed = parse(FIXTURE_PATH).unwrap();
        let tx = build(parsed.records[0].as_ref());
        assert_eq!(tx.date.to_string(), "2026-04-02");
        assert_eq!(tx.description, "SASCO SENIOR CITIZENS SINGAPORE SG");
        assert_eq!(tx.debit, Some(3900));
        assert_eq!(tx.credit, None);
        assert_eq!(tx.status, "*");
    }

    #[test]
    fn parse_credit_transaction() {
        let parsed = parse(FIXTURE_PATH).unwrap();
        let tx = build(parsed.records[1].as_ref());
        assert_eq!(tx.date.to_string(), "2026-03-26");
        assert_eq!(tx.description, "INBOUND FT PYMT");
        assert_eq!(tx.debit, None);
        assert_eq!(tx.credit, Some(3900));
    }

    #[test]
    fn parse_memo_and_number_fields() {
        let parsed = parse(FIXTURE_PATH).unwrap();
        let tx = build(parsed.records[2].as_ref());
        assert_eq!(tx.code, "REF001");
        assert_eq!(tx.ref1, "Insurance payment");
    }

    #[test]
    fn parse_amount_with_comma_separator() {
        let parsed = parse(FIXTURE_PATH).unwrap();
        let tx = build(parsed.records[3].as_ref());
        assert_eq!(tx.debit, Some(123456));
    }
}
