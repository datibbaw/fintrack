use anyhow::Result;
use chrono::NaiveDate;
use rusty_money::{iso, Money};
use std::path::Path;

use super::pdf_text::{PdfTextDocument, TextObject};
use crate::models::TransactionBuilder;

// ── Text predicates and parsers ───────────────────────────────────────────────

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%d %b %Y").ok()
}

fn is_date_str(o: &TextObject) -> bool {
    parse_date(&o.text).is_some()
}

fn is_balance_line(o: &TextObject) -> bool {
    o.text == "Opening Balance" || o.text == "Closing Balance"
}

fn is_money_str(o: &TextObject) -> bool {
    let s = o.text.trim();
    let Some(rest) = s.strip_prefix(['¥', '$', '€']) else { return false };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == ',' || c == '.')
}

fn is_time_str(o: &TextObject) -> bool {
    // "H:MM AM" / "HH:MM PM"
    let s = o.text.trim();
    let Some(colon) = s.find(':') else { return false };
    let rest = &s[colon + 1..];
    s[..colon].parse::<u8>().is_ok()
        && rest.get(..2).and_then(|m| m.parse::<u8>().ok()).is_some()
        && matches!(rest.get(2..), Some(" AM") | Some(" PM"))
}

// ── TransactionRowIterator ────────────────────────────────────────────────────

/// Wraps any `TextObject` iterator and yields one `TransactionBuilder` per
/// transaction row, skipping headers, footers, and balance lines.
///
/// Expects each transaction to arrive in PDF stream order:
/// `date → time → desc+ → amount → balance`
struct TransactionRowIterator<T: Iterator<Item = TextObject>> {
    iter: T,
    currency: &'static iso::Currency,
    prev_balance: Option<i64>,
}

impl<T: Iterator<Item = TextObject>> TransactionRowIterator<T> {
    fn new(iter: T, currency: &'static iso::Currency) -> Self {
        Self { iter, currency, prev_balance: None }
    }

    /// Advances past tokens until `pred` matches; returns the matching token,
    /// or `None` on EOF.
    fn skip_until(&mut self, pred: impl Fn(&TextObject) -> bool) -> Option<TextObject> {
        self.iter.by_ref().find(|o| pred(o))
    }

    /// Collects tokens into a `Vec` until `pred` matches.
    /// Returns `(collected, terminator)` where `terminator` is the matching
    /// token, or `None` if EOF was reached before any match.
    fn take_until(
        &mut self,
        pred: impl Fn(&TextObject) -> bool,
    ) -> Option<(Vec<TextObject>, TextObject)> {
        let mut collected = Vec::new();
        for obj in self.iter.by_ref() {
            if pred(&obj) {
                return Some((collected, obj));
            }
            collected.push(obj);
        }
        None
    }

    fn parse_money(&self, s: &str) -> Option<i64> {
        let start = s.find(|c: char| c.is_ascii_digit())?;
        Money::from_str(&s[start..], self.currency).ok().map(|m| m.to_minor_units())
    }

    fn build(
        &mut self,
        date: NaiveDate,
        descs: Vec<String>,
        amount: i64,
        balance: i64,
    ) -> TransactionBuilder {
        let mut builder = TransactionBuilder::default();
        builder.date(date);
        let mut it = descs.into_iter();
        builder.description(it.next().unwrap_or_default());
        if let Some(r) = it.next() { builder.ref1(r); }
        if let Some(r) = it.next() { builder.ref2(r); }
        if let Some(r) = it.next() { builder.ref3(r); }
        let prev = self.prev_balance.unwrap_or(0);
        if balance > prev {
            builder.credit(Some(amount));
            builder.debit(None);
        } else {
            builder.debit(Some(amount));
            builder.credit(None);
        }
        builder
    }
}

impl<T: Iterator<Item = TextObject>> Iterator for TransactionRowIterator<T> {
    type Item = TransactionBuilder;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let date = self.skip_until(is_date_str).and_then(|o| parse_date(&o.text))?;

