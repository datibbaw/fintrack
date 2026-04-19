use crate::models::TransactionBuilder;
use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use csv::Reader;
use regex::Regex;
use rust_embed::RustEmbed;
use serde::Deserialize;

const CSV_SPEC: &str = "csv.yaml";

#[derive(RustEmbed)]
#[folder = "specs/"]
struct Specs;

#[derive(Deserialize)]
pub struct FormatSpec {
    // name: String,
    date_format: String,
    #[serde(default)]
    invert_amount_sign: bool,
    columns: Vec<ColumnSpec>,
}

pub struct ReaderSpec {
    formats: Vec<FormatSpec>,
}

struct Meta<'a> {
    format: &'a FormatSpec,
    spec: &'a ColumnSpec,
}

impl Meta<'_> {
    fn parse_date(&self, value: &str) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(value, &self.format.date_format).ok()
    }

    fn parse_amount(&self, value: &str) -> Option<f64> {
        let value = parse_money_value(value).ok();
        if self.format.invert_amount_sign {
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

pub struct Row<'a>(Vec<(Meta<'a>, String)>);

pub(crate) fn into_builder(row: Row<'_>) -> TransactionBuilder {
    let mut builder = TransactionBuilder::default();

    for (meta, value) in row.0 {
        match meta.spec.field {
            Field::Date => {
                if let Some(date) = meta.parse_date(&value) {
                    builder.date(date);
                }
            }
            Field::Debit => {
                builder.debit(parse_money_value(&value).ok());
            }
            Field::Credit => {
                builder.credit(parse_money_value(&value).ok());
            }
            Field::Amount => {
                if let Some(n) = meta.parse_amount(&value) {
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

    builder
}

impl ReaderSpec {
    pub fn new() -> Result<Self> {
        let content = Specs::get(CSV_SPEC)
            .ok_or_else(|| anyhow!("Spec {CSV_SPEC} not found"))?
            .data;
        let formats = serde_yaml::from_slice(&content)
            .with_context(|| "failed to parse csv format: invalid YAML or missing fields")?;

        Ok(Self { formats })
    }

    /// Scans records until it finds a matching header row and returns the corresponding format.
    /// Leaves the reader positioned after the matched row, so consumed rows cannot be read again.
    fn detect_format(
        &self,
        reader: &mut Reader<impl std::io::Read>,
    ) -> Result<Option<&FormatSpec>> {
        while let Some(record) = reader.records().next() {
            let record = record?;
            if let Some(format) = self.formats.iter().find(|format| {
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

    pub fn rows(&self, path: impl AsRef<std::path::Path>) -> Result<Vec<Row<'_>>> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_path(path)?;

        match self.detect_format(&mut reader)? {
            Some(format) => {
                let column_specs_by_index: std::collections::HashMap<usize, &ColumnSpec> = format
                    .columns
                    .iter()
                    .map(|col| (col.index(), col))
                    .collect();

                reader
                    .records()
                    .map(|record| {
                        let record = record?;
                        let cols = column_specs_by_index
                            .iter()
                            .filter_map(|(idx, col)| {
                                record.get(*idx).map(|cell| {
                                    let meta = Meta { format, spec: col };
                                    (meta, cell.trim().to_string())
                                })
                            })
                            .collect();
                        Ok(Row(cols))
                    })
                    .collect()
            }
            None => Err(anyhow!("Failed to detect format")),
        }
    }
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
