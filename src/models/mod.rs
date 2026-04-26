mod account;
mod category;
mod rule;
mod transaction;

pub(crate) use account::Account;
pub(crate) use category::Category;
pub(crate) use rule::{Field, Rule};
pub(crate) use transaction::{Transaction, TransactionBuilder};

fn opt_string(v: &Option<String>) -> String {
    v.as_deref().unwrap_or("-").to_string()
}
