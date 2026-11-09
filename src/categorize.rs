use anyhow::Result;
use rusqlite::{Connection, params};
use serde_rusqlite::from_rows;

use crate::{db, models::Transaction};

/// Re-apply all categorisation rules to every transaction.
/// The highest-priority matching rule wins. Transactions with no match are left
/// as-is (existing category_id is preserved unless a rule now matches).
pub fn apply_rules(conn: &Connection) -> Result<usize> {
    let rules = db::all_rules_with_depth(conn)?;

    if rules.is_empty() {
        return Ok(0);
    }

    let mut stmt = conn.prepare(
        "SELECT id, account_id, date, code, description, ref1, ref2, ref3, \
                status, debit, credit, hash \
         FROM transactions",
    )?;

    let transactions: Vec<Transaction> = from_rows(stmt.query([])?)
        .collect::<serde_rusqlite::Result<Vec<_>>>()
        .map_err(|e| anyhow::anyhow!("failed to query transactions: {e}"))?;

    let mut categorized = 0usize;

    for transaction in &transactions {
        // Pick the best matching rule: highest priority wins; sub-category rules
        // beat parent catch-all rules of equal priority so the more specific
        // assignment always takes precedence.
        let best = rules
            .iter()
            .filter(|r| r.is_match(transaction))
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
    use rusty_money::iso;

    fn test_conn() -> Connection {
        crate::db::open(":memory:").expect("in-memory db")
    }

    #[test]
    fn apply_rules_categorizes_matching_transaction() {
        let conn = test_conn();

        let account_id = crate::db::add_account(
            &conn,
            "Test Bank",
            "ACC001",
            "TEST",
            iso::find("SGD").unwrap(),
        )
        .unwrap();
        let cat_id = crate::db::add_category(&conn, "Food", None).unwrap();
        let criteria =
            serde_json::from_str(r#"{"and":[{"field":"description","matches":"McDonald"}]}"#)
                .unwrap();
        crate::db::add_rule(&conn, cat_id, None, &criteria, 0).unwrap();

        conn.execute(
            "INSERT INTO transactions \
             (account_id, date, code, description, ref1, ref2, ref3, status, debit, credit, hash) \
             VALUES (?1, '2024-01-15', '', 'McDonald''s', '', '', '', '', 1250, NULL, 'hash001')",
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

        let account_id = crate::db::add_account(
            &conn,
            "Test Bank",
            "ACC002",
            "TEST",
            iso::find("SGD").unwrap(),
        )
        .unwrap();
        let cat_id = crate::db::add_category(&conn, "Transport", None).unwrap();
        let criteria =
            serde_json::from_str(r#"{"and":[{"field":"description","matches":"Grab"}]}"#).unwrap();
        crate::db::add_rule(&conn, cat_id, None, &criteria, 0).unwrap();

        conn.execute(
            "INSERT INTO transactions \
             (account_id, date, code, description, ref1, ref2, ref3, status, debit, credit, hash) \
             VALUES (?1, '2024-01-15', '', 'NTUC Fairprice', '', '', '', '', 3000, NULL, 'hash002')",
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

    #[test]
    fn apply_rules_respects_account_scope() {
        let conn = test_conn();
        let sgd = iso::find("SGD").unwrap();
        let acc_a = crate::db::add_account(&conn, "A", "ACC-A", "TEST", sgd).unwrap();
        let acc_b = crate::db::add_account(&conn, "B", "ACC-B", "TEST", sgd).unwrap();
        let cat_id = crate::db::add_category(&conn, "Rent", None).unwrap();
        let criteria =
            serde_json::from_str(r#"{"and":[{"field":"description","includes":"GIRO"}]}"#).unwrap();
        crate::db::add_rule(&conn, cat_id, Some(acc_a), &criteria, 0).unwrap();

        for (acc, hash) in [(acc_a, "hA"), (acc_b, "hB")] {
            conn.execute(
                "INSERT INTO transactions \
                 (account_id, date, code, description, ref1, ref2, ref3, status, debit, credit, hash) \
                 VALUES (?1, '2024-01-15', '', 'GIRO PAYMENT', '', '', '', '', 100, NULL, ?2)",
                params![acc, hash],
            )
            .unwrap();
        }

        assert_eq!(apply_rules(&conn).unwrap(), 1);
        let cat_of = |hash: &str| -> Option<i64> {
            conn.query_row(
                "SELECT category_id FROM transactions WHERE hash = ?1",
                params![hash],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(cat_of("hA"), Some(cat_id));
        assert_eq!(cat_of("hB"), None);
    }
}
