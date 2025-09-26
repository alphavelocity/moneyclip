// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::{
    http_client, id_for_account, id_for_asset, parse_date, parse_decimal, pretty_table,
};
use anyhow::Result;
use chrono::Utc;
use rusqlite::{Connection, params};
use serde::Deserialize;

pub fn handle(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("add-asset", sub)) => add_asset(conn, sub)?,
        Some(("list-assets", _)) => list_assets(conn)?,
        Some(("trade", sub)) => trade(conn, sub)?,
        Some(("value", sub)) => value(conn, sub)?,
        Some(("tax", sub)) => tax_cg(conn, sub)?,
        Some(("price", sub)) => price_cmd(conn, sub)?,
        _ => {}
    }
    Ok(())
}

fn add_asset(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let ticker = sub.get_one::<String>("ticker").unwrap();
    let name = sub.get_one::<String>("name").unwrap();
    let currency = sub.get_one::<String>("currency").unwrap();
    conn.execute(
        "INSERT INTO assets(ticker, name, currency) VALUES (?1,?2,?3)",
        params![ticker, name, currency],
    )?;
    println!("Added asset {} ({}) {}", ticker, name, currency);
    Ok(())
}

fn list_assets(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("SELECT ticker, name, currency FROM assets ORDER BY ticker")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut data = Vec::new();
    for row in rows {
        let (t, n, c) = row?;
        data.push(vec![t, n, c]);
    }
    println!("{}", pretty_table(&["Ticker", "Name", "CCY"], data));
    Ok(())
}

fn trade(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("buy", sub)) => record_trade(conn, sub, "buy"),
        Some(("sell", sub)) => record_trade(conn, sub, "sell"),
        _ => Ok(()),
    }
}

fn record_trade(conn: &Connection, sub: &clap::ArgMatches, side: &str) -> Result<()> {
    let date = parse_date(sub.get_one::<String>("date").unwrap())?;
    let ticker = sub.get_one::<String>("ticker").unwrap();
    let account = sub.get_one::<String>("account").unwrap();
    let qty = parse_decimal(sub.get_one::<String>("quantity").unwrap())?;
    let price = parse_decimal(sub.get_one::<String>("price").unwrap())?;
    let fees = parse_decimal(sub.get_one::<String>("fees").unwrap_or(&"0".to_string()))?;

    let asset_id = id_for_asset(conn, ticker)?;
    let account_id = id_for_account(conn, account)?;

    conn.execute(
        "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            date.to_string(),
            asset_id,
            account_id,
            qty.to_string(),
            price.to_string(),
            fees.to_string(),
            side
        ],
    )?;
    println!(
        "Recorded {} {} x {} @ {} (fees {})",
        side, qty, ticker, price, fees
    );
    Ok(())
}

fn value(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let live = sub.get_flag("live");
    if live {
        fetch_prices(conn)?;
    }
    let mut stmt = conn.prepare(
        "SELECT a.ticker, a.currency,
                IFNULL((SELECT price FROM prices p WHERE p.asset_id=a.id ORDER BY as_of DESC LIMIT 1), '0') as last_price,
                IFNULL((SELECT SUM(CASE WHEN side='buy' THEN CAST(quantity AS REAL) ELSE -CAST(quantity AS REAL) END) FROM trades WHERE asset_id=a.id), 0) as qty
         FROM assets a ORDER BY a.ticker")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, f64>(3)?,
        ))
    })?;
    let mut data = Vec::new();
    for row in rows {
        let (ticker, ccy, px_s, qty_f) = row?;
        let px: f64 = px_s.parse().unwrap_or(0.0);
        let val = px * qty_f;
        data.push(vec![
            ticker,
            ccy,
            format!("{:.4}", qty_f),
            format!("{:.2}", px),
            format!("{:.2}", val),
        ]);
    }
    println!(
        "{}",
        pretty_table(&["Ticker", "CCY", "Qty", "Price", "Value"], data)
    );
    Ok(())
}

