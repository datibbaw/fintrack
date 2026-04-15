use anyhow::Result;

use crate::format::{ParsedCsv, ParsedRow};

/// Parse a QIF file using the `qif_parser` crate and convert the result into
/// the shared `ParsedCsv` structure used by the rest of the import pipeline.
///
/// DBS QIF exports use DD/MM/YYYY dates and a signed T-amount field
/// (negative = debit/expense, positive = credit/payment).
///
/// QIF files carry no account number or currency, so those fields are `None`.
pub fn parse(content: &str) -> Result<ParsedCsv> {
    // Strip a leading UTF-8 BOM if present — some exporters (including DBS)
    // include one.
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(content);

    let qif = qif_parser::parse(content, "%d/%m/%Y")
        .map_err(|e| anyhow::anyhow!("QIF parse error: {e}"))?;

    let rows = qif
        .transactions
        .into_iter()
        .map(|tx| {
            // qif_parser returns amount as a signed f64:
            //   negative → the card was charged (debit)
            //   positive → a payment was received (credit)
            let (debit, credit) = if tx.amount < 0.0 {
                (format!("{:.2}", -tx.amount), String::new())
            } else {
                (String::new(), format!("{:.2}", tx.amount))
            };

            // Collapse runs of whitespace in the payee string (DBS pads merchant
            // names to fixed-width columns).
            let description = tx.payee.split_whitespace().collect::<Vec<_>>().join(" ");

            ParsedRow {
                // qif_parser already outputs YYYY-MM-DD; we store that directly.
                date: tx.date,
                code: tx.number_of_the_check.to_string(),
                description,
                ref1: tx.memo.to_string(),
                ref2: String::new(),
                ref3: String::new(),
                status: tx.cleared_status.to_string(),
                debit,
                credit,
            }
        })
        .collect();

    Ok(ParsedCsv {
        account_number: None,
        account_name: None,
        currency: None,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/qif_ccard.qif");

    #[test]
    fn parse_row_count() {
        let parsed = parse(FIXTURE).unwrap();
        assert_eq!(parsed.rows.len(), 4);
    }

    #[test]
    fn parse_no_account_info() {
        let parsed = parse(FIXTURE).unwrap();
        assert!(parsed.account_number.is_none());
        assert!(parsed.account_name.is_none());
        assert!(parsed.currency.is_none());
    }

    #[test]
    fn parse_debit_transaction() {
        let parsed = parse(FIXTURE).unwrap();
        // First row: T-39.00 → debit
        let row = &parsed.rows[0];
        assert_eq!(row.date, "2026-04-02");
        assert_eq!(row.description, "SASCO SENIOR CITIZENS SINGAPORE SG");
        assert_eq!(row.debit, "39.00");
        assert_eq!(row.credit, "");
        assert_eq!(row.status, "*");
    }

    #[test]
    fn parse_credit_transaction() {
        let parsed = parse(FIXTURE).unwrap();
        // Second row: T39.00 → credit (inbound payment)
        let row = &parsed.rows[1];
        assert_eq!(row.date, "2026-03-26");
        assert_eq!(row.description, "INBOUND FT PYMT");
        assert_eq!(row.debit, "");
        assert_eq!(row.credit, "39.00");
    }

    #[test]
    fn parse_memo_and_number_fields() {
        let parsed = parse(FIXTURE).unwrap();
        // Third row has a memo and number
        let row = &parsed.rows[2];
        assert_eq!(row.code, "REF001");
        assert_eq!(row.ref1, "Insurance payment");
    }

    #[test]
    fn parse_amount_with_comma_separator() {
        let parsed = parse(FIXTURE).unwrap();
        // Fourth row: T-1,234.56
        let row = &parsed.rows[3];
        assert_eq!(row.debit, "1234.56");
    }
}
