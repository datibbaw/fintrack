use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_rusqlite::from_rows;

use crate::{
    db,
    models::{Field, Rule},
};

use super::{ApiError, Db};

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
    pub field: Field,
    pub pattern: String,
    pub priority: i64,
}

impl From<Rule> for RuleDto {
    fn from(rule: Rule) -> Self {
        RuleDto {
            id: rule.id,
            category_id: rule.category_id,
            field: rule.field,
            pattern: rule.pattern.as_str().to_string(),
            priority: rule.priority,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateBody {
    pub name: String,
    pub parent_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct UpdateBody {
    pub name: String,
    pub parent_id: Option<i64>,
}

fn query_list(conn: &rusqlite::Connection) -> anyhow::Result<Vec<CategoryDto>> {
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
    let rows =
        from_rows::<CategoryDto>(stmt.query([])?).collect::<serde_rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub async fn index(State(db): State<Db>) -> Result<Json<Vec<CategoryDto>>, ApiError> {
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

pub async fn update(
    State(db): State<Db>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateBody>,
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

pub async fn destroy(
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

pub async fn rules(
    State(db): State<Db>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<RuleDto>>, ApiError> {
    let db = Arc::clone(&db);
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = db.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        let rules = db::list_rules_for_category(&conn, id)?;
        Ok(rules.into_iter().map(Into::into).collect::<Vec<_>>())
    })
    .await
    .map_err(|e| anyhow!("thread error: {e}"))??;
    Ok(Json(result))
}
