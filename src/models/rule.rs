use tabled::Tabled;

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
