use anyhow::Result;
use chrono::NaiveDate;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1, take_while_m_n},
    character::complete::{one_of, space1},
    combinator::{map_res, recognize},
    IResult, Parser,
};
use rusty_money::{iso, Money};
use std::path::Path;

use crate::models::TransactionBuilder;

// ── Tokens ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum Token {
    Date(NaiveDate),
    Time,
    Amount(i64),
    Text(String),
}

// ── Line tokenizer ────────────────────────────────────────────────────────────

fn date_from_str(input: &str) -> IResult<&str, NaiveDate> {
    let day = take_while_m_n(1, 2, |c: char| c.is_ascii_digit());
    let month = alt((
        tag("Jan"),
        tag("Feb"),
        tag("Mar"),
        tag("Apr"),
        tag("May"),
        tag("Jun"),
        tag("Jul"),
        tag("Aug"),
        tag("Sep"),
        tag("Oct"),
        tag("Nov"),
        tag("Dec"),
    ));
    let year = take_while_m_n(4, 4, |c: char| c.is_ascii_digit());

    map_res(recognize((day, space1, month, space1, year)), |s: &str| {
        NaiveDate::parse_from_str(s, "%d %b %Y")
    })
    .parse(input)
}

fn time_from_str(input: &str) -> IResult<&str, &str> {
    let hour = take_while_m_n(1, 2, |c: char| c.is_ascii_digit());
    let minute = take_while_m_n(2, 2, |c: char| c.is_ascii_digit());
    let am_pm = alt((tag("AM"), tag("PM")));

    recognize((hour, tag(":"), minute, space1, am_pm)).parse(input)
}

fn parse_amount_str<'a>(input: &'a str, currency: &'static iso::Currency) -> IResult<&'a str, i64> {
    let parser = (
        one_of("¥$€"),
        take_while1(|c: char| c.is_ascii_digit() || c == ',' || c == '.'),
    );

    map_res(parser, |(_, digits)| {
        Money::from_str(digits, currency).map(|m| m.to_minor_units())
    })
    .parse(input)
}

fn tokenize_line(s: &str, currency: &'static iso::Currency) -> Vec<Token> {
    let s = s.trim();
    if s.is_empty() {
        return vec![];
    }
    if let Ok((rest, date)) = date_from_str(s) {
        let mut tokens = vec![Token::Date(date)];
        tokens.extend(tokenize_line(rest, currency));
        return tokens;
    }
    if let Ok((rest, _)) = time_from_str(s) {
        let mut tokens = vec![Token::Time];
        tokens.extend(tokenize_line(rest, currency));
        return tokens;
    }

    let mut result = Vec::new();

    let mut iter = s.split_whitespace().rev();
    // Find longest suffix of amounts
    while let Some(part) = iter.next() {
        if let Ok((_, amount)) = parse_amount_str(part, currency) {
            result.push(Token::Amount(amount));
        } else {
            let mut prefix: Vec<&str> = vec![part];
            prefix.extend(iter);
            prefix.reverse();
            result.push(Token::Text(prefix.join(" ")));
            break;
        }
    }

    result.reverse();

    result
}

// ── Transaction parser ────────────────────────────────────────────────────────

struct TransactionParser<I: Iterator<Item = Token>> {
    tokens: I,
    prev_balance: Option<i64>,
}

impl<I: Iterator<Item = Token>> TransactionParser<I> {
    fn new(tokens: I) -> Self {
        Self {
            tokens,
            prev_balance: None,
        }
    }

    fn skip_until_date(&mut self) -> Option<NaiveDate> {
        self.tokens.by_ref().find_map(|t| {
            if let Token::Date(d) = t {
                Some(d)
            } else {
                None
            }
        })
    }

    fn take_until_amount(&mut self) -> Option<(Vec<String>, i64)> {
        let mut descs = Vec::new();
        for t in self.tokens.by_ref() {
            match t {
                Token::Text(s) => descs.push(s),
                Token::Amount(a) => {
                    return Some((descs, a));
                }
                _ => break,
            }
        }
        None
    }

