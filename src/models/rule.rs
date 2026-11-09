use std::fmt;
use std::str::FromStr;

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use tabled::Tabled;

use crate::models::Transaction;

#[derive(Debug, Clone, Deserialize, Tabled)]
#[tabled(rename_all = "PascalCase")]
pub struct Rule {
    pub id: i64,
    #[tabled(skip)]
    pub category_id: i64,
    /// optional eager loaded fields from the joined category:
    #[tabled(rename = "Category")]
    #[serde(default)]
    pub category_name: String,
    #[tabled(skip)]
    #[serde(default)]
    pub category_is_sub: bool,
    /// When set, the rule only applies to transactions of this account.
    #[tabled(skip)]
    #[serde(default)]
    pub account_id: Option<i64>,
    /// optional eager loaded field from the joined account:
    #[tabled(rename = "Account", display = "super::opt_string")]
    #[serde(default)]
    pub account_name: Option<String>,
    /// Stored in the database as a JSON-encoded TEXT column.
    #[serde(deserialize_with = "criteria_from_json")]
    pub criteria: Criteria,
    pub priority: i64,
}

fn criteria_from_json<'de, D: Deserializer<'de>>(d: D) -> Result<Criteria, D::Error> {
    let json = String::deserialize(d)?;
    serde_json::from_str(&json).map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    Code,
    Description,
    Ref1,
    Ref2,
    Ref3,
    /// Signed amount in minor units: credits positive, debits negative.
    Amount,
}

impl Field {
    pub const ALL: &[&str] = &["description", "ref1", "ref2", "ref3", "code", "amount"];

    /// Integer-valued fields only support `equals` with an `i64` value.
    pub fn is_integer(self) -> bool {
        matches!(self, Field::Amount)
    }
}

impl FromStr for Field {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| {
            anyhow::anyhow!(
                "Invalid field '{s}'. Must be one of: {}",
                Field::ALL.join(", ")
            )
        })
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        f.write_str(s.as_str().unwrap_or_default())
    }
}

/// Operand of an `equals` matcher: a JSON string for text fields, a JSON
/// integer for integer fields (`amount`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Integer(i64),
    Text(String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(n) => write!(f, "{n}"),
            Value::Text(s) => write!(f, "{s:?}"),
        }
    }
}

/// How a single field value is compared.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Matcher {
    /// Exact equality.
    Equals(Value),
    /// Substring match (text fields only).
    Includes(String),
    /// Regular expression match (text fields only).
    Matches(#[serde(with = "serde_regex")] Regex),
}

impl Matcher {
    fn is_text_match(&self, value: &str) -> bool {
        match self {
            Matcher::Equals(Value::Text(s)) => value == s,
            Matcher::Equals(Value::Integer(_)) => false, // rejected on construction
            Matcher::Includes(s) => value.contains(s.as_str()),
            Matcher::Matches(re) => re.is_match(value),
        }
    }

    fn is_integer_match(&self, value: i64) -> bool {
        // only `equals <i64>` is accepted for integer fields
        matches!(self, Matcher::Equals(Value::Integer(n)) if *n == value)
    }
}

/// A single condition. Serialized flat, with the matcher kind as the key:
///
/// ```json
/// { "field": "ref1", "equals": "foobar" }
/// { "field": "amount", "equals": -1250 }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RawCondition")]
pub struct Condition {
    pub field: Field,
    #[serde(flatten)]
    pub matcher: Matcher,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCondition {
    field: Field,
    equals: Option<Value>,
    includes: Option<String>,
    #[serde(default, with = "serde_regex")]
    matches: Option<Regex>,
}

impl TryFrom<RawCondition> for Condition {
    type Error = String;

    fn try_from(raw: RawCondition) -> Result<Self, Self::Error> {
        let matcher = match (raw.equals, raw.includes, raw.matches) {
            (Some(v), None, None) => Matcher::Equals(v),
            (None, Some(s), None) => Matcher::Includes(s),
            (None, None, Some(re)) => Matcher::Matches(re),
            _ => {
                return Err(
                    "condition needs exactly one of `equals`, `includes` or `matches`".into(),
                );
            }
        };
        Condition::new(raw.field, matcher)
    }
}

impl Condition {
    pub fn new(field: Field, matcher: Matcher) -> Result<Self, String> {
        match (field.is_integer(), &matcher) {
            (true, Matcher::Equals(Value::Integer(_)))
            | (false, Matcher::Equals(Value::Text(_))) => {}
            (true, _) => {
                return Err(format!(
                    "`{field}` only supports `equals` with an integer value"
                ));
            }
            (false, Matcher::Equals(Value::Integer(_))) => {
                return Err(format!("`{field}` equals expects a string value"));
            }
            (false, _) => {}
        }
        Ok(Condition { field, matcher })
    }

