use anyhow::{anyhow, bail, Result};
use regex::Regex;
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

// ── Embedded format assets ─────────────────────────────────────────────────────

#[derive(RustEmbed)]
#[folder = "formats/"]
struct FormatAssets;

// ── DSL types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Format {
    pub name: String,
    /// Entries evaluated in order; first match wins.
    #[serde(default)]
    pub account: Vec<ValueEntry>,
    /// Entries evaluated in order; first match wins.
    #[serde(default)]
    pub currency: Vec<ValueEntry>,
    /// Tried in order; the first entry whose column expressions all match the file wins.
    pub header: Vec<HeaderDef>,
}

#[derive(Debug, Deserialize)]
pub struct ValueEntry {
    /// If present, the cell at `location` must match `expression` before `value` is tried.
    pub condition: Option<CellMatch>,
    pub value: CellMatch,
}

#[derive(Debug, Deserialize)]
pub struct CellMatch {
    /// Cell reference, e.g. "B1" (column B, first row). Rows start at 1.
    pub location: String,
    /// Regex applied to the trimmed cell content. For `value`, capture group 1 is
    /// returned when present; otherwise the full match is used.
    pub expression: String,
}

#[derive(Debug, Deserialize)]
pub struct HeaderDef {
    /// 1-based row number of the column header row. Data rows begin on the next row.
    pub row: usize,
    pub mappings: Vec<ColumnMapping>,
}

#[derive(Debug, Deserialize)]
pub struct ColumnMapping {
    /// Column letter(s), e.g. "A" or "C". Rows are given by `HeaderDef::row`.
    pub column: String,
    /// Regex that must match the header cell. Acts as a safety check that the
    /// column contains what the format definition expects.
    pub expression: String,
    /// Transaction field to populate: date | code | description | ref1 | ref2 |
    /// ref3 | status | debit | credit
    pub field: String,
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

const VALID_FIELDS: &[&str] = &[
    "date", "code", "description", "ref1", "ref2", "ref3", "status", "debit", "credit",
];
const REQUIRED_FIELDS: &[&str] = &["date", "debit", "credit"];
const IDENTIFIER_FIELDS: &[&str] = &["code", "description", "ref1", "ref2", "ref3"];

fn validate(fmt: &Format) -> Result<()> {
    for (kind, entries) in &[("account", &fmt.account), ("currency", &fmt.currency)] {
        for (i, entry) in entries.iter().enumerate() {
            if let Some(cond) = &entry.condition {
                parse_cell_ref(&cond.location).map_err(|e| {
                    anyhow!(
                        "format '{}': {}[{i}].condition.location: {e}",
                        fmt.name,
                        kind
                    )
                })?;
                Regex::new(&cond.expression).map_err(|e| {
                    anyhow!(
                        "format '{}': {}[{i}].condition.expression: {e}",
                        fmt.name,
                        kind
                    )
                })?;
            }
            parse_cell_ref(&entry.value.location).map_err(|e| {
                anyhow!(
                    "format '{}': {}[{i}].value.location: {e}",
                    fmt.name,
                    kind
                )
            })?;
            Regex::new(&entry.value.expression).map_err(|e| {
                anyhow!(
                    "format '{}': {}[{i}].value.expression: {e}",
                    fmt.name,
                    kind
                )
            })?;
        }
    }

    if fmt.header.is_empty() {
        bail!("format '{}': 'header' must have at least one entry", fmt.name);
    }

    for (h, hdr) in fmt.header.iter().enumerate() {
        if hdr.row < 1 {
            bail!("format '{}': header[{h}].row must be >= 1", fmt.name);
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for (i, m) in hdr.mappings.iter().enumerate() {
            parse_col_ref(&m.column).map_err(|e| {
                anyhow!("format '{}': header[{h}].mappings[{i}].column: {e}", fmt.name)
            })?;
            Regex::new(&m.expression).map_err(|e| {
                anyhow!("format '{}': header[{h}].mappings[{i}].expression: {e}", fmt.name)
            })?;
            if !VALID_FIELDS.contains(&m.field.as_str()) {
                bail!(
                    "format '{}': header[{h}].mappings[{i}].field '{}' is not valid; must be one of: {}",
                    fmt.name, m.field, VALID_FIELDS.join(", ")
                );
            }
            seen.insert(m.field.as_str());
        }
        for req in REQUIRED_FIELDS {
            if !seen.contains(req) {
                bail!(
                    "format '{}': header[{h}].mappings must include a '{}' mapping",
                    fmt.name, req
                );
            }
        }
        if !IDENTIFIER_FIELDS.iter().any(|f| seen.contains(f)) {
            bail!(
                "format '{}': header[{h}].mappings must include at least one of: {}",
                fmt.name, IDENTIFIER_FIELDS.join(", ")
            );
        }
    }

    Ok(())
}

// ── Cell/column reference parsing ──────────────────────────────────────────────

/// Parse "B4" → (col=1, row=3) — both 0-based.
fn parse_cell_ref(s: &str) -> Result<(usize, usize)> {
    let col_str: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let row_str: String = s.chars().skip_while(|c| c.is_ascii_alphabetic()).collect();
    if col_str.is_empty() || row_str.is_empty() {
        bail!("expected a cell reference like 'A1' or 'B4', got '{s}'");
    }
    let col = col_letters_to_index(&col_str)?;
    let row = row_str
        .parse::<usize>()
        .map_err(|_| anyhow!("invalid row number in cell reference '{s}'"))?
        .checked_sub(1)
        .ok_or_else(|| anyhow!("row numbers start at 1, got '{s}'"))?;
    Ok((col, row))
}

/// Parse "C" → 2 (0-based column index).
fn parse_col_ref(s: &str) -> Result<usize> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphabetic()) {
        bail!("expected a column reference like 'A' or 'C', got '{s}'");
    }
    col_letters_to_index(s)
}