    fn next_amount(&mut self) -> Option<i64> {
        self.tokens.by_ref().find_map(|t| {
            if let Token::Amount(a) = t {
                Some(a)
            } else {
                None
            }
        })
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
        if let Some(r) = it.next() {
            builder.ref1(r);
        }
        if let Some(r) = it.next() {
            builder.ref2(r);
        }
        if let Some(r) = it.next() {
            builder.ref3(r);
        }
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

impl<I: Iterator<Item = Token>> Iterator for TransactionParser<I> {
    type Item = TransactionBuilder;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Find next date
            let date = self.skip_until_date()?;

            match self.tokens.next()? {
                Token::Text(s) if s == "Opening Balance" || s == "Closing Balance" => {
                    if let Some(bal) = self.next_amount() {
                        self.prev_balance.get_or_insert(bal);
                    }
                }
                Token::Time => {
                    let Some((descs, amount)) = self.take_until_amount() else {
                        continue;
                    };
                    let Some(balance) = self.next_amount() else {
                        continue;
                    };
                    let builder = self.build(date, descs, amount, balance);
                    self.prev_balance = Some(balance);
                    return Some(builder);
                }
                _ => (),
            }
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub(super) fn parse<P: AsRef<Path>>(
    path: P,
    currency: &'static iso::Currency,
) -> Result<Vec<TransactionBuilder>> {
    let text = pdf_extract::extract_text(path.as_ref())?;
    let tokens: Vec<Token> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .flat_map(|l| tokenize_line(l, currency))
        .collect();
    Ok(TransactionParser::new(tokens.into_iter()).collect())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%d %b %Y").unwrap()
    }

    fn collect_tokens(tokens: Vec<Token>) -> Vec<TransactionBuilder> {
        TransactionParser::new(tokens.into_iter()).collect()
    }

    #[test]
    fn debit_reduces_balance() {
        let rows = collect_tokens(vec![
            Token::Date(d("01 Jan 2024")),
            Token::Text("Opening Balance".into()),
            Token::Amount(1500),
            Token::Date(d("01 Jan 2024")),
            Token::Time,
            Token::Text("FamilyMart".into()),
            Token::Amount(500),
            Token::Amount(1000),
        ]);
        assert_eq!(rows.len(), 1);
        let mut b = rows.into_iter().next().unwrap();
        let tx = b.account_id(1).build().unwrap();
        assert_eq!(tx.debit, Some(500));
        assert_eq!(tx.credit, None);
        assert_eq!(tx.description, "FamilyMart");
    }

    #[test]
    fn credit_increases_balance() {
        let rows = collect_tokens(vec![
            Token::Date(d("01 Jan 2024")),
            Token::Text("Opening Balance".into()),
            Token::Amount(1000),
            Token::Date(d("01 Jan 2024")),
            Token::Time,
            Token::Text("Refund".into()),
            Token::Amount(500),
            Token::Amount(1500),
        ]);
        assert_eq!(rows.len(), 1);
        let mut b = rows.into_iter().next().unwrap();
        let tx = b.account_id(1).build().unwrap();
        assert_eq!(tx.credit, Some(500));
        assert_eq!(tx.debit, None);
    }

    #[test]
    fn multi_line_description() {
        let rows = collect_tokens(vec![
            Token::Date(d("01 Jan 2024")),
            Token::Text("Opening Balance".into()),
            Token::Amount(2000),
            Token::Date(d("01 Jan 2024")),
            Token::Time,
            Token::Text("Sushi Restaurant".into()),
            Token::Text("Shibuya Tokyo".into()),
            Token::Text("Floor 3".into()),
            Token::Amount(800),
            Token::Amount(1200),
        ]);
        assert_eq!(rows.len(), 1);
        let mut b = rows.into_iter().next().unwrap();
        let tx = b.account_id(1).build().unwrap();
        assert_eq!(tx.description, "Sushi Restaurant");
        assert_eq!(tx.ref1, "Shibuya Tokyo");
        assert_eq!(tx.ref2, "Floor 3");
    }

    #[test]
    fn multiple_transactions_sequential() {
        let rows = collect_tokens(vec![
            Token::Date(d("01 Jan 2024")),
            Token::Text("Opening Balance".into()),
            Token::Amount(5000),
            Token::Date(d("01 Jan 2024")),
            Token::Time,
            Token::Text("Convenience".into()),
            Token::Amount(200),
            Token::Amount(4800),
            Token::Date(d("01 Jan 2024")),
            Token::Time,
            Token::Text("Ramen".into()),
            Token::Amount(900),
            Token::Amount(3900),
        ]);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn skips_non_transaction_tokens() {
        let rows = collect_tokens(vec![
            Token::Text("YouTrip Statement".into()),
            Token::Text("Page 1".into()),
            Token::Text("Date".into()),
            Token::Text("Description".into()),
            Token::Date(d("01 Jan 2024")),
            Token::Text("Opening Balance".into()),
            Token::Amount(3000),
            Token::Date(d("01 Jan 2024")),
            Token::Time,
            Token::Text("7-Eleven".into()),
            Token::Amount(150),
            Token::Amount(2850),
        ]);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn tokenize_date_only_line() {
        let tokens = tokenize_line("8 Mar 2026", iso::JPY);
        assert!(matches!(tokens[..], [Token::Date(_)]));
    }

    #[test]
    fn tokenize_time_only_line() {
        let tokens = tokenize_line("11:19 AM", iso::JPY);
        assert!(matches!(tokens[..], [Token::Time]));
    }

    #[test]
    fn tokenize_two_amounts_line() {
        let tokens = tokenize_line(" ¥123,148 ¥124,284", iso::JPY);
        assert_eq!(tokens.len(), 2);
        assert!(matches!(tokens[0], Token::Amount(123148)));
        assert!(matches!(tokens[1], Token::Amount(124284)));
    }

    #[test]
    fn tokenize_time_with_description_and_amounts() {
        // "1:15 AM MANDAI TADA ,HYOGO ¥13,629 ¥222,570"
        let tokens = tokenize_line("1:15 AM MANDAI TADA ,HYOGO ¥13,629 ¥222,570", iso::JPY);
        assert!(matches!(tokens[0], Token::Time));
        assert!(matches!(&tokens[1], Token::Text(s) if s == "MANDAI TADA ,HYOGO"));
        assert!(matches!(tokens[2], Token::Amount(13629)));
        assert!(matches!(tokens[3], Token::Amount(222570)));
    }

    #[test]
    fn tokenize_description_with_embedded_currency_not_split() {
        // Amounts inside descriptions have trailing currency codes — kept as Text
        let tokens = tokenize_line("$1,000.00 SGD to ¥123,148 JPY", iso::JPY);
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0], Token::Text(s) if s.contains("SGD")));
    }

    #[test]
    fn tokenize_date_with_balance() {
        let tokens = tokenize_line("1 Mar 2026 Opening Balance ¥1,136", iso::JPY);
        assert!(matches!(tokens[0], Token::Date(_)));
        assert!(matches!(&tokens[1], Token::Text(s) if s == "Opening Balance"));
        assert!(matches!(tokens[2], Token::Amount(1136)));
    }

    #[test]
    fn test_parse_date_from_str() {
        let date = date_from_str("8 Mar 2026")
            .unwrap_or_else(|e| {
                println!("Error: {e:?}");
                panic!("Failed to parse date");
            })
            .1;
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 3, 8).unwrap());
    }

    #[test]
    #[ignore]
    fn parse_actual_pdf() {
        // cargo test parse_actual_pdf -- --ignored --nocapture
        let path = env::var("PDF_FILE").expect("set PDF_FILE=/path/to/youtrip-statement.pdf");
        for (i, mut builder) in parse(path, iso::JPY).unwrap().into_iter().enumerate() {
            let tx = builder.account_id(1).build().unwrap();
            println!(
                "{:2}. {} {:50} debit={:?} credit={:?}  ref1={}",
                i + 1,
                tx.date,
                tx.description,
                tx.debit,
                tx.credit,
                tx.ref1,
            );
        }
    }
}
