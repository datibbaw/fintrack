use rusty_money::{iso::Currency, Money};
use tabled::Tabled;

use crate::money::{CurrencyCode, HasCurrency};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Tabled)]
#[tabled(rename_all = "PascalCase")]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub number: String,
    pub bank: String,
    #[tabled(skip)]
    pub currency: CurrencyCode,
}

crate::impl_has_currency!(Account);

impl Account {
    pub fn currency_factor(&self) -> i64 {
        10i64.pow(self.currency().exponent)
    }

    pub fn amount_from_minor(&self, minor: i64) -> Money<'_, Currency> {
        Money::from_minor(minor, self.currency())
    }
}
