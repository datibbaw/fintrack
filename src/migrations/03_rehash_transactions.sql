-- Recompute hashes using integer cent values instead of the old REAL representation.
-- Payload mirrors compute_hash: account_id|date|code|ref1|ref2|ref3|debit|credit
-- debit/credit are the raw INTEGER cent values (or empty string when NULL).
UPDATE transactions
SET hash = fintrack_sha256(
    CAST(account_id AS TEXT) || '|' ||
    date                     || '|' ||
    code                     || '|' ||
    ref1                     || '|' ||
    ref2                     || '|' ||
    ref3                     || '|' ||
    COALESCE(CAST(debit  AS TEXT), '') || '|' ||
    COALESCE(CAST(credit AS TEXT), '')
);