fn col_letters_to_index(s: &str) -> Result<usize> {
    Ok(s.to_ascii_uppercase()
        .chars()
        .fold(0usize, |acc, c| acc * 26 + (c as usize - 'A' as usize + 1))
        - 1)
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

fn get_cell<'a>(grid: &'a [Vec<String>], col: usize, row: usize) -> &'a str {
    grid.get(row)
        .and_then(|r| r.get(col))
        .map(|s| s.trim())
        .unwrap_or("")
}

/// Returns `(name, number)`. The name is the trimmed text in the cell before the
/// capture match; if that prefix is empty, the number is used as the name.
fn resolve_account(entry: &ValueEntry, grid: &[Vec<String>]) -> Result<Option<(String, String)>> {
    if let Some(cond) = &entry.condition {
        let (col, row) = parse_cell_ref(&cond.location)?;
        if !Regex::new(&cond.expression)?.is_match(get_cell(grid, col, row)) {
            return Ok(None);
        }
    }
    let (col, row) = parse_cell_ref(&entry.value.location)?;
    let cell = get_cell(grid, col, row);
    Ok(Regex::new(&entry.value.expression)?.captures(cell).map(|caps| {
        let m = caps.get(1).unwrap_or_else(|| caps.get(0).unwrap());
        let number = m.as_str().to_string();
        let prefix = cell[..m.start()].trim().to_string();
        let name = if prefix.is_empty() { number.clone() } else { prefix };
        (name, number)
    }))
}

fn resolve_value(entry: &ValueEntry, grid: &[Vec<String>]) -> Result<Option<String>> {
    if let Some(cond) = &entry.condition {
        let (col, row) = parse_cell_ref(&cond.location)?;
        if !Regex::new(&cond.expression)?.is_match(get_cell(grid, col, row)) {
            return Ok(None);
        }
    }
    let (col, row) = parse_cell_ref(&entry.value.location)?;
    let cell = get_cell(grid, col, row);
    Ok(Regex::new(&entry.value.expression)?.captures(cell).map(|caps| {
        caps.get(1)
            .unwrap_or_else(|| caps.get(0).unwrap())
            .as_str()
            .to_string()
    }))
}

/// Parse CSV content using the given format definition.
pub fn apply(fmt: &Format, content: &str) -> Result<ParsedCsv> {
    // Load all rows into a grid
    let mut grid: Vec<Vec<String>> = Vec::new();
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(content.as_bytes());
    for result in rdr.records() {
        grid.push(result?.iter().map(str::to_string).collect());
    }

    // Extract account and currency — first matching entry wins
    let (account_name, account_number) = fmt
        .account
        .iter()
        .find_map(|e| resolve_account(e, &grid).ok().flatten())
        .map(|(name, number)| (Some(name), Some(number)))
        .unwrap_or((None, None));
    let currency = fmt
        .currency
        .iter()
        .find_map(|e| resolve_value(e, &grid).ok().flatten());

    // Find the first header definition whose column expressions all match the file
    let hdr = fmt.header.iter().find(|hdr| {
        let row = hdr.row - 1;
        hdr.mappings.iter().all(|m| {
            parse_col_ref(&m.column)
                .ok()
                .and_then(|col| Regex::new(&m.expression).ok().map(|re| re.is_match(get_cell(&grid, col, row))))
                .unwrap_or(false)
        })
    }).ok_or_else(|| anyhow!(
        "no header entry in format '{}' matched the file; \
         check that --format is correct",
        fmt.name
    ))?;

    let mut field_col: HashMap<&str, usize> = HashMap::new();
    for m in &hdr.mappings {
        field_col.insert(m.field.as_str(), parse_col_ref(&m.column)?);
    }

    // Extract data rows
    let get = |row: &Vec<String>, field: &str| -> String {
        field_col
            .get(field)
            .and_then(|&col| row.get(col))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };

    let mut rows = Vec::new();
    for row in grid.iter().skip(hdr.row) {
        let date = get(row, "date");
        if date.is_empty() {
            continue;
        }
        rows.push(ParsedRow {
            date,
            code:        get(row, "code"),
            description: get(row, "description"),
            ref1:        get(row, "ref1"),
            ref2:        get(row, "ref2"),
            ref3:        get(row, "ref3"),
            status:      get(row, "status"),
            debit:       get(row, "debit"),
            credit:      get(row, "credit"),
        });
    }

    Ok(ParsedCsv { account_number, account_name, currency, rows })
}
