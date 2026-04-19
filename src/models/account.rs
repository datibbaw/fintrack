use tabled::Tabled;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Tabled)]
#[tabled(rename_all = "PascalCase")]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub number: String,
    pub bank: String,
    pub currency: String,
}
