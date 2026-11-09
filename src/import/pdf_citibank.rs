use std::{path::Path, str::Lines, sync::LazyLock};

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use regex::Regex;
use rusty_money::{Money, iso};

use crate::{
    import::{AsTransactionBuilder, ParseData},
    models::TransactionBuilder,
};

static STATEMENT_DATE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Statement Date (\w{5,} \d{1,2}, \d{4})$").unwrap());

static ACCOUNT_NUMBER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{4} \d{4} \d{4} \d{4}) Payment Due Date:").unwrap());

/// Transaction line pattern
///
/// Captures the day (1), month (2), description (3), and debit (4) or credit (5).
static STATEMENT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+) ([A-Z]{3}) (.*) (?:([\d,.]+)|\(([\d,.]+)\))$").unwrap());

const MONTHS: [(&str, u32); 12] = [
    ("JAN", 1),
    ("FEB", 2),
    ("MAR", 3),
    ("APR", 4),
    ("MAY", 5),
    ("JUN", 6),
    ("JUL", 7),
    ("AUG", 8),
    ("SEP", 9),
    ("OCT", 10),
    ("NOV", 11),
    ("DEC", 12),
];

#[derive(Debug)]
enum Amount {
    Debit(String),
    Credit(String),
}

#[derive(Debug)]
struct Transaction {
    date: NaiveDate,
    description: String,
    amount: Amount,
}

impl AsTransactionBuilder for Transaction {
    fn as_transaction_builder(
        &self,
        currency: &'static iso::Currency,
    ) -> Result<TransactionBuilder> {
        let mut builder = TransactionBuilder::default();
        let to_amount = |s: &str| Money::from_str(s, currency).map(|m| m.to_minor_units());
        builder.date(self.date);
        builder.description(self.description.clone());
        let amount = match &self.amount {
            Amount::Credit(value) => to_amount(value)?,
            Amount::Debit(value) => -to_amount(value)?,
        };
        builder.amount(amount);
        Ok(builder)
    }
}

fn skip_until_transaction_date(iter: &mut Lines) -> Option<NaiveDate> {
    iter.find_map(|line| {
        STATEMENT_DATE_REGEX
            .captures(line)
            .and_then(|caps| NaiveDate::parse_from_str(caps.get(1)?.as_str(), "%B %d, %Y").ok())
    })
}

fn skip_until_account_number(iter: &mut Lines) -> Option<String> {
    iter.find_map(|line| {
        ACCOUNT_NUMBER_REGEX
            .captures(line)
            .and_then(|caps| Some(caps.get(1)?.as_str().to_string()))
    })
}

fn full_transaction_date(day: u32, month: u32, statement_date: NaiveDate) -> Option<NaiveDate> {
    // transaction dates always come before statement dates
    let year = if month > statement_date.month() {
        // adjust for december transactions
        statement_date.year() - 1
    } else {
        statement_date.year()
    };
    NaiveDate::from_ymd_opt(year, month, day)
}

fn parse_transaction(input: &str, statement_date: NaiveDate) -> Option<Transaction> {
    let captures = STATEMENT_REGEX.captures(input)?;
    let amount = match (captures.get(4), captures.get(5)) {
        (Some(debit), None) => Amount::Debit(debit.as_str().to_string()),
        (None, Some(credit)) => Amount::Credit(credit.as_str().to_string()),
        _ => return None,
    };
    let month = captures.get(2)?.as_str();
    let month = MONTHS
        .iter()
        .find(|(name, _)| *name == month)
        .map(|(_, no)| *no)?;
    let date = full_transaction_date(
        captures.get(1)?.as_str().parse::<u32>().ok()?,
        month,
        statement_date,
    )?;
    Some(Transaction {
        date,
        description: captures.get(3)?.as_str().to_string(),
        amount,
    })
}

pub(super) fn parse<P: AsRef<Path>>(path: P) -> Result<ParseData> {
    let text = pdf_extract::extract_text(path.as_ref())?;
    let mut lines = text.lines();
    let statement_date = skip_until_transaction_date(&mut lines)
        .ok_or_else(|| anyhow::anyhow!("could not find statement date"))?;
    let account_number = skip_until_account_number(&mut lines)
        .ok_or_else(|| anyhow::anyhow!("could not find account number"))?;
    let mut data = ParseData {
        account_number: Some(account_number),
        ..Default::default()
    };

    for line in lines {
        if let Some(transaction) = parse_transaction(line, statement_date) {
            data.records.push(Box::new(transaction));
        }
    }

    Ok(data)
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    #[ignore]
    fn pdf_contents() {
        let path = env::var("PDF_FILE").expect("set PDF_FILE=/path/to/citibank-statement.pdf");
        let contents: Vec<u8> = std::fs::read(&path).unwrap();
        let text = pdf_extract::extract_text_from_mem(&contents).unwrap();

        println!("{text}");

        let data = parse(path).unwrap();
        println!(
            "account_number: {}",
            data.account_number.as_deref().unwrap_or("")
        );
        for record in &data.records {
            println!(
                "Transaction: {:?}",
                record
                    .as_transaction_builder(iso::SGD)
                    .unwrap()
                    .build()
                    .unwrap()
            );
        }
    }
}
