use std::path::Path;

use crate::{import::specs, models::TransactionBuilder};
use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use csv::{Reader, StringRecord};
use regex::Regex;
use serde::Deserialize;

const CSV_SPEC: &str = "csv.yaml";

#[derive(Deserialize)]
struct Spec {
    account_number: Vec<ValueSpec>,
    currency: Vec<ValueSpec>,
    row_formats: Vec<RowFormat>,
}

pub(super) struct CsvMetadata {
    pub(super) account_number: Option<String>,
    pub(super) currency: Option<String>,
}

pub(super) struct TransactionRows {
    records: csv::StringRecordsIntoIter<std::fs::File>,
    row_format: RowFormat,
}

impl Iterator for TransactionRows {
    type Item = Result<TransactionBuilder>;

    fn next(&mut self) -> Option<Self::Item> {
        let record = self.records.next()?.map_err(anyhow::Error::from);
        Some(record.and_then(|r| {
            let row = r
                .into_iter()
                .enumerate()
                .filter_map(|(idx, cell)| {
                    let col = self.row_format.columns.iter().find(|c| c.index() == idx)?;
                    Some((col.field, cell.trim().to_string()))
                })
                .collect();
            into_builder(&self.row_format, row)
        }))
    }
}

/// ValueSpec defines how to extract a single value (e.g. account number) from the CSV file.
#[derive(Deserialize)]
struct ValueSpec {
    condition: CellMatch,
    value: CellMatch,
}

#[derive(Deserialize)]
struct CellMatch {
    location: String,
    #[serde(with = "serde_regex")]
    expression: Regex,
}

/// RowFormat defines how to detect and parse transaction rows in the CSV file.
#[derive(Clone, Deserialize)]
struct RowFormat {
    date_format: String,
    #[serde(default)]
    invert_amount_sign: bool,
    columns: Vec<ColumnSpec>,
}

impl RowFormat {
    fn parse_date(&self, value: &str) -> Result<NaiveDate> {
        NaiveDate::parse_from_str(value, &self.date_format)
            .with_context(|| format!("failed to parse date: '{}'", value))
    }

    fn parse_amount(&self, value: &str) -> Option<i64> {
        let value = parse_money_value(value).ok();
        if self.invert_amount_sign {
            value.map(|n| -n)
        } else {
            value
        }
    }
}

impl CellMatch {
    fn coordinate(&self) -> (usize, usize) {
        parse_cell_ref(&self.location)
    }

    fn is_match(&self, data: &StringRecord, row_number: usize) -> bool {
        let (col, row) = self.coordinate();
        if row != row_number {
            return false;
        }
        data.get(col)
            .is_some_and(|cell| self.expression.is_match(cell.trim()))
    }

    fn resolve<'a>(&self, data: &'a StringRecord, row_number: usize) -> Option<&'a str> {
        let (col, row) = self.coordinate();
        if row != row_number {
            return None;
        }
        let cell = data.get(col)?;
        self.expression.captures(cell)?.get(1).map(|m| m.as_str())
    }
}

enum SpecField<'a> {
    AccountNumber(String),
    RowFormat(&'a RowFormat),
    Currency(String),
}

fn parse_money_value(s: &str) -> Result<i64> {
    s.replace(",", "")
        .parse::<f64>()
        .map(|f| (f * 100.0).round() as i64)
        .map_err(|e| anyhow!("Failed to parse money value '{}': {}", s, e))
}

pub(super) fn parse<P: AsRef<Path>>(path: P) -> Result<(CsvMetadata, TransactionRows)> {
    let spec: Spec = specs::load(CSV_SPEC)?;
    spec.scan(path)
}

fn reader_from_path<P: AsRef<Path>>(path: P) -> Result<Reader<std::fs::File>> {
    Ok(csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)?)
}

impl Spec {
    fn parse_value_spec(
        &self,
        record: &StringRecord,
        row_number: usize,
        value_specs: &[ValueSpec],
    ) -> Option<String> {
        value_specs.iter().find_map(|value_spec| {
            value_spec
                .condition
                .is_match(record, row_number)
                .then(|| value_spec.value.resolve(record, row_number))
                .flatten()
                .map(|s| s.to_string())
        })
    }

    fn parse_row_format(&self, record: &StringRecord) -> Option<&RowFormat> {
        self.row_formats.iter().find(|format| {
            format.columns.iter().all(|col| {
                record
                    .get(col.index())
                    .is_some_and(|cell| col.expression.is_match(cell.trim()))
            })
        })
    }