            match self.iter.next()? {
                tok if is_time_str(&tok) => {
                    // transaction row: time consumed, then descs → amount → balance
                    let (descs, amount_obj) = self.take_until(is_money_str)?;
                    let amount = self.parse_money(&amount_obj.text)?;
                    let balance = self.skip_until(is_money_str).and_then(|o| self.parse_money(&o.text))?;
                    let descs = descs.into_iter().map(|o| o.text).collect();
                    let builder = self.build(date, descs, amount, balance);
                    self.prev_balance = Some(balance);
                    return Some(builder);
                }
                tok if is_balance_line(&tok) => {
                    // opening/closing balance: capture for debit/credit direction, no emit
                    let balance = self.skip_until(is_money_str).and_then(|o| self.parse_money(&o.text))?;
                    self.prev_balance.get_or_insert(balance);
                }
                _ => {} // unexpected token after date — skip to next date
            }
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub(super) fn parse<P: AsRef<Path>>(path: P, currency: &'static iso::Currency) -> Result<Vec<TransactionBuilder>> {
    let doc = PdfTextDocument::load(path)?;
    Ok(TransactionRowIterator::new(doc.text_object_iter(), currency).collect())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    fn obj(text: &str) -> TextObject {
        TextObject { text: text.to_string(), x: 0.0, y: 0.0 }
    }

    fn collect(objects: Vec<TextObject>, currency: &'static iso::Currency) -> Vec<TransactionBuilder> {
        TransactionRowIterator::new(objects.into_iter(), currency).collect()
    }

    #[test]
    fn debit_reduces_balance() {
        // balance drops 1500 → 1000 → transaction is debit of 500
        let rows = collect(vec![
            obj("01 Jan 2024"), obj("Opening Balance"), obj("¥1500"),
            obj("01 Jan 2024"), obj("3:00 PM"), obj("FamilyMart"), obj("¥500"), obj("¥1000"),
        ], iso::JPY);
        assert_eq!(rows.len(), 1);
        let mut b = rows.into_iter().next().unwrap();
        let tx = b.account_id(1).build().unwrap();
        assert_eq!(tx.debit, Some(500));
        assert_eq!(tx.credit, None);
        assert_eq!(tx.description, "FamilyMart");
    }

    #[test]
    fn credit_increases_balance() {
        // balance rises 1000 → 1500 → transaction is credit of 500
        let rows = collect(vec![
            obj("01 Jan 2024"), obj("Opening Balance"), obj("¥1000"),
            obj("01 Jan 2024"), obj("10:30 AM"), obj("Refund"), obj("¥500"), obj("¥1500"),
        ], iso::JPY);
        assert_eq!(rows.len(), 1);
        let mut b = rows.into_iter().next().unwrap();
        let tx = b.account_id(1).build().unwrap();
        assert_eq!(tx.credit, Some(500));
        assert_eq!(tx.debit, None);
    }

    #[test]
    fn multi_line_description() {
        let rows = collect(vec![
            obj("01 Jan 2024"), obj("Opening Balance"), obj("¥2000"),
            obj("01 Jan 2024"), obj("2:00 PM"),
            obj("Sushi Restaurant"), obj("Shibuya Tokyo"), obj("Floor 3"),
            obj("¥800"), obj("¥1200"),
        ], iso::JPY);
        assert_eq!(rows.len(), 1);
        let mut b = rows.into_iter().next().unwrap();
        let tx = b.account_id(1).build().unwrap();
        assert_eq!(tx.description, "Sushi Restaurant");
        assert_eq!(tx.ref1, "Shibuya Tokyo");
        assert_eq!(tx.ref2, "Floor 3");
    }

    #[test]
    fn multiple_transactions_sequential() {
        let rows = collect(vec![
            obj("01 Jan 2024"), obj("Opening Balance"), obj("¥5000"),
            obj("01 Jan 2024"), obj("9:00 AM"), obj("Convenience"), obj("¥200"), obj("¥4800"),
            obj("01 Jan 2024"), obj("12:00 PM"), obj("Ramen"), obj("¥900"), obj("¥3900"),
        ], iso::JPY);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn skips_non_transaction_tokens() {
        // junk tokens before the first date should be skipped
        let rows = collect(vec![
            obj("YouTrip Statement"), obj("Page 1"), obj("Date"), obj("Description"),
            obj("01 Jan 2024"), obj("Opening Balance"), obj("¥3000"),
            obj("01 Jan 2024"), obj("1:00 PM"), obj("7-Eleven"), obj("¥150"), obj("¥2850"),
        ], iso::JPY);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    #[ignore]
    fn parse_actual_pdf() {
        // cargo test parse_actual_pdf -- --ignored --nocapture
        let path = env::var("PDF_FILE").expect("set PDF_FILE=/path/to/youtrip-statement.pdf to run this test");
        for (i, mut builder) in parse(path, iso::JPY).unwrap().into_iter().enumerate() {
            let tx = builder.account_id(1).build().unwrap();
            println!(
                "{:2}. {} {:50} debit={:?} credit={:?}  ref1={}",
                i + 1, tx.date, tx.description, tx.debit, tx.credit, tx.ref1,
            );
        }
    }
}
