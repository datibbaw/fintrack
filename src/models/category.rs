use super::opt_string;
use tabled::Tabled;

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
