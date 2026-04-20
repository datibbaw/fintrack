use regex::Regex;
use serde::{Deserialize, Serialize};
use tabled::Tabled;

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
    #[tabled(format = "{:?}")]
    pub field: Field,
    #[serde(with = "serde_regex")]
    pub pattern: Regex,
    pub priority: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    Code,
    Description,
    Ref1,
    Ref2,
    Ref3,
    Any,
}