    fn is_match(&self, row: &Transaction) -> bool {
        match self.field {
            Field::Code => self.matcher.is_text_match(&row.code),
            Field::Description => self.matcher.is_text_match(&row.description),
            Field::Ref1 => self.matcher.is_text_match(&row.ref1),
            Field::Ref2 => self.matcher.is_text_match(&row.ref2),
            Field::Ref3 => self.matcher.is_text_match(&row.ref3),
            Field::Amount => match (row.credit, row.debit) {
                (Some(credit), _) => self.matcher.is_integer_match(credit),
                (None, Some(debit)) => self.matcher.is_integer_match(-debit),
                (None, None) => false,
            },
        }
    }
}

impl fmt::Display for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.matcher {
            Matcher::Equals(v) => write!(f, "{} = {v}", self.field),
            Matcher::Includes(s) => write!(f, "{} contains {s:?}", self.field),
            Matcher::Matches(re) => write!(f, "{} =~ /{}/", self.field, re.as_str()),
        }
    }
}

/// Rule matching criteria: every `and` condition must match, and — when `or`
/// is non-empty — at least one `or` condition must match. Criteria without any
/// conditions never match.
///
/// ```json
/// {
///     "and": [{ "field": "ref1", "equals": "foobar" }],
///     "or":  [{ "field": "code", "matches": "^42|56$" },
///             { "field": "amount", "equals": -1250 }]
/// }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Criteria {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub and: Vec<Condition>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub or: Vec<Condition>,
}

impl Criteria {
    pub fn single(condition: Condition) -> Self {
        Criteria {
            and: vec![condition],
            or: vec![],
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("criteria always serializes")
    }

    fn is_empty(&self) -> bool {
        self.and.is_empty() && self.or.is_empty()
    }

    fn is_match(&self, row: &Transaction) -> bool {
        if self.and.is_empty() {
            self.or.iter().any(|c| c.is_match(row))
        } else {
            let all_matches = self.and.iter().all(|c| c.is_match(row));

            if all_matches {
                self.or.is_empty() || self.or.iter().any(|c| c.is_match(row))
            } else {
                false
            }
        }
    }
}

impl fmt::Display for Criteria {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let join = |conds: &[Condition], sep: &str| {
            conds
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(sep)
        };
        if self.is_empty() {
            return f.write_str("(never matches)");
        }
        let and = join(&self.and, " AND ");
        let or = join(&self.or, " OR ");
        match (and.is_empty(), or.is_empty()) {
            (_, true) => f.write_str(&and),
            (true, false) => f.write_str(&or),
            (false, false) if self.or.len() == 1 => write!(f, "{and} AND {or}"),
            (false, false) => write!(f, "{and} AND ({or})"),
        }
    }
}

