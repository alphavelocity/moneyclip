// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use anyhow::{Context, Result, anyhow, ensure};
use chrono::NaiveDate;
use comfy_table::{Cell, Table, presets::UTF8_FULL};
use rusqlite::{Connection, OptionalExtension, ffi, params};
use rust_decimal::Decimal;
use std::{
    borrow::Cow,
    collections::{BinaryHeap, HashMap, VecDeque, hash_map::Entry},
    io::{self, Write},
    sync::{Arc, RwLock},
};

use once_cell::sync::{Lazy, OnceCell};

const UA: &str = concat!(
    "moneyclip/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/alphavelocity/moneyclip)"
);

static HTTP_CLIENT: OnceCell<reqwest::blocking::Client> = OnceCell::new();

pub fn http_client() -> Result<&'static reqwest::blocking::Client> {
    HTTP_CLIENT.get_or_try_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent(UA)
            .build()
            .map_err(|err| anyhow!("Failed to build HTTP client: {err}"))
    })
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
    let mut stmt = conn.prepare_cached("SELECT id FROM accounts WHERE name=?1")?;
    let id: i64 = stmt
        .query_row(params![name], |r| r.get(0))
        .with_context(|| format!("Account '{}' not found", name))?;
    Ok(id)
}

pub fn id_for_category(conn: &Connection, name: &str) -> Result<i64> {
    let mut stmt = conn.prepare_cached("SELECT id FROM categories WHERE name=?1")?;
    let id: i64 = stmt
        .query_row(params![name], |r| r.get(0))
        .with_context(|| format!("Category '{}' not found", name))?;
    Ok(id)
}

pub fn id_for_asset(conn: &Connection, ticker: &str) -> Result<i64> {
    let mut stmt = conn.prepare_cached("SELECT id FROM assets WHERE ticker=?1")?;
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

struct FxGraph {
    adjacency: Vec<Vec<(usize, Decimal)>>,
    currency_index: HashMap<String, usize>,
}

struct FxGraphCacheEntry {
    data_version: i64,
    total_changes: i64,
    graphs: HashMap<NaiveDate, Arc<FxGraph>>,
    order: VecDeque<NaiveDate>,
}

static FX_GRAPH_CACHE: Lazy<RwLock<HashMap<usize, FxGraphCacheEntry>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

const MAX_FX_GRAPH_CACHE_DATES: usize = 32;

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
    let graph = fx_graph_for(conn, date)?;

    let Some(&from_idx) = graph.currency_index.get(from_ccy) else {
        return Err(anyhow!(
            "No FX rate path from {} to {} on or before {}",
            from_ccy,
            to_ccy,
            date
        ));
    };
    let Some(&to_idx) = graph.currency_index.get(to_ccy) else {
        return Err(anyhow!(
            "No FX rate path from {} to {} on or before {}",
            from_ccy,
            to_ccy,
            date
        ));
    };

    let magnitude = amount.abs();
    if magnitude.is_zero() {
        return Ok(amount);
    }

    let adjacency = &graph.adjacency;
    let mut best = vec![Decimal::ZERO; adjacency.len()];
    let mut heap: BinaryHeap<(Decimal, usize)> = BinaryHeap::new();
    best[from_idx] = magnitude;
    heap.push((magnitude, from_idx));

    while let Some((current_amount, idx)) = heap.pop() {
        if current_amount < best[idx] {
            continue;
        }
        if idx == to_idx {
            let signed = if amount.is_sign_negative() {
                -current_amount
            } else {
                current_amount
            };
            return Ok(signed);
        }

        for &(next_idx, rate) in &adjacency[idx] {
            let next_amount = current_amount * rate;
            if next_amount > best[next_idx] {
                best[next_idx] = next_amount;
                heap.push((next_amount, next_idx));
            }
        }
    }

    Err(anyhow!(
        "No FX rate path from {} to {} on or before {}",
        from_ccy,
        to_ccy,
        date
    ))
}

