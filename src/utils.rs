// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use comfy_table::{presets::UTF8_FULL, Cell, Table};
use rusqlite::{params, Connection, OptionalExtension};
use rust_decimal::Decimal;

const UA: &str = concat!(
    "moneyclip/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/alphavelocity/moneyclip)"
);

pub fn http_client() -> Result<reqwest::blocking::Client> {
    let c = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(UA)
        .build()?;
    Ok(c)
}

pub fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("Invalid date '{}', expected YYYY-MM-DD", s))
}

pub fn parse_month(s: &str) -> Result<String> {
    chrono::NaiveDate::parse_from_str(&format!("{}-01", s), "%Y-%m-%d")
        .with_context(|| format!("Invalid month '{}', expected YYYY-MM", s))?;
    Ok(s.to_string())
}

pub fn parse_decimal(s: &str) -> Result<Decimal> {
    s.parse::<Decimal>()
        .with_context(|| format!("Invalid decimal '{}'", s))
}

#[allow(dead_code)]
pub fn fmt_money(d: &Decimal, ccy: &str) -> String {
    format!("{} {}", ccy, d.round_dp(2))
}

pub fn pretty_table(headers: &[&str], rows: Vec<Vec<String>>) -> Table {
    let mut t = Table::new();
    t.load_preset(UTF8_FULL);
    t.set_header(headers.iter().map(|h| Cell::new(*h)));
    for r in rows {
        t.add_row(r.into_iter().map(Cell::new));
    }
    t
}

pub fn id_for_account(conn: &Connection, name: &str) -> Result<i64> {
    let mut stmt = conn.prepare("SELECT id FROM accounts WHERE name=?1")?;
    let id: i64 = stmt
        .query_row(params![name], |r| r.get(0))
        .with_context(|| format!("Account '{}' not found", name))?;
    Ok(id)
}

pub fn id_for_category(conn: &Connection, name: &str) -> Result<i64> {
    let mut stmt = conn.prepare("SELECT id FROM categories WHERE name=?1")?;
    let id: i64 = stmt
        .query_row(params![name], |r| r.get(0))
        .with_context(|| format!("Category '{}' not found", name))?;
    Ok(id)
}

pub fn id_for_asset(conn: &Connection, ticker: &str) -> Result<i64> {
    let mut stmt = conn.prepare("SELECT id FROM assets WHERE ticker=?1")?;
    let id: i64 = stmt
        .query_row(params![ticker], |r| r.get(0))
        .with_context(|| format!("Asset '{}' not found", ticker))?;
    Ok(id)
}

// Base currency settings
pub fn get_base_currency(conn: &Connection) -> Result<String> {
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key='base_currency'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v.unwrap_or_else(|| "USD".to_string()))
}

pub fn set_base_currency(conn: &Connection, ccy: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES('base_currency', ?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![ccy],
    )?;
    Ok(())
}

/// Convert an amount from 'from_ccy' to 'to_ccy' using the closest on-or-before rate.
/// We store base->quote rates. If pair not found directly, we attempt via the base currency hub.
pub fn fx_convert(
    conn: &Connection,
    date: NaiveDate,
    amount: Decimal,
    from_ccy: &str,
    to_ccy: &str,
) -> Result<Decimal> {
    if from_ccy == to_ccy {
        return Ok(amount);
    }
    let hub = get_base_currency(conn)?;

    fn find_rate(
        conn: &Connection,
        date: NaiveDate,
        base: &str,
        quote: &str,
    ) -> Result<Option<Decimal>> {
        let mut stmt = conn.prepare(
            "SELECT rate FROM fx_rates WHERE base=?1 AND quote=?2 AND date<=?3 ORDER BY date DESC LIMIT 1"
        )?;
        let r: Option<String> = stmt
            .query_row(params![base, quote, date.to_string()], |r| r.get(0))
            .optional()?;
        if let Some(s) = r {
            let d = s
                .parse::<Decimal>()
                .with_context(|| format!("Invalid rate '{}' for {}/{}", s, base, quote))?;
            Ok(Some(d))
        } else {
            Ok(None)
        }
    }

    if to_ccy == hub {
        if let Some(r) = find_rate(conn, date, &hub, from_ccy)? {
            if r.is_zero() {
                return Ok(amount);
            }
            return Ok(amount / r);
        }
    } else if from_ccy == hub {
        if let Some(r) = find_rate(conn, date, &hub, to_ccy)? {
            return Ok(amount * r);
        }
    } else {
        let base_amt = fx_convert(conn, date, amount, from_ccy, &hub)?;
        return fx_convert(conn, date, base_amt, &hub, to_ccy);
    }

    // Try reciprocal last
    if let Some(r) = find_rate(conn, date, to_ccy, from_ccy)? {
        if r.is_zero() {
            return Ok(amount);
        }
        return Ok(amount / r);
    }

    Ok(amount)
}

pub fn month_end(month: &str) -> Result<NaiveDate> {
    let parts: Vec<&str> = month.split('-').collect();
    if parts.len() != 2 {
        return Err(anyhow::anyhow!("Invalid month '{}'", month));
    }
    let y: i32 = parts[0].parse()?;
    let m: u32 = parts[1].parse()?;
    let last_day = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if chrono::NaiveDate::from_ymd_opt(y, 2, 29).is_some() {
                29
            } else {
                28
            }
        }
        _ => return Err(anyhow::anyhow!("Invalid month number {}", m)),
    };
    NaiveDate::from_ymd_opt(y, m, last_day)
        .ok_or_else(|| anyhow::anyhow!("Invalid month '{}'", month))
}

use regex::Regex;

pub fn maybe_print_json<T: serde::Serialize>(
    json_flag: bool,
    jsonl_flag: bool,
    v: &T,
) -> Result<bool> {
    if json_flag {
        println!("{}", serde_json::to_string_pretty(v)?);
        return Ok(true);
    }
    if jsonl_flag {
        // If v is an array, stream each element; else stream single line
        let val = serde_json::to_value(v)?;
        if let Some(arr) = val.as_array() {
            for item in arr {
                println!("{}", serde_json::to_string(item)?);
            }
        } else {
            println!("{}", serde_json::to_string(&val)?);
        }
        return Ok(true);
    }
    Ok(false)
}

pub fn apply_import_rules(
    conn: &Connection,
    payee: &str,
    memo: Option<&str>,
) -> Result<(Option<i64>, Option<String>)> {
    let mut stmt =
        conn.prepare("SELECT id, pattern, category_id, payee_rewrite FROM rules ORDER BY id DESC")?;
    let mut cur = stmt.query([])?;
    let hay = if let Some(m) = memo {
        format!("{} {}", payee, m)
    } else {
        payee.to_string()
    };
    while let Some(r) = cur.next()? {
        let _id: i64 = r.get(0)?;
        let pat: String = r.get(1)?;
        let cat: Option<i64> = r.get(2)?;
        let rewrite: Option<String> = r.get(3)?;
        if let Ok(re) = Regex::new(&pat) {
            if re.is_match(&hay) {
                return Ok((cat, rewrite));
            }
        }
    }
    Ok((None, None))
}
