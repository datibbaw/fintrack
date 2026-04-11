use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};

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

#[derive(Serialize)]
pub struct CategoryDto {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub parent: Option<String>,
}

#[derive(Serialize)]
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

#[derive(Serialize)]
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

fn default_limit() -> i64 { 100 }

// ── DB helpers ────────────────────────────────────────────────────────────────

fn query_summary(
    conn: &rusqlite::Connection,
    p: &SummaryParams,
) -> anyhow::Result<SummaryResponse> {
    let (filter_clause, vals) = db::build_filters(
        p.from.as_deref(),
        p.to.as_deref(),
        p.account.as_deref(),
    );

    let sql = format!(
        "SELECT \
           c.id            AS category_id, \
           COALESCE(c.name, 'Uncategorized') AS category, \
           c.parent_id, \
           p.name          AS parent, \
           SUM(COALESCE(t.debit,  0)) AS total_debit, \
           SUM(COALESCE(t.credit, 0)) AS total_credit, \
           COUNT(*) AS tx_count \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         LEFT JOIN categories p ON c.parent_id = p.id \
         JOIN  accounts a ON t.account_id = a.id \
         WHERE 1=1{filter_clause} \
         GROUP BY c.id \
         ORDER BY ABS(total_credit - total_debit) DESC"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(vals.iter()), |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,   // category_id
                row.get::<_, String>(1)?,         // category
                row.get::<_, Option<i64>>(2)?,   // parent_id
                row.get::<_, Option<String>>(3)?, // parent
                row.get::<_, f64>(4)?,            // debit
                row.get::<_, f64>(5)?,            // credit
                row.get::<_, i64>(6)?,            // count
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut total_debit = 0f64;
    let mut total_credit = 0f64;
    let mut summary_rows = Vec::new();

    for (category_id, category, parent_id, parent, debit, credit, count) in rows {
        total_debit += debit;
        total_credit += credit;
        summary_rows.push(SummaryRow {
            category,
            category_id,
            parent,
            parent_id,
            debit,
            credit,
            net: credit - debit,
            count,
        });
    }

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
    let (mut filter_clause, mut vals) = db::build_filters(
        p.from.as_deref(),
        p.to.as_deref(),
        p.account.as_deref(),
    );

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
    let total: usize = conn.query_row(
        &count_sql,
        rusqlite::params_from_iter(vals.iter()),
        |row| row.get(0),
    )?;

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
    let rows = stmt
        .query_map(rusqlite::params_from_iter(paginated_vals.iter()), |row| {
            Ok(TransactionDto {
                id: row.get(0)?,
                date: row.get(1)?,
                code: row.get(2)?,
                description: row.get(3)?,
                ref1: row.get(4)?,
                ref2: row.get(5)?,
                ref3: row.get(6)?,
                status: row.get(7)?,
                debit: row.get(8)?,
                credit: row.get(9)?,
                category: row.get(10)?,
                category_id: row.get(11)?,
                account: row.get(12)?,
                account_id: row.get(13)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(TransactionsResponse { rows, total })
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
        let cats = db::list_categories(&conn)?;
        let dtos = cats
            .iter()
            .map(|c| {
                let parent = c.parent_id
                    .and_then(|pid| cats.iter().find(|p| p.id == pid))
                    .map(|p| p.name.clone());
                CategoryDto {
                    id: c.id,
                    name: c.name.clone(),
                    parent_id: c.parent_id,
                    parent,
                }
            })
            .collect();
        Ok(dtos)
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
        .route("/categories", get(api_categories))
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
    std::process::Command::new("cmd").args(["/c", "start", url]).spawn()?;
    Ok(())
}
