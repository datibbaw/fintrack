/// A bank account tracked in the system.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub number: String,
    pub bank: String,
    pub currency: String,
}

/// A spending / income category (optionally nested under a parent).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
}

/// A categorization rule: when `field` matches `pattern` (regex), assign `category_id`.
/// Higher `priority` wins when multiple rules match.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Rule {
    pub id: i64,
    pub category_id: i64,
    /// One of: description | ref1 | ref2 | ref3 | code | any
    pub field: String,
    pub pattern: String,
    pub priority: i64,
}
