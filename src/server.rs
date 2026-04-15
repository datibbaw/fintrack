use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};

use serde_rusqlite::from_rows;

use crate::{db, models::Account};

// ── Embedded web assets ───────────────────────────────────────────────────────

#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Assets;

// ── Shared state ──────────────────────────────────────────────────────────────

type Db = Arc<Mutex<rusqlite::Connection>>;

// ── Error type ────────────────────────────────────────────────────────────────

struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(e.into())
    }
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct CategoryDto {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub parent: Option<String>,
    pub transaction_count: i64,
    pub rule_count: i64,
}

#[derive(Serialize)]
pub struct RuleDto {
    pub id: i64,
    pub category_id: i64,
    pub field: String,
    pub pattern: String,
    pub priority: i64,
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

// ── Request bodies ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateCategoryBody {
    pub name: String,
    pub parent_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct UpdateCategoryBody {
    pub name: String,
    pub parent_id: Option<i64>,
}

// ── Query params ──────────────────────────────────────────────────────────────

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

// ── DB helpers ────────────────────────────────────────────────────────────────

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
    let summary_rows = from_rows::<SummaryRow>(
        stmt.query(rusqlite::params_from_iter(double_vals.iter()))?,
    )
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
    let rows = from_rows::<TransactionDto>(
        stmt.query(rusqlite::params_from_iter(paginated_vals.iter()))?,
    )
    .collect::<serde_rusqlite::Result<Vec<_>>>()?;

    Ok(TransactionsResponse { rows, total })
}

// ── Category query ────────────────────────────────────────────────────────────

fn query_categories(conn: &rusqlite::Connection) -> anyhow::Result<Vec<CategoryDto>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, c.parent_id, p.name AS parent, \
                COUNT(DISTINCT t.id) AS transaction_count, \
                COUNT(DISTINCT r.id) AS rule_count \
         FROM categories c \
         LEFT JOIN categories p ON c.parent_id = p.id \
         LEFT JOIN transactions t ON t.category_id = c.id \
         LEFT JOIN rules r ON r.category_id = c.id \
         GROUP BY c.id \
         ORDER BY c.parent_id NULLS FIRST, c.name",
    )?;
    let rows = from_rows::<CategoryDto>(stmt.query([])?)
        .collect::<serde_rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ── API handlers ──────────────────────────────────────────────────────────────

async fn api_accounts(State(db): State<Db>) -> Result<Json<Vec<Account>>, ApiError> {
    let db = Arc::clone(&db);
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        db::list_accounts(&conn)
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(Json(result))
}

async fn api_categories(State(db): State<Db>) -> Result<Json<Vec<CategoryDto>>, ApiError> {
    let db = Arc::clone(&db);
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        query_categories(&conn)
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(Json(result))
}

async fn api_create_category(
    State(db): State<Db>,
    Json(body): Json<CreateCategoryBody>,
) -> Result<Json<CategoryDto>, ApiError> {
    let db = Arc::clone(&db);
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        let id = db::add_category(&conn, &body.name, body.parent_id)?;
        let cats = db::list_categories(&conn)?;
        let cat = cats
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| anyhow!("category not found after insert"))?;
        let parent = cat
            .parent_id
            .and_then(|pid| cats.iter().find(|p| p.id == pid))
            .map(|p| p.name.clone());
        Ok(CategoryDto {
            id: cat.id,
            name: cat.name.clone(),
            parent_id: cat.parent_id,
            parent,
            transaction_count: 0,
            rule_count: 0,
        })
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(Json(result))
}

async fn api_update_category(
    State(db): State<Db>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateCategoryBody>,
) -> Result<StatusCode, ApiError> {
    let db = Arc::clone(&db);
    tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        db::update_category(&conn, id, &body.name, body.parent_id)
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(StatusCode::NO_CONTENT)
}

async fn api_delete_category(
    State(db): State<Db>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let db = Arc::clone(&db);
    tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        db::remove_category(&conn, id)
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(StatusCode::NO_CONTENT)
}

async fn api_category_rules(
    State(db): State<Db>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<RuleDto>>, ApiError> {
    let db = Arc::clone(&db);
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        let rules = db::list_rules_for_category(&conn, id)?;
        Ok(rules
            .into_iter()
            .map(|r| RuleDto {
                id: r.id,
                category_id: r.category_id,
                field: r.field,
                pattern: r.pattern,
                priority: r.priority,
            })
            .collect::<Vec<_>>())
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(Json(result))
}

async fn api_summary(
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

async fn api_transactions(
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

// ── Static file handler ───────────────────────────────────────────────────────

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    serve_asset(path)
}

fn serve_asset(path: &str) -> Response {
    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => {
            // SPA fallback: serve index.html for any unmatched path
            match Assets::get("index.html") {
                Some(content) => {
                    let mime = mime_guess::from_path("index.html").first_or_octet_stream();
                    ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
                }
                None => (StatusCode::NOT_FOUND, "Not found").into_response(),
            }
        }
    }
}

// ── Server entry point ────────────────────────────────────────────────────────

pub async fn serve(db_path: &str, port: u16, open: bool) -> anyhow::Result<()> {
    let conn = crate::db::open(db_path)?;
    let state: Db = Arc::new(Mutex::new(conn));

    let api = Router::new()
        .route("/accounts", get(api_accounts))
        .route("/categories", get(api_categories).post(api_create_category))
        .route(
            "/categories/:id",
            put(api_update_category).delete(api_delete_category),
        )
        .route("/categories/:id/rules", get(api_category_rules))
        .route("/summary", get(api_summary))
        .route("/transactions", get(api_transactions));

    let app = Router::new()
        .nest("/api", api)
        .fallback(static_handler)
        .with_state(state);

    let addr = format!("127.0.0.1:{port}");
    let url = format!("http://{addr}");
    println!("fintrack server running at {url}");
    println!("Press Ctrl+C to stop.");

    if open {
        // Best-effort browser open; ignore errors
        let _ = open_browser(&url);
    }

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn open_browser(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn()?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd")
        .args(["/c", "start", url])
        .spawn()?;
    Ok(())
}