    fn parse_spec_field<'a>(
        &'a self,
        record: &StringRecord,
        row_number: usize,
    ) -> Option<SpecField<'a>> {
        if let Some(row_format) = self.parse_row_format(record) {
            return Some(SpecField::RowFormat(row_format));
        }
        if let Some(n) = self.parse_value_spec(record, row_number, &self.account_number) {
            return Some(SpecField::AccountNumber(n));
        }
        if let Some(c) = self.parse_value_spec(record, row_number, &self.currency) {
            return Some(SpecField::Currency(c));
        }
        None
    }

    fn scan<P: AsRef<Path>>(&self, path: P) -> Result<(CsvMetadata, TransactionRows)> {
        let mut reader = reader_from_path(path)?;
        let mut metadata = CsvMetadata {
            account_number: None,
            currency: None,
        };
        let mut row_format: Option<RowFormat> = None;

        for (row_number, record) in reader.records().enumerate() {
            let record = record?;

            match self.parse_spec_field(&record, row_number) {
                Some(SpecField::RowFormat(fmt)) => {
                    row_format = Some(fmt.clone());
                    break;
                }
                Some(SpecField::AccountNumber(n)) => metadata.account_number = Some(n),
                Some(SpecField::Currency(c)) => metadata.currency = Some(c),
                None => {}
            }
        }

        let row_format =
            row_format.ok_or_else(|| anyhow!("no matching row format found in CSV"))?;
        let rows = TransactionRows {
            records: reader.into_records(),
            row_format,
        };

        Ok((metadata, rows))
    }
}

type Row = Vec<(Field, String)>;

fn into_builder(format: &RowFormat, row: Row) -> Result<TransactionBuilder> {
    let mut builder = TransactionBuilder::default();

    for (field, value) in row {
        match field {
            Field::Date => {
                builder.date(
                    format
                        .parse_date(&value)
                        .map_err(|e| anyhow!("Failed to parse date: {}", e))?,
                );
            }
            Field::Debit => {
                builder.debit(parse_money_value(&value).ok().map(|c| c.abs()));
            }
            Field::Credit => {
                builder.credit(parse_money_value(&value).ok().map(|c| c.abs()));
            }
            Field::Amount => {
                if let Some(n) = format.parse_amount(&value) {
                    builder.amount(n);
                }
            }
            Field::Code => {
                builder.code(value);
            }
            Field::Description => {
                builder.description(value);
            }
            Field::Ref1 => {
                builder.ref1(value);
            }
            Field::Ref2 => {
                builder.ref2(value);
            }
            Field::Ref3 => {
                builder.ref3(value);
            }
            Field::Status => {
                builder.status(value);
            }
        }
    }

    Ok(builder)
}

#[derive(Debug, Deserialize, PartialEq, Eq, Hash, Copy, Clone)]
#[serde(rename_all = "lowercase")]
enum Field {
    Date,
    Code,
    Description,
    Ref1,
    Ref2,
    Ref3,
    Status,
    Debit,
    Credit,
    Amount, // auto detect debit vs credit based on sign
}

#[derive(Clone, Debug, Deserialize)]
struct ColumnSpec {
    column: String,
    #[serde(with = "serde_regex")]
    expression: Regex,
    field: Field,
}

impl ColumnSpec {
    fn index(&self) -> usize {
        parse_cell_ref(&self.column).0
    }
}

fn parse_cell_ref(s: &str) -> (usize, usize) {
    let mut col = 0usize;
    let mut row_str = String::new();

    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            col = col * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1);
        } else if c.is_ascii_digit() {
            row_str.push(c);
        }
    }

    (
        col.saturating_sub(1),
        row_str.parse::<usize>().unwrap_or(1).saturating_sub(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cell_ref_misc() {
        assert_eq!(parse_cell_ref("A1"), (0, 0));
        assert_eq!(parse_cell_ref("a1"), (0, 0));
        assert_eq!(parse_cell_ref("B2"), (1, 1));
        assert_eq!(parse_cell_ref("b2"), (1, 1));
        assert_eq!(parse_cell_ref("Z1"), (25, 0));
        assert_eq!(parse_cell_ref("AA1"), (26, 0));
        assert_eq!(parse_cell_ref("AB1"), (27, 0));
        assert_eq!(parse_cell_ref("A100"), (0, 99));
        // no column letters → default to column 0
        assert_eq!(parse_cell_ref("1"), (0, 0));
        // no row digits → default to row 0
        assert_eq!(parse_cell_ref("C"), (2, 0));
    }

    #[test]
    fn test_parse_cell_ref_empty() {
        assert_eq!(parse_cell_ref(""), (0, 0));
    }
}
