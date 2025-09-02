// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::{get_base_currency, http_client, pretty_table, set_base_currency};
use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Deserialize;

pub fn handle(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("set-base", sub)) => {
            let ccy = sub.get_one::<String>("currency").unwrap().to_uppercase();
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
    for sql in [
        "SELECT DISTINCT currency FROM accounts",
        "SELECT DISTINCT currency FROM assets",
        "SELECT DISTINCT currency FROM transactions",
    ] {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for row in rows {
            let c: String = row?;
            if !c.is_empty() && !out.contains(&c) {
                out.push(c);
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
fn fetch_rates(conn: &Connection, days: usize) -> Result<()> {
    let base = get_base_currency(conn)?;
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
    for (date, mp) in s.rates {
        for (quote, rate) in mp {
            conn.execute(
                "INSERT OR IGNORE INTO fx_rates(date, base, quote, rate) VALUES (?1, ?2, ?3, ?4)",
                params![date, base, quote, rate.to_string()],
            )?;
        }
    }
    println!("FX rates fetched via Frankfurter (ECB).");
    Ok(())
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
    let date = crate::utils::parse_date(sub.get_one::<String>("date").unwrap())?;
    let amount = crate::utils::parse_decimal(sub.get_one::<String>("amount").unwrap())?;
    let from = sub.get_one::<String>("from").unwrap().to_uppercase();
    let to = sub.get_one::<String>("to").unwrap().to_uppercase();
    let res = crate::utils::fx_convert(conn, date, amount, &from, &to)?;
    println!("{} {} -> {:.4} {}", amount, from, res, to);
    Ok(())
}
