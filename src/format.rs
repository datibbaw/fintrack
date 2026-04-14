use anyhow::{anyhow, bail, Result};
use regex::Regex;
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::{collections::HashSet, hash::Hash};

// ── Embedded format assets ─────────────────────────────────────────────────────

#[derive(RustEmbed)]
#[folder = "formats/"]
struct FormatAssets;

// ── DSL types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Format {
    name: String,
    /// Entries evaluated in order; first match wins.
    #[serde(default)]
    account: Vec<ValueEntry>,
    /// Entries evaluated in order; first match wins.
    #[serde(default)]
    currency: Vec<ValueEntry>,
    /// Tried in order; the first entry whose column expressions all match the file wins.
    header: Vec<HeaderDef>,
}

#[derive(Debug, Deserialize)]
struct ValueEntry {
    /// If present, the cell at `location` must match `expression` before `value` is tried.
    condition: Option<CellMatch>,
    value: CellMatch,
}

impl ValueEntry {
    fn resolve<'a>(&self, grid: &'a [Vec<String>]) -> Option<(&'a str, regex::Captures<'a>)> {
        if let Some(condition) = &self.condition {
            if !condition.is_match(grid) {
                return None;
            }
        }

        self.value.resolve(grid)
    }
}

#[derive(Debug, Deserialize)]
struct CellMatch {
    /// Cell reference, e.g. "B1" (column B, first row). Rows start at 1.
    location: String,
    /// Regex applied to the trimmed cell content. For `value`, capture group 1 is
    /// returned when present; otherwise the full match is used.
    #[serde(with = "serde_regex")]
    expression: Regex,
}

impl CellMatch {
    fn coordinate(&self) -> (usize, usize) {
        parse_cell_ref(&self.location)
    }

    fn is_match(&self, grid: &[Vec<String>]) -> bool {
        let (col, row) = self.coordinate();
        self.expression.is_match(get_cell(grid, col, row))
    }

    fn resolve<'a>(&self, grid: &'a [Vec<String>]) -> Option<(&'a str, regex::Captures<'a>)> {
        let (col, row) = self.coordinate();
        let cell = get_cell(grid, col, row);
        self.expression.captures(cell).map(|caps| (cell, caps))
    }
}

#[derive(Debug, Deserialize)]
struct HeaderDef {
    /// 1-based row number of the column header row. Data rows begin on the next row.
    row: usize,
    mappings: Vec<ColumnMapping>,
}

impl HeaderDef {
    fn is_match(&self, grid: &[Vec<String>]) -> bool {
        let row = self.row - 1;
        self.mappings.iter().all(|m| m.is_match(grid, row))
    }

    fn fields(&self) -> Vec<Field> {
        self.mappings.iter().map(|m| m.field).collect()
    }

    fn parse_row(&self, row: &[String]) -> Result<ParsedRow> {
        let mut parsed = ParsedRow::default();
        for m in &self.mappings {
            let value = row
                .get(m.index())
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let setter: fn(&mut ParsedRow, String) = match m.field {
                Field::Date => |r, v| r.date = v,
                Field::Code => |r, v| r.code = v,
                Field::Description => |r, v| r.description = v,
                Field::Ref1 => |r, v| r.ref1 = v,
                Field::Ref2 => |r, v| r.ref2 = v,
                Field::Ref3 => |r, v| r.ref3 = v,
                Field::Status => |r, v| r.status = v,
                Field::Debit => |r, v| r.debit = v,
                Field::Credit => |r, v| r.credit = v,
            };
            setter(&mut parsed, value);
        }
        Ok(parsed)
    }
}

#[derive(Debug, Deserialize)]
struct ColumnMapping {
    /// Column letter(s), e.g. "A" or "C". Rows are given by `HeaderDef::row`.
    column: String,
    /// Regex that must match the header cell. Acts as a safety check that the
    /// column contains what the format definition expects.
    #[serde(with = "serde_regex")]
    expression: Regex,
    /// Transaction field to populate: date | code | description | ref1 | ref2 |
    /// ref3 | status | debit | credit
    field: Field,
}

impl ColumnMapping {
    fn index(&self) -> usize {
        parse_cell_ref(&self.column).0
    }

    fn is_match(&self, grid: &[Vec<String>], row: usize) -> bool {
        let col = self.index();
        self.expression.is_match(get_cell(grid, col, row))
    }
}

// ── Parsed output ──────────────────────────────────────────────────────────────

pub struct ParsedCsv {
    /// Account number extracted from the file, if the format defines how to find it.
    pub account_number: Option<String>,
    /// Account name derived from the text preceding the number capture (trimmed).
    /// Falls back to the number itself when that prefix is empty.
    pub account_name: Option<String>,
    /// Currency code extracted from the file, if the format defines how to find it.
    pub currency: Option<String>,
    pub rows: Vec<ParsedRow>,
}

#[derive(Default)]
pub struct ParsedRow {
    pub date: String,
    pub code: String,
    pub description: String,
    pub ref1: String,
    pub ref2: String,
    pub ref3: String,
    pub status: String,
    pub debit: String,
    pub credit: String,
}

// ── Validation ─────────────────────────────────────────────────────────────────

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
}

