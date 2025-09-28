// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::{get_base_currency, http_client, pretty_table, set_base_currency};
use anyhow::{Context, Result, ensure};
use chrono::Utc;
use rusqlite::{Connection, params};
use serde::Deserialize;
use std::collections::HashSet;
use std::convert::TryFrom;

use rust_decimal::Decimal;

pub fn handle(conn: &mut Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("set-base", sub)) => {
            let ccy = sub
                .get_one::<String>("currency")
                .unwrap()
                .trim()
                .to_uppercase();
            set_base_currency(conn, &ccy)?;
            println!("Base currency set to {}", ccy);
        }
        Some(("fetch", sub)) => {
            let days: usize = *sub.get_one::<usize>("days").unwrap_or(&120);
            fetch_rates(conn, days)?;
        }
        Some(("list", _)) => list_rates(conn)?,
        Some(("convert", sub)) => convert_amount(conn, sub)?,
        _ => {}
    }
    Ok(())
}

fn distinct_currencies(conn: &Connection) -> Result<Vec<String>> {
    let mut out = Vec::<String>::new();
    let mut seen = HashSet::new();
    for sql in [
        "SELECT DISTINCT currency FROM accounts",
        "SELECT DISTINCT currency FROM assets",
        "SELECT DISTINCT currency FROM transactions",
    ] {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for row in rows {
            let c = row?;
            let trimmed = c.trim();
            if !trimmed.is_empty() {
                let normalized = trimmed.to_uppercase();
                if seen.insert(normalized.clone()) {
                    out.push(normalized);
                }
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
struct Series {
    rates: std::collections::HashMap<String, std::collections::HashMap<String, f64>>,
    #[serde(rename = "base")]
    _base: String,
}
fn fetch_rates(conn: &mut Connection, days: usize) -> Result<()> {
    let base = get_base_currency(conn)?.trim().to_uppercase();
    let today = Utc::now().date_naive();
    let start = today - chrono::Duration::days(days as i64);
    let ccy_list = distinct_currencies(conn)?;
    let targets: Vec<String> = ccy_list.into_iter().filter(|c| c != &base).collect();
    if targets.is_empty() {
        println!("No non-base currencies found; nothing to fetch.");
        return Ok(());
    }
    let to_param = targets.join(",");
    let url = format!("https://api.frankfurter.dev/{start}..{today}?from={base}&to={to_param}");
    let client = http_client()?;
    let resp = client.get(url).send()?.error_for_status()?;
    let s: Series = resp.json()?;
    let mut upserted = 0usize;
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT OR REPLACE INTO fx_rates(date, base, quote, rate) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (date, mp) in s.rates {
            for (quote, rate) in mp {
                let normalized_quote = quote.trim().to_uppercase();
                let rate_str = decimal_string(rate).with_context(|| {
                    format!("Invalid FX rate {} for {}/{}", rate, base, normalized_quote)
                })?;
                upserted += stmt.execute(params![&date, &base, &normalized_quote, &rate_str])?;
            }
        }
    }
    tx.commit()?;
    println!(
        "FX rates fetched via Frankfurter (ECB); {} rows upserted.",
        upserted
    );
    Ok(())
}

fn decimal_string(rate: f64) -> Result<String> {
    ensure!(rate.is_finite(), "Fetched FX rate must be finite");
    let decimal = Decimal::try_from(rate).context("Failed to convert FX rate to Decimal")?;
    ensure!(decimal > Decimal::ZERO, "Fetched FX rate must be positive");
    Ok(decimal.to_string())
}

fn list_rates(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT date, base, quote, rate FROM fx_rates ORDER BY date DESC, base, quote LIMIT 50",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut data = Vec::new();
    for row in rows {
        let (d, b, q, r) = row?;
        data.push(vec![d, b, q, r]);
    }
    println!("{}", pretty_table(&["Date", "Base", "Quote", "Rate"], data));
    Ok(())
}

fn convert_amount(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let date = crate::utils::parse_date(sub.get_one::<String>("date").unwrap().trim())?;
    let amount = crate::utils::parse_decimal(sub.get_one::<String>("amount").unwrap().trim())?;
    let from = sub.get_one::<String>("from").unwrap().trim().to_uppercase();
    let to = sub.get_one::<String>("to").unwrap().trim().to_uppercase();
    let res = crate::utils::fx_convert(conn, date, amount, &from, &to)?;
    println!("{} {} -> {:.4} {}", amount, from, res, to);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decimal_string, distinct_currencies};
    use rusqlite::Connection;

    #[test]
    fn distinct_currencies_dedupes_and_normalizes() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE accounts(currency TEXT);
            CREATE TABLE assets(currency TEXT);
            CREATE TABLE transactions(currency TEXT);
            "#,
        )
        .unwrap();

        conn.execute("INSERT INTO accounts(currency) VALUES (' usd ')", [])
            .unwrap();
        conn.execute("INSERT INTO accounts(currency) VALUES ('USD')", [])
            .unwrap();
        conn.execute("INSERT INTO assets(currency) VALUES ('eur')", [])
            .unwrap();
        conn.execute("INSERT INTO assets(currency) VALUES (' EUR ')", [])
            .unwrap();
        conn.execute("INSERT INTO transactions(currency) VALUES ('jpy ')", [])
            .unwrap();
        conn.execute("INSERT INTO transactions(currency) VALUES ('JPY')", [])
            .unwrap();

        let values = distinct_currencies(&conn).unwrap();
        assert_eq!(values, vec!["USD", "EUR", "JPY"]);
    }

    #[test]
    fn decimal_string_formats_small_rates_without_exponent() {
        let formatted = decimal_string(0.00001234).unwrap();
        assert_eq!(formatted, "0.00001234");
    }

    #[test]
    fn decimal_string_rejects_invalid_values() {
        assert!(decimal_string(f64::NAN).is_err());
        assert!(decimal_string(-1.0).is_err());
        assert!(decimal_string(0.0).is_err());
    }
}
