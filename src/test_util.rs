use crate::{db, models::Account};
use anyhow::Result;
use rusqlite::Connection;

pub(crate) fn create_account(conn: &Connection, number: &str, name: &str) -> Result<Account> {
    create_account_with_currency(conn, number, name, "SGD")
}

pub(crate) fn create_account_with_currency(
    conn: &Connection,
    number: &str,
    name: &str,
    currency: &str,
) -> Result<Account> {
    db::add_account(conn, name, number, "DBS", currency).map(|id| Account {
        id,
        name: name.to_string(),
        number: number.to_string(),
        bank: "DBS".to_string(),
        currency: currency.to_string(),
    })
}
