use std::{collections::HashMap, path::Path};

use crate::{import::specs, models::TransactionBuilder};
use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use csv::Reader;
use regex::Regex;
use serde::Deserialize;

const CSV_SPEC: &str = "csv.yaml";

#[derive(Deserialize)]
struct Format {
    date_format: String,
    #[serde(default)]
    invert_amount_sign: bool,
    columns: Vec<ColumnSpec>,
}

impl Format {
    fn parse_date(&self, value: &str) -> Result<NaiveDate> {
        NaiveDate::parse_from_str(value, &self.date_format)
            .with_context(|| format!("failed to parse date: '{}'", value))
    }

    fn parse_amount(&self, value: &str) -> Option<f64> {
        let value = parse_money_value(value).ok();
        if self.invert_amount_sign {
            value.map(|n| -n)
        } else {
            value
        }
    }
}

fn parse_money_value(s: &str) -> Result<f64> {
    s.trim().replace(',', "").parse::<f64>().with_context(|| {
        format!("failed to parse money value: '{s}' (expected a number, optionally with commas)")
    })
}

pub(super) fn parse<P: AsRef<Path>>(path: P) -> Result<Vec<TransactionBuilder>> {
    let formats: Vec<Format> = specs::load(CSV_SPEC)?;
    let mut reader = reader_from_path(&path)?;
    let format =
        detect_format(&formats, &mut reader)?.ok_or_else(|| anyhow!("Failed to detect format"))?;

    parse_with_format(reader, format)
}

fn reader_from_path<P: AsRef<Path>>(path: P) -> Result<Reader<std::fs::File>> {
    Ok(csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)?)
}

fn detect_format<'a>(
    formats: &'a [Format],
    reader: &mut Reader<impl std::io::Read>,
) -> Result<Option<&'a Format>> {
    while let Some(record) = reader.records().next() {
        let record = record?;
        if let Some(format) = formats.iter().find(|format| {
            format.columns.iter().all(|col| {
                record
                    .get(col.index())
                    .is_some_and(|cell| col.expression.is_match(cell.trim()))
            })
        }) {
            return Ok(Some(format));
        }
    }
    Ok(None)
}

fn parse_with_format(
    mut reader: Reader<impl std::io::Read>,
    format: &Format,
) -> Result<Vec<TransactionBuilder>> {
    let column_specs_by_index: HashMap<usize, &ColumnSpec> = format
        .columns
        .iter()
        .map(|col| (col.index(), col))
        .collect();

    reader
        .records()
        .map(|record| {
            let record = record?;
            let row = column_specs_by_index
                .iter()
                .filter_map(|(idx, col)| {
                    record
                        .get(*idx)
                        .map(|cell| (col.field, cell.trim().to_string()))
                })
                .collect();
            into_builder(format, row)
        })
        .collect()
}

type Row = Vec<(Field, String)>;

fn into_builder(format: &Format, row: Row) -> Result<TransactionBuilder> {
    let mut builder = TransactionBuilder::default();

    for (field, value) in row {
        match field {
            Field::Date => {
                builder.date(format.parse_date(&value)?);
            }
            Field::Debit => {
                builder.debit(parse_money_value(&value).ok());
            }
            Field::Credit => {
                builder.credit(parse_money_value(&value).ok());
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

#[derive(Debug, Deserialize)]
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
            // Convert 'A' -> 1, 'B' -> 2, etc. (Base 26)
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
