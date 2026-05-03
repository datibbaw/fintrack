use crate::{db, models::Account, money::CurrencyCode};
use anyhow::{anyhow, Result};
use rusqlite::Connection;
use rusty_money::iso;

pub(crate) fn create_account(conn: &Connection, number: &str, name: &str) -> Result<Account> {
    create_account_with_currency(conn, number, name, "SGD")
}

pub(crate) fn create_account_with_currency(
    conn: &Connection,
    number: &str,
    name: &str,
    currency: &str,
) -> Result<Account> {
    let currency = iso::find(currency)
        .ok_or_else(|| anyhow!("unknown currency: '{}'", currency))?;
    db::add_account(conn, name, number, "DBS", currency).map(|id| Account {
        id,
        name: name.to_string(),
        number: number.to_string(),
        bank: "DBS".to_string(),
        currency: CurrencyCode(currency),
    })
}
