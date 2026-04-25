use std::sync::Arc;

use anyhow::anyhow;
use axum::{extract::State, Json};

use crate::{db, models::Account};

use super::{ApiError, Db};

pub async fn index(State(db): State<Db>) -> Result<Json<Vec<Account>>, ApiError> {
    let db = Arc::clone(&db);
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        db::list_accounts(&conn)
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(Json(result))
}
