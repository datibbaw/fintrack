use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rusty_money::iso;
use serde::{Deserialize, Serialize};
use serde_rusqlite::from_rows;

use crate::db;

use super::{ApiError, Db};

#[derive(Serialize, Deserialize)]
pub struct AccountDto {
    pub id: i64,
    pub name: String,
    pub number: String,
    pub bank: String,
    pub currency: String,
    pub transaction_count: i64,
}

#[derive(Deserialize)]
pub struct CreateBody {
    pub name: String,
    pub number: String,
    pub bank: String,
    pub currency: String,
}

#[derive(Deserialize)]
pub struct UpdateBody {
    pub name: String,
    pub number: String,
    pub bank: String,
    pub currency: String,
}

fn validate_currency(currency: &str) -> Result<(), ApiError> {
    if iso::find(currency).is_none() {
        return Err(ApiError::unprocessable(format!("unknown currency: '{currency}'")));
    }
    Ok(())
}

fn query_list(conn: &rusqlite::Connection) -> anyhow::Result<Vec<AccountDto>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.name, a.number, a.bank, a.currency, \
                COUNT(t.id) AS transaction_count \
         FROM accounts a \
         LEFT JOIN transactions t ON t.account_id = a.id \
         GROUP BY a.id \
         ORDER BY a.id",
    )?;
    let rows =
        from_rows::<AccountDto>(stmt.query([])?).collect::<serde_rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub async fn index(State(db): State<Db>) -> Result<Json<Vec<AccountDto>>, ApiError> {
    let db = Arc::clone(&db);
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        query_list(&conn)
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(Json(result))
}

pub async fn create(
    State(db): State<Db>,
    Json(body): Json<CreateBody>,
) -> Result<(StatusCode, Json<AccountDto>), ApiError> {
    validate_currency(&body.currency)?;
    let db = Arc::clone(&db);
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        let id = db::add_account(&conn, &body.name, &body.number, &body.bank, &body.currency)?;
        Ok(AccountDto {
            id,
            name: body.name,
            number: body.number,
            bank: body.bank,
            currency: body.currency,
            transaction_count: 0,
        })
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok((StatusCode::CREATED, Json(result)))
}

pub async fn update(
    State(db): State<Db>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateBody>,
) -> Result<StatusCode, ApiError> {
    validate_currency(&body.currency)?;

    // Check currency-lock constraint: fetch current currency + tx count in one query.
    let db_check = Arc::clone(&db);
    let new_currency = body.currency.clone();
    let (current_currency, tx_count) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db_check.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        let acc = db::find_account_by_id(&conn, id)?
            .ok_or_else(|| anyhow!("account {} not found", id))?;
        let count = if acc.currency != new_currency {
            db::count_transactions_for_account(&conn, id)?
        } else {
            0
        };
        Ok((acc.currency, count))
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;

    if current_currency != body.currency && tx_count > 0 {
        return Err(ApiError::unprocessable(
            "currency cannot be changed while the account has transactions",
        ));
    }

    tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        db::update_account(&conn, id, &body.name, &body.number, &body.bank, &body.currency)
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn destroy(State(db): State<Db>, Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    let db = Arc::clone(&db);
    tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        db::remove_account(&conn, id)
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(StatusCode::NO_CONTENT)
}

static CURRENCY_CODES: &[&str] = &[
    "AED","AFN","ALL","AMD","ANG","AOA","ARS","AUD","AWG","AZN","BAM","BBD","BDT","BGN","BHD",
    "BIF","BMD","BND","BOB","BRL","BSD","BTN","BWP","BYN","BYR","BZD","CAD","CDF","CHF","CLF",
    "CLP","CNY","COP","CRC","CUC","CUP","CVE","CZK","DJF","DKK","DOP","DZD","EGP","ERN","ETB",
    "EUR","FJD","FKP","GBP","GEL","GHS","GIP","GMD","GNF","GTQ","GYD","HKD","HNL","HRK","HTG",
    "HUF","IDR","ILS","INR","IQD","IRR","ISK","JMD","JOD","JPY","KES","KGS","KHR","KMF","KPW",
    "KRW","KWD","KYD","KZT","LAK","LBP","LKR","LRD","LSL","LYD","MAD","MDL","MGA","MKD","MMK",
    "MNT","MOP","MRU","MUR","MVR","MWK","MXN","MYR","MZN","NAD","NGN","NIO","NOK","NPR","NZD",
    "OMR","PAB","PEN","PGK","PHP","PKR","PLN","PYG","QAR","ROL","RON","RSD","RUB","RWF","SAR",
    "SBD","SCR","SDG","SEK","SGD","SHP","SKK","SLE","SLL","SOS","SRD","SSP","STD","STN","SVC",
    "SYP","SZL","THB","TJS","TMT","TND","TOP","TRY","TTD","TWD","TZS","UAH","UGX","USD","UYU",
    "UYW","UZS","VED","VES","VND","VUV","WST","XAF","XAG","XAU","XBA","XBB","XBC","XBD","XCD",
    "XCG","XDR","XOF","XPD","XPF","XPT","XTS","YER","ZAR","ZMK","ZMW","ZWG","ZWL",
];

pub async fn currencies() -> Json<&'static [&'static str]> {
    Json(CURRENCY_CODES)
}
