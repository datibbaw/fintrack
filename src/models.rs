use tabled::Tabled;

/// A bank account tracked in the system.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Tabled)]
#[tabled(rename_all = "PascalCase")]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub number: String,
    pub bank: String,
    pub currency: String,
}

/// A spending / income category (optionally nested under a parent).
#[derive(Debug, Clone, serde::Deserialize, Tabled)]
#[tabled(rename_all = "PascalCase")]
pub struct Category {
    pub id: i64,
    pub name: String,
    #[tabled(skip)]
    pub parent_id: Option<i64>,
    #[tabled(display = "opt_string", rename = "Parent")]
    pub parent_name: Option<String>,
}

fn opt_string(v: &Option<String>) -> String {
    v.as_deref().unwrap_or("-").to_string()
}

/// A categorization rule: when `field` matches `pattern` (regex), assign `category_id`.
/// Higher `priority` wins when multiple rules match.
#[derive(Debug, Clone, serde::Deserialize, Tabled)]
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
    /// One of: description | ref1 | ref2 | ref3 | code | any
    pub field: String,
    pub pattern: String,
    pub priority: i64,
}
