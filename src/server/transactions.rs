use std::sync::Arc;

use anyhow::anyhow;
use axum::{extract::{Query, State}, Json};
use serde::{Deserialize, Serialize};
use serde_rusqlite::from_rows;

use crate::db;

use super::{ApiError, Db};

#[derive(Serialize, Deserialize)]
pub struct TransactionDto {
    pub id: i64,
    pub date: String,
    pub code: String,
    pub description: String,
    pub ref1: String,
    pub ref2: String,
    pub ref3: String,
    pub status: String,
    pub debit: Option<f64>,
    pub credit: Option<f64>,
    pub category: Option<String>,
    pub category_id: Option<i64>,
    pub account: String,
    pub account_id: i64,
}

#[derive(Serialize)]
pub struct TransactionsResponse {
    pub rows: Vec<TransactionDto>,
    pub total: usize,
}

#[derive(Serialize, Deserialize)]
pub struct SummaryRow {
    pub category: String,
    pub category_id: Option<i64>,
    pub parent: Option<String>,
    pub parent_id: Option<i64>,
    pub debit: f64,
    pub credit: f64,
    pub net: f64,
    pub count: i64,
}

#[derive(Serialize)]
pub struct SummaryResponse {
    pub rows: Vec<SummaryRow>,
    pub total_debit: f64,
    pub total_credit: f64,
    pub total_net: f64,
}

#[derive(Deserialize, Default)]
pub struct SummaryParams {
    pub from: Option<String>,
    pub to: Option<String>,
    pub account: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct TransactionsParams {
    pub from: Option<String>,
    pub to: Option<String>,
    pub category: Option<String>,
    pub account: Option<String>,
    pub uncategorized: Option<bool>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    100
}

fn query_summary(
    conn: &rusqlite::Connection,
    p: &SummaryParams,
) -> anyhow::Result<SummaryResponse> {
    let (filter_clause, vals) =
        db::build_filters(p.from.as_deref(), p.to.as_deref(), p.account.as_deref());

    // Grand totals: simple sum with no rollup so each transaction is counted once.
    let totals_sql = format!(
        "SELECT COALESCE(SUM(t.debit),0), COALESCE(SUM(t.credit),0) \
         FROM transactions t \
         JOIN accounts a ON t.account_id = a.id \
         WHERE 1=1{filter_clause}"
    );
    let (total_debit, total_credit): (f64, f64) = conn.query_row(
        &totals_sql,
        rusqlite::params_from_iter(vals.iter()),
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    // Per-category rows with parent rollup.
    //
    // Each sub-category transaction contributes to two buckets via UNION ALL:
    //   1. Its own category  (appears in the sub-category row)
    //   2. Its parent        (rolled up into the parent row)
    //
    // Top-level and uncategorised transactions contribute to only one bucket.
    // Grand totals are computed above from raw transactions to avoid double-counting.
    let sql = format!(
        "SELECT sub.cat_id AS category_id, \
                COALESCE(c.name, 'Uncategorized') AS category, \
                c.parent_id, p.name AS parent, \
                SUM(sub.d)                    AS debit, \
                SUM(sub.cr)                   AS credit, \
                SUM(sub.cr) - SUM(sub.d)      AS net, \
                SUM(sub.cnt)                  AS count \
         FROM ( \
           SELECT t.category_id AS cat_id, \
                  COALESCE(t.debit,0)  AS d, \
                  COALESCE(t.credit,0) AS cr, \
                  1 AS cnt \
           FROM transactions t \
           JOIN accounts a ON t.account_id = a.id \
           WHERE 1=1{filter_clause} \
           UNION ALL \
           SELECT c2.parent_id AS cat_id, \
                  COALESCE(t.debit,0)  AS d, \
                  COALESCE(t.credit,0) AS cr, \
                  1 AS cnt \
           FROM transactions t \
           JOIN categories c2 ON t.category_id = c2.id AND c2.parent_id IS NOT NULL \
           JOIN accounts a ON t.account_id = a.id \
           WHERE 1=1{filter_clause} \
         ) sub \
         LEFT JOIN categories c ON sub.cat_id = c.id \
         LEFT JOIN categories p ON c.parent_id = p.id \
         GROUP BY sub.cat_id \
         ORDER BY ABS(net) DESC"
    );

    // filter vals are used in both sub-queries of the UNION ALL.
    let mut double_vals = vals.clone();
    double_vals.extend(vals.iter().cloned());

    let mut stmt = conn.prepare(&sql)?;
    let summary_rows =
        from_rows::<SummaryRow>(stmt.query(rusqlite::params_from_iter(double_vals.iter()))?)
            .collect::<serde_rusqlite::Result<Vec<_>>>()?;

    Ok(SummaryResponse {
        rows: summary_rows,
        total_debit,
        total_credit,
        total_net: total_credit - total_debit,
    })
}

fn query_transactions(
    conn: &rusqlite::Connection,
    p: &TransactionsParams,
) -> anyhow::Result<TransactionsResponse> {
    let (mut filter_clause, mut vals) =
        db::build_filters(p.from.as_deref(), p.to.as_deref(), p.account.as_deref());

    if p.uncategorized == Some(true) {
        filter_clause.push_str(" AND t.category_id IS NULL");
    } else if let Some(cat) = &p.category {
        if !cat.is_empty() {
            filter_clause.push_str(" AND c.name = ?");
            vals.push(cat.clone());
        }
    }

    // Count query (same filters, no pagination)
    let count_sql = format!(
        "SELECT COUNT(*) \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         JOIN  accounts a ON t.account_id = a.id \
         WHERE 1=1{filter_clause}"
    );
    let total: usize =
        conn.query_row(&count_sql, rusqlite::params_from_iter(vals.iter()), |row| {
            row.get(0)
        })?;

    // Data query with pagination
    let mut paginated_vals = vals.clone();
    paginated_vals.push(p.limit.to_string());
    paginated_vals.push(p.offset.to_string());

    let sql = format!(
        "SELECT \
           t.id, t.date, t.code, t.description, t.ref1, t.ref2, t.ref3, t.status, \
           t.debit, t.credit, \
           c.name AS category, t.category_id, \
           a.name AS account, t.account_id \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         JOIN  accounts a ON t.account_id = a.id \
         WHERE 1=1{filter_clause} \
         ORDER BY t.date DESC, t.id DESC \
         LIMIT ? OFFSET ?"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows =
        from_rows::<TransactionDto>(stmt.query(rusqlite::params_from_iter(paginated_vals.iter()))?)
            .collect::<serde_rusqlite::Result<Vec<_>>>()?;

    Ok(TransactionsResponse { rows, total })
}

pub async fn summary(
    State(db): State<Db>,
    Query(params): Query<SummaryParams>,
) -> Result<Json<SummaryResponse>, ApiError> {
    let db = Arc::clone(&db);
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        query_summary(&conn, &params)
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(Json(result))
}

pub async fn index(
    State(db): State<Db>,
    Query(params): Query<TransactionsParams>,
) -> Result<Json<TransactionsResponse>, ApiError> {
    let db = Arc::clone(&db);
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        query_transactions(&conn, &params)
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(Json(result))
}
