use std::ops::Deref;

use rusty_money::{Money, iso::{self, Currency}};
use serde::{Deserialize, Deserializer, Serialize, de};

pub fn display_amount<A: IntoMinorAmount, R: HasCurrency>(amount: &A, record: &R) -> String {
    amount.to_minor()
        .map(|a| Money::from_minor(a, record.currency()).to_string())
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct CurrencyCode(pub &'static Currency);

impl Serialize for CurrencyCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.iso_alpha_code.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CurrencyCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        iso::find(&s)
            .map(CurrencyCode)
            .ok_or_else(|| de::Error::custom(format!("unknown currency code: {s}")))
    }
}

impl Deref for CurrencyCode {
    type Target = Currency;
    fn deref(&self) -> &Currency {
        self.0
    }
}

pub trait IntoMinorAmount {
    fn to_minor(&self) -> Option<i64>;
}

impl IntoMinorAmount for i64 {
    fn to_minor(&self) -> Option<i64> {
        Some(*self)
    }
}

impl IntoMinorAmount for Option<i64> {
    fn to_minor(&self) -> Option<i64> {
        *self
    }
}

pub trait HasCurrency {
    fn currency(&self) -> &'static iso::Currency;
}

impl<T: HasCurrency> HasCurrency for &T {
    fn currency(&self) -> &'static iso::Currency {
        (*self).currency()
    }
}

/// Implement `HasCurrency` for any struct with a `currency: CurrencyCode` field.
#[macro_export]
macro_rules! impl_has_currency {
    ($t:ty) => {
        impl $crate::money::HasCurrency for $t {
            fn currency(&self) -> &'static ::rusty_money::iso::Currency {
                self.currency.0
            }
        }
    };
}