const REQUIRED_FIELDS: &[Field] = &[Field::Date, Field::Debit, Field::Credit];
const IDENTIFIER_FIELDS: &[Field] = &[
    Field::Code,
    Field::Description,
    Field::Ref1,
    Field::Ref2,
    Field::Ref3,
];

fn validate(fmt: &Format) -> Result<()> {
    if fmt.header.is_empty() {
        bail!(
            "format '{}': 'header' must have at least one entry",
            fmt.name
        );
    }

    for (h, hdr) in fmt.header.iter().enumerate() {
        if hdr.row < 1 {
            bail!("format '{}': header[{h}].row must be >= 1", fmt.name);
        }
        let seen: HashSet<Field> = hdr.fields().into_iter().collect();

        for req in REQUIRED_FIELDS {
            if !seen.contains(req) {
                bail!(
                    "format '{}': header[{h}].mappings must include a '{:?}' mapping",
                    fmt.name,
                    req
                );
            }
        }

        if !IDENTIFIER_FIELDS.iter().any(|f| seen.contains(f)) {
            bail!(
                "format '{}': header[{h}].mappings must include at least one of: {}",
                fmt.name,
                IDENTIFIER_FIELDS
                    .iter()
                    .map(|f| format!("{:?}", f))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    Ok(())
}

// ── Cell/column reference parsing ──────────────────────────────────────────────
/// Parse "B4" → (col=1, row=3) — both 0-based.
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

    (col - 1, row_str.parse::<usize>().unwrap_or(1) - 1)
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Load a built-in format by name (case-insensitive).
pub fn load(name: &str) -> Result<Format> {
    let filename = format!("{}.yaml", name.to_lowercase());
    let asset = FormatAssets::get(&filename).ok_or_else(|| {
        anyhow!(
            "unknown CSV format '{name}'; available: {}",
            list_names().join(", ")
        )
    })?;
    let yaml = std::str::from_utf8(asset.data.as_ref())?;
    let fmt: Format = serde_yaml::from_str(yaml)
        .map_err(|e| anyhow!("failed to parse format file '{filename}': {e}"))?;
    validate(&fmt)?;
    Ok(fmt)
}

/// List available built-in format names (sorted).
pub fn list_names() -> Vec<String> {
    let mut names: Vec<String> = FormatAssets::iter()
        .filter_map(|f| f.strip_suffix(".yaml").map(str::to_string))
        .collect();
    names.sort();
    names
}

// ── Applying a format to CSV content ──────────────────────────────────────────

fn get_cell(grid: &[Vec<String>], col: usize, row: usize) -> &str {
    grid.get(row)
        .and_then(|r| r.get(col))
        .map(|s| s.trim())
        .unwrap_or("")
}

/// Returns `(name, number)`. The name is the trimmed text in the cell before the
/// capture match; if that prefix is empty, the number is used as the name.
fn to_account(cell: &str, caps: &regex::Captures) -> (String, String) {
    let m = caps.get(1).unwrap_or_else(|| caps.get(0).unwrap());
    let number = m.as_str().to_string();
    let prefix = cell[..m.start()].trim().to_string();
    let name = if prefix.is_empty() {
        number.clone()
    } else {
        prefix
    };
    (name, number)
}

/// Iterates over `entries` and returns the first value that matches the file, applying `f` to the cell content and regex captures.
/// Returns `None` if no entry matches.
fn resolve_value_entry<F, U>(entries: &[ValueEntry], grid: &[Vec<String>], f: F) -> Option<U>
where
    F: Fn(&str, &regex::Captures) -> U,
    U: Clone,
{
    entries
        .iter()
        .find_map(|e| e.resolve(grid).map(|(v, caps)| f(v, &caps)))
}

fn load_grid(content: &str) -> Result<Vec<Vec<String>>> {
    let mut grid: Vec<Vec<String>> = Vec::new();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(content.as_bytes());
    for result in reader.records() {
        grid.push(result?.iter().map(str::to_string).collect());
    }
    Ok(grid)
}

/// Parse CSV content using the given format definition.
pub fn apply(fmt: &Format, content: &str) -> Result<ParsedCsv> {
    // Load all rows into a grid
    let grid = load_grid(content)?;
    let (account_name, account_number) = match resolve_value_entry(&fmt.account, &grid, to_account)
    {
        Some((name, number)) => (Some(name), Some(number)),
        None => (None, None),
    };
    let currency = resolve_value_entry(&fmt.currency, &grid, |_v, caps| {
        let m = caps.get(1).unwrap_or_else(|| caps.get(0).unwrap());
        m.as_str().to_string()
    });

    // Find the first header definition whose column expressions all match the file
    let hdr = fmt
        .header
        .iter()
        .find(|hdr| hdr.is_match(&grid))
        .ok_or_else(|| {
            anyhow!(
                "no header entry in format '{}' matched the file; \
         check that --format is correct",
                fmt.name
            )
        })?;

    let rows = grid
        .iter()
        .skip(hdr.row)
        .filter_map(|row| hdr.parse_row(row).ok())
        .collect();

    Ok(ParsedCsv {
        account_number,
        account_name,
        currency,
        rows,
    })
}