impl Rule {
    pub(crate) fn is_match(&self, row: &Transaction) -> bool {
        self.account_id.is_none_or(|id| id == row.account_id) && self.criteria.is_match(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TransactionBuilder;

    fn parse(json: &str) -> Result<Criteria, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn tx(code: &str, description: &str, ref1: &str, debit: Option<i64>) -> Transaction {
        TransactionBuilder::default()
            .date(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())
            .code(code.into())
            .description(description.into())
            .ref1(ref1.into())
            .debit(debit)
            .credit(None)
            .build()
            .unwrap()
    }

    #[test]
    fn parses_and_roundtrips_example_payload() {
        let json = r#"{"and":[{"field":"ref1","equals":"foobar"}],"or":[{"field":"code","matches":"^42|56$"},{"field":"description","includes":"x"}]}"#;
        let c = parse(json).unwrap();
        assert_eq!(c.and.len(), 1);
        assert_eq!(c.or.len(), 2);
        assert_eq!(c.to_json(), json);
    }

    #[test]
    fn empty_criteria_never_match() {
        for json in ["{}", r#"{"and":[],"or":[]}"#] {
            let c = parse(json).unwrap();
            assert!(!c.is_match(&tx("", "", "", None)));
            assert_eq!(c.to_string(), "(never matches)");
        }
    }

    #[test]
    fn single_condition_evaluates_same_in_and_or_or() {
        let and = parse(r#"{"and":[{"field":"code","equals":"x"}]}"#).unwrap();
        let or = parse(r#"{"or":[{"field":"code","equals":"x"}]}"#).unwrap();
        for code in ["x", "y"] {
            let row = tx(code, "", "", None);
            assert_eq!(and.is_match(&row), or.is_match(&row));
        }
    }

    #[test]
    fn rejects_bad_conditions() {
        // no matcher
        assert!(parse(r#"{"and":[{"field":"code"}]}"#).is_err());
        // two matchers
        assert!(parse(r#"{"and":[{"field":"code","equals":"a","includes":"b"}]}"#).is_err());
        // unknown key / field
        assert!(parse(r#"{"and":[{"field":"code","like":"a"}]}"#).is_err());
        assert!(parse(r#"{"and":[{"field":"nope","equals":"a"}]}"#).is_err());
        assert!(parse(r#"{"not":[]}"#).is_err());
        // invalid regex
        assert!(parse(r#"{"and":[{"field":"code","matches":"("}]}"#).is_err());
        // integer fields: only `equals` with an i64
        assert!(parse(r#"{"and":[{"field":"amount","equals":"-1250"}]}"#).is_err());
        assert!(parse(r#"{"and":[{"field":"amount","equals":12.5}]}"#).is_err());
        assert!(parse(r#"{"and":[{"field":"amount","matches":"^-12"}]}"#).is_err());
        assert!(parse(r#"{"and":[{"field":"amount","includes":"1"}]}"#).is_err());
        // account scoping is a rule column, not a condition field
        assert!(parse(r#"{"and":[{"field":"account","equals":1}]}"#).is_err());
        // text fields: `equals` takes a string
        assert!(parse(r#"{"and":[{"field":"code","equals":42}]}"#).is_err());
    }

    #[test]
    fn and_requires_all_or_requires_one() {
        let c = parse(
            r#"{"and":[{"field":"description","includes":"GRAB"}],
                "or":[{"field":"code","equals":"POS"},{"field":"code","equals":"MST"}]}"#,
        )
        .unwrap();
        assert!(c.is_match(&tx("POS", "GRAB*FOOD", "", None)));
        assert!(c.is_match(&tx("MST", "GRAB*FOOD", "", None)));
        assert!(!c.is_match(&tx("ICT", "GRAB*FOOD", "", None)));
        assert!(!c.is_match(&tx("POS", "FOODPANDA", "", None)));
    }

    #[test]
    fn amount_matching() {
        let eq = parse(r#"{"and":[{"field":"amount","equals":-1250}]}"#).unwrap();
        assert!(eq.is_match(&tx("", "", "", Some(1250))));
        assert!(!eq.is_match(&tx("", "", "", Some(12500))));
        assert_eq!(
            eq.to_json(),
            r#"{"and":[{"field":"amount","equals":-1250}]}"#
        );
    }

    #[test]
    fn or_group_searches_multiple_fields() {
        let c = parse(
            r#"{"or":[{"field":"description","matches":"(?i)netflix"},
                      {"field":"ref1","matches":"(?i)netflix"}]}"#,
        )
        .unwrap();
        assert!(c.is_match(&tx("", "", "NETFLIX.COM", None)));
        assert!(!c.is_match(&tx("", "SPOTIFY", "", None)));
        // `any` was replaced by `or` groups
        assert!(parse(r#"{"and":[{"field":"any","matches":"x"}]}"#).is_err());
    }

    #[test]
    fn display() {
        let c = parse(
            r#"{"and":[{"field":"ref1","equals":"foo"}],
                "or":[{"field":"code","matches":"^42$"},{"field":"amount","equals":-5}]}"#,
        )
        .unwrap();
        assert_eq!(
            c.to_string(),
            r#"ref1 = "foo" AND (code =~ /^42$/ OR amount = -5)"#
        );
    }
}
