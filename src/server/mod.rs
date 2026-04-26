use std::sync::{Arc, Mutex};

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, put},
    Router,
};
use rusqlite::Connection;
use rust_embed::RustEmbed;

mod accounts;
mod categories;
mod transactions;

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

pub async fn serve(conn: Connection, port: u16, open: bool) -> anyhow::Result<()> {
    let state: Db = Arc::new(Mutex::new(conn));

    let api = Router::new()
        .route("/accounts", get(accounts::index))
        .route(
            "/categories",
            get(categories::index).post(categories::create),
        )
        .route(
            "/categories/:id",
            put(categories::update).delete(categories::destroy),
        )
        .route("/categories/:id/rules", get(categories::rules))
        .route("/summary", get(transactions::summary))
        .route("/transactions", get(transactions::index));

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
