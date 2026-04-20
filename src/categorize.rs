use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_rusqlite::from_rows;

use crate::{
    db,
    models::Rule,
};

#[derive(Deserialize)]
struct TransactionRow {
    id: i64,
    code: String,
    description: String,
    ref1: String,
    ref2: String,
    ref3: String,
}

impl TransactionRow {
    fn matches_rule(&self, rule: &Rule) -> bool {
        use crate::models::Field::*;

        match rule.field {
            Code => rule.pattern.is_match(&self.code),
            Description => rule.pattern.is_match(&self.description),
            Ref1 => rule.pattern.is_match(&self.ref1),
            Ref2 => rule.pattern.is_match(&self.ref2),
            Ref3 => rule.pattern.is_match(&self.ref3),
            Any => {
                // "any" — search all text fields
                rule.pattern.is_match(&self.code)
                    || rule.pattern.is_match(&self.description)
                    || rule.pattern.is_match(&self.ref1)
                    || rule.pattern.is_match(&self.ref2)
                    || rule.pattern.is_match(&self.ref3)
            }
        }
    }
}
/// Re-apply all categorisation rules to every transaction.
/// The highest-priority matching rule wins. Transactions with no match are left
/// as-is (existing category_id is preserved unless a rule now matches).
pub fn apply_rules(conn: &Connection) -> Result<usize> {
    let rules = db::all_rules_with_depth(conn)?;

    if rules.is_empty() {
        return Ok(0);
    }

    let mut stmt =
        conn.prepare("SELECT id, code, description, ref1, ref2, ref3 FROM transactions")?;

    let transactions: Vec<TransactionRow> = from_rows(stmt.query([])?)
        .collect::<serde_rusqlite::Result<Vec<_>>>()
        .map_err(|e| anyhow::anyhow!("failed to query transactions: {e}"))?;

    let mut categorized = 0usize;

    for transaction in &transactions {
        // Pick the best matching rule: highest priority wins; sub-category rules
        // beat parent catch-all rules of equal priority so the more specific
        // assignment always takes precedence.
        let best = rules
            .iter()
            .filter(|r| transaction.matches_rule(r))
            .max_by_key(|r| (r.priority, r.category_is_sub));

        if let Some(rule) = best {
            conn.execute(
                "UPDATE transactions SET category_id = ?1 WHERE id = ?2",
                params![rule.category_id, transaction.id],
            )?;
            categorized += 1;
        }
    }

    Ok(categorized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn test_conn() -> Connection {
        crate::db::open(":memory:").expect("in-memory db")
    }

    #[test]
    fn apply_rules_categorizes_matching_transaction() {
        let conn = test_conn();

        let account_id =
            crate::db::add_account(&conn, "Test Bank", "ACC001", "TEST", "SGD").unwrap();
        let cat_id = crate::db::add_category(&conn, "Food", None).unwrap();
        crate::db::add_rule(&conn, cat_id, "description", "McDonald", 0).unwrap();

        conn.execute(
            "INSERT INTO transactions \
             (account_id, date, code, description, ref1, ref2, ref3, status, debit, credit, hash) \
             VALUES (?1, '2024-01-15', '', 'McDonald''s', '', '', '', '', 12.50, NULL, 'hash001')",
            params![account_id],
        )
        .unwrap();

        let categorized = apply_rules(&conn).unwrap();
        assert_eq!(categorized, 1);

        let assigned: i64 = conn
            .query_row(
                "SELECT category_id FROM transactions WHERE hash = 'hash001'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(assigned, cat_id);
    }

    #[test]
    fn apply_rules_skips_non_matching_transaction() {
        let conn = test_conn();

        let account_id =
            crate::db::add_account(&conn, "Test Bank", "ACC002", "TEST", "SGD").unwrap();
        let cat_id = crate::db::add_category(&conn, "Transport", None).unwrap();
        crate::db::add_rule(&conn, cat_id, "description", "Grab", 0).unwrap();

        conn.execute(
            "INSERT INTO transactions \
             (account_id, date, code, description, ref1, ref2, ref3, status, debit, credit, hash) \
             VALUES (?1, '2024-01-15', '', 'NTUC Fairprice', '', '', '', '', 30.00, NULL, 'hash002')",
            params![account_id],
        )
        .unwrap();

        let categorized = apply_rules(&conn).unwrap();
        assert_eq!(categorized, 0);

        let assigned: Option<i64> = conn
            .query_row(
                "SELECT category_id FROM transactions WHERE hash = 'hash002'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(assigned.is_none());
    }
}
