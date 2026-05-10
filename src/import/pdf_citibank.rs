use std::{collections::HashMap, path::Path, str::Lines, sync::LazyLock};

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use regex::Regex;
use rusty_money::{iso, Money};

use crate::models::TransactionBuilder;

static STATEMENT_DATE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Statement Date (\w{5,} \d{1,2}, \d{4})$").unwrap());

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

static MONTH_TO_NUM: LazyLock<HashMap<&str, u32>> = LazyLock::new(|| HashMap::from(MONTHS));

#[derive(Debug)]
struct Transaction {
    day: u32,
    month: u32,
    description: String,
    amount: i64,
}

fn skip_until_transaction_date(iter: &mut Lines) -> Option<NaiveDate> {
    iter.find_map(|line| {
        STATEMENT_DATE_REGEX
            .captures(line)
            .and_then(|caps| NaiveDate::parse_from_str(caps.get(1)?.as_str(), "%B %d, %Y").ok())
    })
}

fn full_transaction_date(day: u32, month: u32, statement_date: &NaiveDate) -> Option<NaiveDate> {
    // transaction dates always come before statement dates
    let year = if month > statement_date.month() {
        // adjust for december transactions
        statement_date.year() - 1
    } else {
        statement_date.year()
    };
    NaiveDate::from_ymd_opt(year, month, day)
}

fn parse_transaction(input: &str, currency: &'static iso::Currency) -> Option<Transaction> {
    let to_amount = |s: &str| {
        Money::from_str(s, currency)
            .map(|m| m.to_minor_units())
            .ok()
    };
    STATEMENT_REGEX.captures(input).and_then(|caps| {
        let amount = match (caps.get(4), caps.get(5)) {
            (Some(amount), None) => to_amount(amount.as_str())?,
            (None, Some(amount)) => -to_amount(amount.as_str())?,
            _ => return None,
        };
        Some(Transaction {
            day: caps.get(1)?.as_str().parse::<u32>().ok()?,
            month: *MONTH_TO_NUM.get(caps.get(2)?.as_str())?,
            description: caps.get(3)?.as_str().to_string(),
            amount,
        })
    })
}

pub(super) fn parse<P: AsRef<Path>>(
    path: P,
    currency: &'static iso::Currency,
) -> Result<Vec<TransactionBuilder>> {
    let text = pdf_extract::extract_text(path.as_ref())?;
    let mut lines = text.lines();
    let statement_date = skip_until_transaction_date(&mut lines)
        .ok_or_else(|| anyhow::anyhow!("could not find statement date in Citibank PDF"))?;
    let builders = lines
        .filter_map(|line| {
            let tx = parse_transaction(line, currency)?;
            let date = full_transaction_date(tx.day, tx.month, &statement_date)?;
            let mut builder = TransactionBuilder::default();
            builder.date(date);
            builder.description(tx.description);
            if tx.amount >= 0 {
                builder.debit(Some(tx.amount));
            } else {
                builder.credit(Some(-tx.amount));
            }
            Some(builder)
        })
        .collect();
    Ok(builders)
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    #[ignore]
    fn pdf_contents() {
        let path = env::var("PDF_FILE").expect("set PDF_FILE=/path/to/citibank-statement.pdf");
        let contents: Vec<u8> = std::fs::read(path).unwrap();
        let text = pdf_extract::extract_text_from_mem(&contents).unwrap();
        let mut iter = text.lines().into_iter();

        let statement_date = skip_until_transaction_date(&mut iter).unwrap();
        println!("statement_date: {statement_date:?}");

        iter.filter_map(|line| parse_transaction(line, &iso::SGD))
            .for_each(|t| {
                let transaction_date = full_transaction_date(t.day, t.month, &statement_date);
                println!("Transaction: {t:?} (transaction_date: {transaction_date:?})");
            });
    }
}