fn fx_graph_for(conn: &Connection, date: NaiveDate) -> Result<Arc<FxGraph>> {
    let conn_key = unsafe { conn.handle() as usize };
    let current_version = data_version(conn)?;
    let change_count = total_changes(conn);

    if let Some(graph) = {
        let cache = FX_GRAPH_CACHE.read().unwrap();
        cache
            .get(&conn_key)
            .filter(|entry| {
                entry.data_version == current_version && entry.total_changes == change_count
            })
            .and_then(|entry| entry.graphs.get(&date).cloned())
    } {
        return Ok(graph);
    }

    let graph = Arc::new(build_fx_graph(conn, date)?);
    let refreshed_version = data_version(conn)?;
    let refreshed_changes = total_changes(conn);

    let mut cache = FX_GRAPH_CACHE.write().unwrap();
    let entry = cache.entry(conn_key).or_insert_with(|| FxGraphCacheEntry {
        data_version: refreshed_version,
        total_changes: refreshed_changes,
        graphs: HashMap::new(),
        order: VecDeque::new(),
    });

    if entry.data_version != refreshed_version || entry.total_changes != refreshed_changes {
        entry.data_version = refreshed_version;
        entry.total_changes = refreshed_changes;
        entry.graphs.clear();
        entry.order.clear();
    }

    entry.order.retain(|d| d != &date);
    entry.order.push_back(date);
    entry.graphs.insert(date, Arc::clone(&graph));

    while entry.order.len() > MAX_FX_GRAPH_CACHE_DATES {
        if let Some(oldest) = entry.order.pop_front() {
            entry.graphs.remove(&oldest);
        }
    }

    Ok(graph)
}

fn build_fx_graph(conn: &Connection, date: NaiveDate) -> Result<FxGraph> {
    let date_str = date.format("%Y-%m-%d").to_string();

    let mut stmt = conn.prepare_cached(
        "SELECT base, quote, rate FROM (
             SELECT base, quote, rate,
                    ROW_NUMBER() OVER (PARTITION BY base, quote ORDER BY date DESC) AS rn
             FROM fx_rates
             WHERE date <= ?1
         )
         WHERE rn = 1",
    )?;
    let mut rows = stmt.query(params![&date_str])?;
    let mut adjacency: Vec<Vec<(usize, Decimal)>> = Vec::new();
    let mut currency_index: HashMap<String, usize> = HashMap::new();

    while let Some(row) = rows.next()? {
        let base: String = row.get(0)?;
        let quote: String = row.get(1)?;
        let rate_str: String = row.get(2)?;
        let rate = rate_str
            .parse::<Decimal>()
            .with_context(|| format!("Invalid rate '{}' for {}/{}", rate_str, base, quote))?;
        ensure!(
            rate > Decimal::ZERO,
            "FX rate for {}/{} on or before {} is not positive",
            base,
            quote,
            date
        );

        let base_idx = match currency_index.entry(base) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(entry) => {
                let idx = adjacency.len();
                adjacency.push(Vec::new());
                entry.insert(idx);
                idx
            }
        };
        let quote_idx = match currency_index.entry(quote) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(entry) => {
                let idx = adjacency.len();
                adjacency.push(Vec::new());
                entry.insert(idx);
                idx
            }
        };

        adjacency[base_idx].push((quote_idx, rate));
        adjacency[quote_idx].push((base_idx, Decimal::ONE / rate));
    }

    Ok(FxGraph {
        adjacency,
        currency_index,
    })
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

#[derive(Clone)]
struct CompiledRule {
    regex: Regex,
    category_id: Option<i64>,
    rewrite: Option<String>,
}

struct RuleCacheEntry {
    rules: Arc<Vec<CompiledRule>>,
    data_version: i64,
}

static RULE_CACHE: Lazy<RwLock<HashMap<usize, RuleCacheEntry>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

const MAX_RULE_CACHE_ENTRIES: usize = 32;

fn data_version(conn: &Connection) -> Result<i64> {
    conn.query_row("PRAGMA data_version", [], |r| r.get(0))
        .context("Fetch PRAGMA data_version")
}

fn total_changes(conn: &Connection) -> i64 {
    unsafe { ffi::sqlite3_total_changes(conn.handle()) as i64 }
}

pub fn maybe_print_json<T: serde::Serialize>(
    json_flag: bool,
    jsonl_flag: bool,
    rows: &[T],
) -> Result<bool> {
    if !json_flag && !jsonl_flag {
        return Ok(false);
    }

    let stdout = io::stdout();
    let mut handle = stdout.lock();
    maybe_print_json_to(json_flag, jsonl_flag, rows, &mut handle)
}

