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
