use anyhow::Result;
use regex::Regex;
use rusqlite::{params, Connection};

use crate::db;

struct CompiledRule {
    category_id: i64,
    field: String,
    pattern: Regex,
    priority: i64,
    /// True when this rule's category is a sub-category (has a parent).
    /// Used as a secondary sort key so sub-category rules win ties against
    /// parent catch-all rules at the same explicit priority.
    is_sub: bool,
}

fn matches(
    rule: &CompiledRule,
    code: &str,
    desc: &str,
    ref1: &str,
    ref2: &str,
    ref3: &str,
) -> bool {
    match rule.field.as_str() {
        "code" => rule.pattern.is_match(code),
        "description" => rule.pattern.is_match(desc),
        "ref1" => rule.pattern.is_match(ref1),
        "ref2" => rule.pattern.is_match(ref2),
        "ref3" => rule.pattern.is_match(ref3),
        _ => {
            // "any" — search all text fields
            rule.pattern.is_match(code)
                || rule.pattern.is_match(desc)
                || rule.pattern.is_match(ref1)
                || rule.pattern.is_match(ref2)
                || rule.pattern.is_match(ref3)
        }
    }
}

/// Re-apply all categorisation rules to every transaction.
/// The highest-priority matching rule wins. Transactions with no match are left
/// as-is (existing category_id is preserved unless a rule now matches).
pub fn apply_rules(conn: &Connection) -> Result<usize> {
    let raw = db::all_rules_with_depth(conn)?;

    let rules: Vec<CompiledRule> = raw
        .into_iter()
        .filter_map(|(r, is_sub)| match Regex::new(&r.pattern) {
            Ok(re) => Some(CompiledRule {
                category_id: r.category_id,
                field: r.field,
                pattern: re,
                priority: r.priority,
                is_sub,
            }),
            Err(e) => {
                eprintln!(
                    "Warning: skipping rule #{} — invalid regex '{}': {e}",
                    r.id, r.pattern
                );
                None
            }
        })
        .collect();

    if rules.is_empty() {
        return Ok(0);
    }

    struct TxRow {
        id: i64,
        code: String,
        desc: String,
        ref1: String,
        ref2: String,
        ref3: String,
    }

    let mut stmt =
        conn.prepare("SELECT id, code, description, ref1, ref2, ref3 FROM transactions")?;
    let txs: Vec<TxRow> = stmt
        .query_map([], |row| {
            Ok(TxRow {
                id: row.get(0)?,
                code: row.get(1)?,
                desc: row.get(2)?,
                ref1: row.get(3)?,
                ref2: row.get(4)?,
                ref3: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut categorized = 0usize;

    for tx in &txs {
        // Pick the best matching rule: highest priority wins; sub-category rules
        // beat parent catch-all rules of equal priority so the more specific
        // assignment always takes precedence.
        let best = rules
            .iter()
            .filter(|r| matches(r, &tx.code, &tx.desc, &tx.ref1, &tx.ref2, &tx.ref3))
            .max_by_key(|r| (r.priority, r.is_sub));

        if let Some(rule) = best {
            conn.execute(
                "UPDATE transactions SET category_id = ?1 WHERE id = ?2",
                params![rule.category_id, tx.id],
            )?;
            categorized += 1;
        }
    }

    Ok(categorized)
}
