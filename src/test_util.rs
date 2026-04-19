use crate::{db, models::Account};
use anyhow::Result;
use rusqlite::Connection;

pub(crate) fn create_account(conn: &Connection, number: &str, name: &str) -> Result<Account> {
    let bank = "DBS";
    let currency = "SGD";
    db::add_account(conn, name, number, bank, currency).map(|id| Account {
        id,
        name: name.to_string(),
        number: number.to_string(),
        bank: bank.to_string(),
        currency: currency.to_string(),
    })
}
