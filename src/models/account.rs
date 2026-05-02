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

impl Account {
    pub fn iso_currency(&self) -> Option<&'static rusty_money::iso::Currency> {
        rusty_money::iso::find(self.currency.as_str())
    }

    pub fn currency_factor(&self) -> i64 {
        self.iso_currency().map(|c| 10_i64.pow(c.exponent)).unwrap_or(100)
    }
}