fn tax_cg(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let year = sub.get_one::<String>("year").unwrap();
    let mut stmt = conn.prepare(
        "SELECT a.ticker, t.date, t.quantity, t.price, t.fees, a.currency
         FROM trades t JOIN assets a ON t.asset_id=a.id
         WHERE t.side='sell' AND substr(t.date,1,4)=?1 ORDER BY a.ticker, t.date",
    )?;
    let sells = stmt.query_map([year], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;

    let mut lot_stmt = conn.prepare(
        "SELECT date, quantity, price, fees FROM trades t JOIN assets a ON t.asset_id=a.id
         WHERE a.ticker=?1 AND t.side='buy' ORDER BY date",
    )?;

    let mut table_rows = Vec::new();

    for sell in sells {
        let (ticker, s_date, s_qty_s, s_price_s, s_fees_s, ccy) = sell?;
        let mut remaining = s_qty_s
            .parse::<rust_decimal::Decimal>()
            .unwrap_or(rust_decimal::Decimal::ZERO);
        let sell_price = s_price_s
            .parse::<rust_decimal::Decimal>()
            .unwrap_or(rust_decimal::Decimal::ZERO);
        let sell_fees = s_fees_s
            .parse::<rust_decimal::Decimal>()
            .unwrap_or(rust_decimal::Decimal::ZERO);
        let mut lots = lot_stmt.query_map([&ticker], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;

        let mut realized = rust_decimal::Decimal::ZERO;

        while remaining > rust_decimal::Decimal::ZERO {
            let next_row = lots.next();
            if let Some(l) = next_row {
                let (_b_date, b_qty_s, b_price_s, b_fees_s) = l?;
                let mut b_qty = b_qty_s
                    .parse::<rust_decimal::Decimal>()
                    .unwrap_or(rust_decimal::Decimal::ZERO);
                let b_price = b_price_s
                    .parse::<rust_decimal::Decimal>()
                    .unwrap_or(rust_decimal::Decimal::ZERO);
                let b_fees = b_fees_s
                    .parse::<rust_decimal::Decimal>()
                    .unwrap_or(rust_decimal::Decimal::ZERO);
                if b_qty.is_zero() {
                    continue;
                }
                let use_qty = remaining.min(b_qty);
                let buy_cost = (b_price * use_qty) + (b_fees * (use_qty / b_qty));
                let sell_proceeds = (sell_price * use_qty) - (sell_fees * (use_qty / remaining));
                realized += sell_proceeds - buy_cost;
                remaining -= use_qty;
                b_qty -= use_qty;
            } else {
                break;
            }
        }

        table_rows.push(vec![ticker, s_date, ccy, format!("{:.2}", realized)]);
    }

    println!(
        "{}",
        pretty_table(&["Ticker", "Sell Date", "CCY", "Realized Gain"], table_rows)
    );
    Ok(())
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct YahooResponse {
    quoteResponse: QuoteResponse,
}
#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct QuoteResponse {
    result: Vec<YahooQuote>,
}
#[derive(Debug, Deserialize)]
struct YahooQuote {
    #[serde(rename = "regularMarketPrice")]
    regular_market_price: Option<f64>,
    symbol: Option<String>,
    #[serde(rename = "currency")]
    _currency: Option<String>,
}

fn price_cmd(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("fetch", _)) => fetch_prices(conn),
        Some(("list", _)) => list_prices(conn),
        _ => Ok(()),
    }
}

fn list_prices(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT a.ticker, p.as_of, p.price, a.currency, p.source
         FROM prices p JOIN assets a ON p.asset_id=a.id
         ORDER BY p.as_of DESC LIMIT 50",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut data = Vec::new();
    for row in rows {
        let (tic, ts, px, ccy, src) = row?;
        data.push(vec![tic, ts, px, ccy, src]);
    }
    println!(
        "{}",
        pretty_table(&["Ticker", "As Of", "Price", "CCY", "Source"], data)
    );
    Ok(())
}

fn fetch_prices(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id, ticker FROM assets ORDER BY ticker")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;

    let mut tickers = Vec::new();
    let mut map = Vec::new();
    for row in rows {
        let (id, tic) = row?;
        tickers.push(tic.clone());
        map.push((tic, id));
    }
    if tickers.is_empty() {
        println!("No assets to fetch");
        return Ok(());
    }

    let url = format!(
        "https://query1.finance.yahoo.com/v7/finance/quote?symbols={}",
        tickers.join(",")
    );
    let client = http_client()?;
    let resp = client.get(url).send()?.error_for_status()?;
    let yr: YahooResponse = resp.json()?;

    let now = Utc::now().to_rfc3339();
    for q in yr.quoteResponse.result {
        if let (Some(sym), Some(px)) = (q.symbol, q.regular_market_price) {
            if let Some((_t, asset_id)) = map.iter().find(|(t, _id)| *t == sym) {
                conn.execute(
                    "INSERT INTO prices(asset_id, as_of, price, source) VALUES (?1, ?2, ?3, 'yahoo')",
                    params![asset_id, &now, px.to_string()],
                )?;
            }
        }
    }
    println!("Fetched prices at {}", now);
    Ok(())
}
