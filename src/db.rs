use anyhow::Result;
use rusqlite::{params, Connection};

use crate::models::{Account, Category, Rule};

pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS accounts (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            name     TEXT    NOT NULL,
            number   TEXT    NOT NULL UNIQUE,
            bank     TEXT    NOT NULL DEFAULT 'DBS',
            currency TEXT    NOT NULL DEFAULT 'SGD'
        );

        CREATE TABLE IF NOT EXISTS categories (
            id        INTEGER PRIMARY KEY AUTOINCREMENT,
            name      TEXT    NOT NULL UNIQUE,
            parent_id INTEGER REFERENCES categories(id) ON DELETE SET NULL
        );

        CREATE TABLE IF NOT EXISTS transactions (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id  INTEGER NOT NULL REFERENCES accounts(id),
            date        TEXT    NOT NULL,
            code        TEXT    NOT NULL DEFAULT '',
            description TEXT    NOT NULL DEFAULT '',
            ref1        TEXT    NOT NULL DEFAULT '',
            ref2        TEXT    NOT NULL DEFAULT '',
            ref3        TEXT    NOT NULL DEFAULT '',
            status      TEXT    NOT NULL DEFAULT '',
            debit       REAL,
            credit      REAL,
            hash        TEXT    NOT NULL UNIQUE,
            category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL
        );

        CREATE TABLE IF NOT EXISTS rules (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
            field       TEXT    NOT NULL DEFAULT 'any',
            pattern     TEXT    NOT NULL,
            priority    INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_tx_date     ON transactions(date);
        CREATE INDEX IF NOT EXISTS idx_tx_account  ON transactions(account_id);
        CREATE INDEX IF NOT EXISTS idx_tx_category ON transactions(category_id);
        CREATE INDEX IF NOT EXISTS idx_tx_hash     ON transactions(hash);
    ")?;
    Ok(())
}

// ── Query helpers ─────────────────────────────────────────────────────────────

/// Build a WHERE clause fragment and bind values for date + account filters.
pub fn build_filters(
    from: Option<&str>,
    to: Option<&str>,
    account: Option<&str>,
) -> (String, Vec<String>) {
    let mut clauses = Vec::new();
    let mut vals: Vec<String> = Vec::new();

    if let Some(f) = from {
        clauses.push("t.date >= ?".to_string());
        vals.push(f.to_string());
    }
    if let Some(t) = to {
        clauses.push("t.date <= ?".to_string());
        vals.push(t.to_string());
    }
    if let Some(acc) = account {
        if !acc.is_empty() {
            clauses.push("(a.number = ? OR a.name = ?)".to_string());
            vals.push(acc.to_string());
            vals.push(acc.to_string());
        }
    }

    let clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" AND {}", clauses.join(" AND "))
    };
    (clause, vals)
}

// ── Accounts ─────────────────────────────────────────────────────────────────

pub fn add_account(conn: &Connection, name: &str, number: &str, bank: &str, currency: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO accounts (name, number, bank, currency) VALUES (?1, ?2, ?3, ?4)",
        params![name, number, bank, currency],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_accounts(conn: &Connection) -> Result<Vec<Account>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, number, bank, currency FROM accounts ORDER BY id"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Account {
            id: row.get(0)?,
            name: row.get(1)?,
            number: row.get(2)?,
            bank: row.get(3)?,
            currency: row.get(4)?,
        })
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn find_account(conn: &Connection, number_or_name: &str) -> Result<Option<Account>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, number, bank, currency \
         FROM accounts WHERE number = ?1 OR name = ?1 LIMIT 1"
    )?;
    let mut rows = stmt.query(params![number_or_name])?;
    Ok(if let Some(row) = rows.next()? {
        Some(Account {
            id: row.get(0)?,
            name: row.get(1)?,
            number: row.get(2)?,
            bank: row.get(3)?,
            currency: row.get(4)?,
        })
    } else {
        None
    })
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
        "SELECT id, name, parent_id FROM categories ORDER BY parent_id NULLS FIRST, name"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Category {
            id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
        })
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn find_category(conn: &Connection, name: &str) -> Result<Option<Category>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, parent_id FROM categories WHERE name = ?1 LIMIT 1"
    )?;
    let mut rows = stmt.query(params![name])?;
    Ok(if let Some(row) = rows.next()? {
        Some(Category {
            id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
        })
    } else {
        None
    })
}

pub fn update_category(conn: &Connection, id: i64, name: &str, parent_id: Option<i64>) -> Result<()> {
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

pub fn add_rule(conn: &Connection, category_id: i64, field: &str, pattern: &str, priority: i64) -> Result<i64> {
    conn.execute(
        "INSERT INTO rules (category_id, field, pattern, priority) VALUES (?1, ?2, ?3, ?4)",
        params![category_id, field, pattern, priority],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_rules_for_category(conn: &Connection, category_id: i64) -> Result<Vec<Rule>> {
    let mut stmt = conn.prepare(
        "SELECT id, category_id, field, pattern, priority \
         FROM rules WHERE category_id = ?1 ORDER BY priority DESC, id"
    )?;
    let rows = stmt.query_map(params![category_id], |row| {
        Ok(Rule {
            id: row.get(0)?,
            category_id: row.get(1)?,
            field: row.get(2)?,
            pattern: row.get(3)?,
            priority: row.get(4)?,
        })
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list_rules(conn: &Connection, category_name: Option<&str>) -> Result<Vec<(Rule, String)>> {
    let mut out = Vec::new();
    if let Some(cat) = category_name {
        let mut stmt = conn.prepare(
            "SELECT r.id, r.category_id, r.field, r.pattern, r.priority, c.name \
             FROM rules r JOIN categories c ON r.category_id = c.id \
             WHERE c.name = ?1 ORDER BY r.priority DESC, r.id"
        )?;
        for row in stmt.query_map(params![cat], map_rule)? {
            out.push(row?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT r.id, r.category_id, r.field, r.pattern, r.priority, c.name \
             FROM rules r JOIN categories c ON r.category_id = c.id \
             ORDER BY c.name, r.priority DESC, r.id"
        )?;
        for row in stmt.query_map([], map_rule)? {
            out.push(row?);
        }
    }
    Ok(out)
}

fn map_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Rule, String)> {
    Ok((
        Rule {
            id: row.get(0)?,
            category_id: row.get(1)?,
            field: row.get(2)?,
            pattern: row.get(3)?,
            priority: row.get(4)?,
        },
        row.get(5)?,
    ))
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

pub fn all_rules(conn: &Connection) -> Result<Vec<Rule>> {
    let mut stmt = conn.prepare(
        "SELECT id, category_id, field, pattern, priority \
         FROM rules ORDER BY priority DESC, id"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Rule {
            id: row.get(0)?,
            category_id: row.get(1)?,
            field: row.get(2)?,
            pattern: row.get(3)?,
            priority: row.get(4)?,
        })
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
