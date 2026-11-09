-- Replace the single (field, pattern) pair on rules with a JSON `criteria`
-- column holding `and` / `or` condition groups (see models::rule::Criteria),
-- and add an optional `account_id` to scope a rule to a single account.
--
-- Existing rules are rewritten with the same (regex) semantics:
--   * `any` rules → an `or` group with a `matches` condition on each text
--     field (code, description, ref1, ref2, ref3).
--   * everything else → a single `matches` condition in the `and` group.
--
-- `amount` rules are not converted: `amount` only supports `equals` with an
-- integer, and no such rules existed before this migration.
CREATE TABLE rules_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    account_id  INTEGER          REFERENCES accounts(id)   ON DELETE CASCADE,
    criteria    TEXT    NOT NULL CHECK (json_valid(criteria)),
    priority    INTEGER NOT NULL DEFAULT 0
);

INSERT INTO rules_new (id, category_id, account_id, criteria, priority)
SELECT
    id,
    category_id,
    NULL,
    CASE field
        -- `any` searched every text field; express it as an `or` group.
        WHEN 'any' THEN json_object('or', json_array(
            json_object('field', 'code',        'matches', pattern),
            json_object('field', 'description', 'matches', pattern),
            json_object('field', 'ref1',        'matches', pattern),
            json_object('field', 'ref2',        'matches', pattern),
            json_object('field', 'ref3',        'matches', pattern)
        ))
        ELSE json_object('and', json_array(
            json_object('field', field, 'matches', pattern)
        ))
    END,
    priority
FROM rules;

DROP TABLE rules;
ALTER TABLE rules_new RENAME TO rules;

CREATE INDEX IF NOT EXISTS idx_rules_account ON rules(account_id);
