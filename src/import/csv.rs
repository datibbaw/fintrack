use std::{collections::HashMap, path::Path};

use crate::import::specs;
use anyhow::{anyhow, Result};
use csv::{Reader, StringRecord};
use regex::Regex;
use serde::Deserialize;

const CSV_SPEC: &str = "csv.yaml";

#[derive(Deserialize)]
pub(super) struct Spec {
    account_number: Vec<ValueSpec>,
    currency: Vec<ValueSpec>,
    row_formats: Vec<RowFormat>,
}

#[derive(Default)]
pub(super) struct Data {
    pub(super) account_number: Option<String>,
    pub(super) currency: Option<String>,
    pub(super) rows: Vec<Row>,
}

pub(super) fn parse<P: AsRef<Path>>(path: P) -> Result<Data> {
    let spec: Spec = specs::load(CSV_SPEC)?;
    spec.parse(path)
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
    #[allow(dead_code)]
    name: String,
    columns: Vec<ColumnSpec>,
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

    fn parse<P: AsRef<Path>>(&self, path: P) -> Result<Data> {
        let mut reader = reader_from_path(path)?;
        let mut data = Data::default();
        let mut row_format: Option<RowFormat> = None;

        for (row_number, record) in reader.records().enumerate() {
            let record = record?;

            match self.parse_spec_field(&record, row_number) {
                Some(SpecField::RowFormat(fmt)) => {
                    row_format = Some(fmt.clone());
                    break;
                }
                Some(SpecField::AccountNumber(n)) => data.account_number = Some(n),
                Some(SpecField::Currency(c)) => data.currency = Some(c),
                None => {}
            }
        }

        let row_format =
            row_format.ok_or_else(|| anyhow!("no matching row format found in CSV"))?;

        let fields: HashMap<usize, Field> = row_format.columns.into_iter()
            .map(|col| (col.index(), col.field))
            .collect();

        for record in reader.records() {
            let row = record?
                .into_iter()
                .enumerate()
                .filter_map(|(idx, cell)| {
                    let field = fields.get(&idx)?;
                    Some((field.clone(), cell.trim().to_string()))
                })
                .collect();

            data.rows.push(row);
        }
        Ok(data)
    }
}

pub(super) type Row = Vec<(Field, String)>;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Deserialize)]
#[serde(tag = "field", rename_all = "lowercase")]
pub(super) enum Field {
    Date { date_format: String },
    Code,
    Description,
    Ref1,
    Ref2,
    Ref3,
    Status,
    Debit,
    Credit,
    Amount { invert: Option<bool> },
}

#[derive(Clone, Debug, Deserialize)]
struct ColumnSpec {
    column: String,
    #[serde(with = "serde_regex")]
    expression: Regex,
    #[serde(flatten)]
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

    #[test]
    fn parse_9col() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dbs_9col.csv");
        let result = parse(path).unwrap();
        assert_eq!(result.rows.len(), 4);
    }
}
