use anyhow::Result;
use rusqlite::{functions::FunctionFlags, params, Connection};
use rusqlite_migration::{Migrations, M};
use rusty_money::iso::Currency;
use serde_rusqlite::{from_row, from_rows};
use sha2::{Digest, Sha256};

use crate::models::{Account, Category, Rule};

pub fn open(path: &str) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    register_functions(&conn)?;
    migrations().to_latest(&mut conn)?;
    // rusqlite_migration temporarily disables foreign keys; re-enable after migration.
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}

fn register_functions(conn: &Connection) -> Result<()> {
    conn.create_scalar_function(
        "fintrack_sha256",
        1,
        FunctionFlags::SQLITE_DETERMINISTIC | FunctionFlags::SQLITE_INNOCUOUS,
        |ctx: &rusqlite::functions::Context<'_>| {
            let input: String = ctx.get(0)?;
            let mut h = Sha256::new();
            h.update(input.as_bytes());
            Ok(hex::encode(h.finalize()))
        },
    )?;
    Ok(())
}

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("migrations/01_initial_schema.sql")),
        M::up(include_str!("migrations/02_amounts_to_cents.sql")),
        M::up(include_str!("migrations/03_rehash_transactions.sql")),
    ])
}

// ── Query helpers ─────────────────────────────────────────────────────────────

/// Build a WHERE clause fragment and bind values for date filters.
pub fn build_filters(
    from: Option<&str>,
    to: Option<&str>,
) -> (String, Vec<String>) {
    let mut clauses = vec![];
    let mut vals: Vec<String> = vec![];

    if let Some(f) = from {
        clauses.push("t.date >= ?".to_string());
        vals.push(f.to_string());
    }
    if let Some(t) = to {
        clauses.push("t.date <= ?".to_string());
        vals.push(t.to_string());
    }

    let clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" AND {}", clauses.join(" AND "))
    };
    (clause, vals)
}

// ── Accounts ─────────────────────────────────────────────────────────────────

pub fn add_account(
    conn: &Connection,
    name: &str,
    number: &str,
    bank: &str,
    currency: &Currency,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO accounts (name, number, bank, currency) VALUES (?1, ?2, ?3, ?4)",
        params![name, number, bank, currency.iso_alpha_code],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_accounts(conn: &Connection) -> Result<Vec<Account>> {
    let mut stmt =
        conn.prepare("SELECT id, name, number, bank, currency FROM accounts ORDER BY id")?;
    let rows = from_rows::<Account>(stmt.query([])?).collect::<serde_rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn find_account(conn: &Connection, number: &str) -> Result<Option<Account>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, number, bank, currency \
         FROM accounts WHERE number = ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query(params![number])?;
    Ok(match rows.next()? {
        Some(row) => Some(from_row::<Account>(row)?),
        None => None,
    })
}

pub fn remove_account(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM transactions WHERE account_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
    Ok(())
}

// ── Categories ────────────────────────────────────────────────────────────────

pub fn add_category(conn: &Connection, name: &str, parent_id: Option<i64>) -> Result<i64> {
    conn.execute(
        "INSERT INTO categories (name, parent_id) VALUES (?1, ?2)",
        params![name, parent_id],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_categories(conn: &Connection) -> Result<Vec<Category>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, c.parent_id, p.name AS parent_name \
         FROM categories c \
         LEFT JOIN categories p ON c.parent_id = p.id \
         ORDER BY c.parent_id NULLS FIRST, c.name",
    )?;
    let rows =
        from_rows::<Category>(stmt.query([])?).collect::<serde_rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn find_category(conn: &Connection, name: &str) -> Result<Option<Category>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, c.parent_id, p.name AS parent_name \
                      FROM categories c \
                      LEFT JOIN categories p ON c.parent_id = p.id \
                      WHERE c.name = ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query(params![name])?;
    Ok(match rows.next()? {
        Some(row) => Some(from_row::<Category>(row)?),
        None => None,
    })
}

pub fn update_category(
    conn: &Connection,
    id: i64,
    name: &str,
    parent_id: Option<i64>,
) -> Result<()> {
    conn.execute(
        "UPDATE categories SET name = ?1, parent_id = ?2 WHERE id = ?3",
        params![name, parent_id, id],
    )?;
    Ok(())
}

pub fn remove_category(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM categories WHERE id = ?1", params![id])?;
    Ok(())
}

// ── Rules ─────────────────────────────────────────────────────────────────────

pub fn add_rule(
    conn: &Connection,
    category_id: i64,
    field: &str,
    pattern: &str,
    priority: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO rules (category_id, field, pattern, priority) VALUES (?1, ?2, ?3, ?4)",
        params![category_id, field, pattern, priority],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_rules_for_category(conn: &Connection, category_id: i64) -> Result<Vec<Rule>> {
    let mut stmt = conn.prepare(
        "SELECT id, category_id, field, pattern, priority \
         FROM rules WHERE category_id = ?1 ORDER BY priority DESC, id",
    )?;
    let rows = from_rows::<Rule>(stmt.query(params![category_id])?)
        .collect::<serde_rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list_rules(conn: &Connection, category_name: Option<&str>) -> Result<Vec<Rule>> {
    let base = "SELECT r.id, r.category_id, r.field, r.pattern, r.priority, \
                       c.name AS category_name, c.parent_id IS NOT NULL AS category_is_sub \
                FROM rules r JOIN categories c ON r.category_id = c.id";
    let (sql, params) = if let Some(cat) = category_name {
        (
            format!("{base} WHERE c.name = ?1 ORDER BY r.priority DESC, r.id"),
            params![cat.to_string()],
        )
    } else {
        (
            format!("{base} ORDER BY c.name, r.priority DESC, r.id"),
            params![],
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows =
        from_rows::<Rule>(stmt.query(params)?).collect::<serde_rusqlite::Result<Vec<_>>>()?;

    Ok(rows)
}

pub fn remove_rule(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM rules WHERE id = ?1", params![id])?;
    Ok(())
}

// ── Transactions ──────────────────────────────────────────────────────────────

pub fn count_transactions_for_account(conn: &Connection, account_id: i64) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE account_id = ?1",
        params![account_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

pub fn delete_transactions_for_account(conn: &Connection, account_id: i64) -> Result<usize> {
    let n = conn.execute(
        "DELETE FROM transactions WHERE account_id = ?1",
        params![account_id],
    )?;
    Ok(n)
}

/// Returns all rules, joined with their category to expose whether each rule belongs
/// to a sub-category.
/// Used by the categorisation engine to break priority ties in favour of more specific rules.
pub fn all_rules_with_depth(conn: &Connection) -> Result<Vec<Rule>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.category_id, r.field, r.pattern, r.priority, \
                c.parent_id IS NOT NULL AS category_is_sub \
         FROM rules r \
         JOIN categories c ON r.category_id = c.id \
         ORDER BY r.priority DESC, r.id",
    )?;
    let rows = from_rows::<Rule>(stmt.query([])?).collect::<serde_rusqlite::Result<Vec<_>>>()?;

    Ok(rows)
}
