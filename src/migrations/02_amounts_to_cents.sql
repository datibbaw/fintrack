-- Migrate debit/credit from REAL to INTEGER (cents).
-- SQLite does not support ALTER COLUMN, so recreate the table.
CREATE TABLE transactions_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id  INTEGER NOT NULL REFERENCES accounts(id),
    date        TEXT    NOT NULL,
    code        TEXT    NOT NULL DEFAULT '',
    description TEXT    NOT NULL DEFAULT '',
    ref1        TEXT    NOT NULL DEFAULT '',
    ref2        TEXT    NOT NULL DEFAULT '',
    ref3        TEXT    NOT NULL DEFAULT '',
    status      TEXT    NOT NULL DEFAULT '',
    debit       INTEGER,
    credit      INTEGER,
    hash        TEXT    NOT NULL UNIQUE,
    category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL
);

INSERT INTO transactions_new
    SELECT id, account_id, date, code, description, ref1, ref2, ref3, status,
           CASE WHEN debit  IS NOT NULL THEN CAST(ROUND(debit  * 100) AS INTEGER) END,
           CASE WHEN credit IS NOT NULL THEN CAST(ROUND(credit * 100) AS INTEGER) END,
           hash, category_id
    FROM transactions;

DROP TABLE transactions;
ALTER TABLE transactions_new RENAME TO transactions;

CREATE INDEX IF NOT EXISTS idx_tx_date     ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_tx_account  ON transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_tx_category ON transactions(category_id);
CREATE INDEX IF NOT EXISTS idx_tx_hash     ON transactions(hash);