fn maybe_print_json_to<T, W>(
    json_flag: bool,
    jsonl_flag: bool,
    rows: &[T],
    mut writer: W,
) -> Result<bool>
where
    T: serde::Serialize,
    W: Write,
{
    if json_flag {
        serde_json::to_writer_pretty(&mut writer, rows)?;
        writer.write_all(b"\n")?;
        return Ok(true);
    }

    if jsonl_flag {
        for row in rows {
            serde_json::to_writer(&mut writer, row)?;
            writer.write_all(b"\n")?;
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
    let hay = memo
        .map(|m| Cow::Owned(format!("{} {}", payee, m)))
        .unwrap_or_else(|| Cow::Borrowed(payee));

    let conn_key = unsafe { conn.handle() as usize };
    let current_version = data_version(conn)?;

    if let Some(rules) = {
        let cache = RULE_CACHE.read().unwrap();
        cache
            .get(&conn_key)
            .filter(|entry| entry.data_version == current_version)
            .map(|entry| Arc::clone(&entry.rules))
    } {
        return Ok(match_rules(&rules, hay.as_ref()));
    }

    let compiled = Arc::new(load_rules(conn)?);
    let refreshed_version = data_version(conn)?;
    {
        let mut cache = RULE_CACHE.write().unwrap();
        cache.insert(
            conn_key,
            RuleCacheEntry {
                rules: Arc::clone(&compiled),
                data_version: refreshed_version,
            },
        );
        if cache.len() > MAX_RULE_CACHE_ENTRIES {
            let mut candidates: Vec<usize> = cache
                .keys()
                .filter(|key| **key != conn_key)
                .copied()
                .collect();
            while cache.len() > MAX_RULE_CACHE_ENTRIES {
                if let Some(key) = candidates.pop() {
                    cache.remove(&key);
                } else {
                    break;
                }
            }
        }
    }

    Ok(match_rules(&compiled, hay.as_ref()))
}

fn load_rules(conn: &Connection) -> Result<Vec<CompiledRule>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, pattern, category_id, payee_rewrite FROM rules ORDER BY id DESC",
    )?;
    let mut rows = stmt.query([])?;
    let mut compiled = Vec::new();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let pattern: String = row.get(1)?;
        let category_id: Option<i64> = row.get(2)?;
        let rewrite: Option<String> = row.get(3)?;
        let regex = Regex::new(&pattern).map_err(|err| {
            anyhow!(
                "Invalid regex pattern '{}' for rule {}: {}",
                pattern,
                id,
                err
            )
        })?;
        compiled.push(CompiledRule {
            regex,
            category_id,
            rewrite,
        });
    }
    Ok(compiled)
}

fn match_rules(rules: &[CompiledRule], hay: &str) -> (Option<i64>, Option<String>) {
    for rule in rules {
        if rule.regex.is_match(hay) {
            return (rule.category_id, rule.rewrite.clone());
        }
    }
    (None, None)
}

pub fn invalidate_rule_cache(_conn: &Connection) {
    let mut cache = RULE_CACHE.write().unwrap();
    cache.clear();
}

#[cfg(test)]
mod tests {
    use super::maybe_print_json_to;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Row {
        value: i32,
    }

    #[test]
    fn json_mode_writes_pretty_array() {
        let rows = vec![Row { value: 1 }];
        let mut buf = Vec::new();
        let printed = maybe_print_json_to(true, false, &rows, &mut buf).unwrap();
        assert!(printed);
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[\n  {\n    \"value\": 1\n  }\n]\n"
        );
    }

    #[test]
    fn jsonl_mode_streams_each_row() {
        let rows = vec![Row { value: 1 }, Row { value: 2 }];
        let mut buf = Vec::new();
        let printed = maybe_print_json_to(false, true, &rows, &mut buf).unwrap();
        assert!(printed);
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "{\"value\":1}\n{\"value\":2}\n"
        );
    }

    #[test]
    fn no_flags_writes_nothing() {
        let rows = vec![Row { value: 1 }];
        let mut buf = Vec::new();
        let printed = maybe_print_json_to(false, false, &rows, &mut buf).unwrap();
        assert!(!printed);
        assert!(buf.is_empty());
    }
}
