// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::{
    http_client, id_for_account, id_for_asset, parse_date, parse_decimal, pretty_table,
};
use anyhow::{Context, Result, anyhow};
use chrono::{NaiveDate, Utc};
use rusqlite::{Connection, params};
use serde::Deserialize;
use std::collections::{HashMap, HashSet, hash_map::Entry};

use rust_decimal::Decimal;

pub fn handle(conn: &mut Connection, m: &clap::ArgMatches) -> Result<()> {
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
    let ticker = sub
        .get_one::<String>("ticker")
        .map(|s| s.trim().to_string())
        .unwrap();
    let name = sub
        .get_one::<String>("name")
        .map(|s| s.trim().to_string())
        .unwrap();
    let currency = sub
        .get_one::<String>("currency")
        .map(|s| s.trim().to_string())
        .unwrap();
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
    let date_raw = sub.get_one::<String>("date").unwrap();
    let date = parse_date(date_raw.trim())?;
    let ticker = sub
        .get_one::<String>("ticker")
        .map(|s| s.trim().to_string())
        .unwrap();
    let account = sub
        .get_one::<String>("account")
        .map(|s| s.trim().to_string())
        .unwrap();
    let qty = parse_decimal(sub.get_one::<String>("quantity").unwrap().trim())?.abs();
    let price = parse_decimal(sub.get_one::<String>("price").unwrap().trim())?;
    let fees = match sub.get_one::<String>("fees") {
        Some(raw) => parse_decimal(raw.trim())?,
        None => Decimal::ZERO,
    };

    let asset_id = id_for_asset(conn, &ticker)?;
    let account_id = id_for_account(conn, &account)?;

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

fn value(conn: &mut Connection, sub: &clap::ArgMatches) -> Result<()> {
    if sub.get_flag("live") {
        fetch_prices(conn)?;
    }

    let positions = portfolio_positions(conn)?;
    let rows = positions
        .into_iter()
        .map(|position| {
            vec![
                position.ticker,
                position.currency,
                format!("{:.4}", position.quantity),
                format!("{:.2}", position.last_price),
                format!("{:.2}", position.market_value),
            ]
        })
        .collect();

    println!(
        "{}",
        pretty_table(&["Ticker", "CCY", "Qty", "Price", "Value"], rows)
    );
    Ok(())
}

#[derive(Debug, Clone)]
struct PositionSummary {
    ticker: String,
    currency: String,
    quantity: Decimal,
    last_price: Decimal,
    market_value: Decimal,
}

fn portfolio_positions(conn: &Connection) -> Result<Vec<PositionSummary>> {
    struct AssetRow {
        ticker: String,
        currency: String,
        last_price: Decimal,
    }

    let mut stmt =
        conn.prepare_cached("SELECT id, ticker, currency FROM assets ORDER BY ticker")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;

    let (lower_bound, _) = rows.size_hint();

    let mut assets = Vec::with_capacity(lower_bound);
    let mut index_by_id = HashMap::with_capacity(lower_bound);
    for row in rows {
        let (id, ticker, currency) = row?;
        let idx = assets.len();
        assets.push(AssetRow {
            ticker,
            currency,
            last_price: Decimal::ZERO,
        });
        index_by_id.insert(id, idx);
    }

    if assets.is_empty() {
        return Ok(Vec::new());
    }

    let mut price_stmt = conn.prepare_cached(
        "SELECT asset_id, price FROM (
             SELECT asset_id,
                    price,
                    ROW_NUMBER() OVER (
                        PARTITION BY asset_id
                        ORDER BY as_of DESC, rowid DESC
                    ) AS rn
             FROM prices
         ) WHERE rn = 1",
    )?;
    let price_rows =
        price_stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    for price in price_rows {
        let (asset_id, price_s) = price?;
        let Some(&idx) = index_by_id.get(&asset_id) else {
            continue;
        };
        let asset = &mut assets[idx];
        let ticker = asset.ticker.as_str();
        let last_price = Decimal::from_str_exact(&price_s)
            .with_context(|| format!("Invalid stored price '{}' for asset {}", price_s, ticker))?;
        asset.last_price = last_price;
    }

    let mut net_quantities = vec![Decimal::ZERO; assets.len()];
    let mut trades_stmt = conn.prepare_cached("SELECT asset_id, quantity, side FROM trades")?;
    let trades = trades_stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;

    for trade in trades {
        let (asset_id, qty_s, side) = trade?;
        let Some(&idx) = index_by_id.get(&asset_id) else {
            continue;
        };
        let ticker = assets[idx].ticker.as_str();
        let qty_raw = Decimal::from_str_exact(&qty_s)
            .with_context(|| format!("Invalid trade quantity '{}' for asset {}", qty_s, ticker))?;
        let qty = qty_raw.abs();
        match side.as_str() {
            "buy" => net_quantities[idx] += qty,
            "sell" => net_quantities[idx] -= qty,
            other => {
                return Err(anyhow!(
                    "Unknown trade side '{}' for asset {}",
                    other,
                    ticker
                ));
            }
        }
    }

    let mut positions = Vec::with_capacity(assets.len());

    for (asset, quantity) in assets.into_iter().zip(net_quantities) {
        if quantity.is_zero() {
            continue;
        }

        positions.push(PositionSummary {
            market_value: asset.last_price * quantity,
            ticker: asset.ticker,
            currency: asset.currency,
            last_price: asset.last_price,
            quantity,
        });
    }

    Ok(positions)
}

fn tax_cg(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let year = sub
        .get_one::<String>("year")
        .map(|s| s.trim().to_string())
        .unwrap();
    let rows = realized_gains(conn, &year)?;
    let table_rows = rows
        .into_iter()
        .map(|row| {
            vec![
                row.ticker,
                row.sell_date,
                row.currency,
                format!("{:.2}", row.realized_gain),
            ]
        })
        .collect();

    println!(
        "{}",
        pretty_table(&["Ticker", "Sell Date", "CCY", "Realized Gain"], table_rows)
    );
    Ok(())
}

struct Lot {
    date: NaiveDate,
    remaining: Decimal,
    original_qty: Decimal,
    price: Decimal,
    fees: Decimal,
}

#[derive(Debug)]
struct RealizedGainRow {
    ticker: String,
    sell_date: String,
    currency: String,
    realized_gain: Decimal,
}

struct SellRecord {
    date: NaiveDate,
    quantity: Decimal,
    price: Decimal,
    fees: Decimal,
}

fn load_sells_before(
    stmt: &mut rusqlite::Statement<'_>,
    ticker: &str,
    cutoff: NaiveDate,
) -> Result<Vec<SellRecord>> {
    let rows = stmt.query_map(params![ticker, cutoff.to_string()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut sells = Vec::new();
    for row in rows {
        let (date_s, qty_s, price_s, fee_s) = row?;
        let date = parse_date(&date_s)
            .with_context(|| format!("Invalid sell date '{}' for {}", date_s, ticker))?;
        let qty_raw = Decimal::from_str_exact(&qty_s)
            .with_context(|| format!("Invalid sell quantity '{}' for {}", qty_s, ticker))?;
        let qty = qty_raw.abs();
        if qty.is_zero() {
            continue;
        }
        let price = Decimal::from_str_exact(&price_s)
            .with_context(|| format!("Invalid sell price '{}' for {}", price_s, ticker))?;
        let fees = Decimal::from_str_exact(&fee_s)
            .with_context(|| format!("Invalid sell fees '{}' for {}", fee_s, ticker))?;
        sells.push(SellRecord {
            date,
            quantity: qty,
            price,
            fees,
        });
    }
    Ok(sells)
}

fn match_sell_against_lots(
    ticker: &str,
    lots: &mut [Lot],
    sell_date: NaiveDate,
    sell_qty: Decimal,
    sell_price: Decimal,
    sell_fees: Decimal,
) -> Result<Decimal> {
    let mut remaining = sell_qty;
    if remaining.is_zero() {
        return Ok(Decimal::ZERO);
    }
    let total_qty = sell_qty;
    let mut realized = Decimal::ZERO;
    for lot in lots.iter_mut() {
        if remaining <= Decimal::ZERO {
            break;
        }
        if lot.remaining <= Decimal::ZERO {
            continue;
        }
        if lot.date > sell_date {
            break;
        }
        let use_qty = if remaining < lot.remaining {
            remaining
        } else {
            lot.remaining
        };
        let buy_fee_share = if lot.original_qty.is_zero() {
            Decimal::ZERO
        } else {
            lot.fees * (use_qty / lot.original_qty)
        };
        let buy_cost = (lot.price * use_qty) + buy_fee_share;
        let fee_allocation = if total_qty.is_zero() {
            Decimal::ZERO
        } else {
            sell_fees * (use_qty / total_qty)
        };
        let sell_proceeds = (sell_price * use_qty) - fee_allocation;
        realized += sell_proceeds - buy_cost;
        lot.remaining -= use_qty;
        remaining -= use_qty;
    }

    if remaining > Decimal::ZERO {
        let has_prior_lot = lots.iter().any(|lot| lot.date <= sell_date);
        if has_prior_lot {
            Err(anyhow!(
                "Sell of {} on {} exceeds available lot quantity before or on the sell date",
                ticker,
                sell_date
            ))
        } else {
            Err(anyhow!(
                "No purchase lots dated on or before sell of {} on {}",
                ticker,
                sell_date
            ))
        }
    } else {
        Ok(realized)
    }
}

fn load_buy_lots(stmt: &mut rusqlite::Statement<'_>, ticker: &str) -> Result<Vec<Lot>> {
    let rows = stmt.query_map([ticker], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut lots = Vec::new();
    for row in rows {
        let (date_s, qty_s, price_s, fee_s) = row?;
        let date = parse_date(&date_s)
            .with_context(|| format!("Invalid buy date '{}' for {}", date_s, ticker))?;
        let qty_raw = Decimal::from_str_exact(&qty_s)
            .with_context(|| format!("Invalid buy quantity '{}' for {}", qty_s, ticker))?;
        let qty = qty_raw.abs();
        if qty.is_zero() {
            continue;
        }
        let price = Decimal::from_str_exact(&price_s)
            .with_context(|| format!("Invalid buy price '{}' for {}", price_s, ticker))?;
        let fees = Decimal::from_str_exact(&fee_s)
            .with_context(|| format!("Invalid buy fees '{}' for {}", fee_s, ticker))?;
        lots.push(Lot {
            date,
            remaining: qty,
            original_qty: qty,
            price,
            fees,
        });
    }
    Ok(lots)
}

fn realized_gains(conn: &Connection, year: &str) -> Result<Vec<RealizedGainRow>> {
    let year_int: i32 = year
        .parse()
        .with_context(|| format!("Invalid year '{}'", year))?;
    let year_start =
        chrono::NaiveDate::from_ymd_opt(year_int, 1, 1).context("Invalid year start date")?;

    let mut sell_stmt = conn.prepare(
        "SELECT a.ticker, t.date, t.quantity, t.price, t.fees, a.currency
         FROM trades t JOIN assets a ON t.asset_id=a.id
         WHERE t.side='sell' AND substr(t.date,1,4)=?1 ORDER BY a.ticker, t.date",
    )?;
    let sells = sell_stmt.query_map([year], |r| {
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
        "SELECT t.date, t.quantity, t.price, t.fees FROM trades t JOIN assets a ON t.asset_id=a.id
         WHERE a.ticker=?1 AND t.side='buy' ORDER BY t.date",
    )?;

    let mut prior_sell_stmt = conn.prepare(
        "SELECT t.date, t.quantity, t.price, t.fees FROM trades t JOIN assets a ON t.asset_id=a.id
         WHERE a.ticker=?1 AND t.side='sell' AND t.date<?2 ORDER BY t.date",
    )?;

    let mut lots_cache: HashMap<String, Vec<Lot>> = HashMap::new();
    let mut pre_consumed: HashSet<String> = HashSet::new();
    let mut results = Vec::new();

    for sell in sells {
        let (ticker, sell_date, qty_s, price_s, fee_s, currency) = sell?;
        let sell_qty_raw = Decimal::from_str_exact(&qty_s)
            .with_context(|| format!("Invalid sell quantity '{}' for {}", qty_s, ticker))?;
        let sell_qty = sell_qty_raw.abs();
        if sell_qty.is_zero() {
            results.push(RealizedGainRow {
                ticker,
                sell_date,
                currency,
                realized_gain: Decimal::ZERO,
            });
            continue;
        }
        let sell_price = Decimal::from_str_exact(&price_s)
            .with_context(|| format!("Invalid sell price '{}' for {}", price_s, ticker))?;
        let sell_fees = Decimal::from_str_exact(&fee_s)
            .with_context(|| format!("Invalid sell fees '{}' for {}", fee_s, ticker))?;
        let sell_date_parsed = parse_date(&sell_date)
            .with_context(|| format!("Invalid sell date '{}' for {}", sell_date, ticker))?;

        let lots = match lots_cache.entry(ticker.clone()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let loaded = load_buy_lots(&mut lot_stmt, &ticker)?;
                entry.insert(loaded)
            }
        };

        if lots.is_empty() {
            return Err(anyhow!(
                "No purchase lots available for sell of {} on {}",
                ticker,
                sell_date
            ));
        }

        if pre_consumed.insert(ticker.clone()) {
            let prior_sells = load_sells_before(&mut prior_sell_stmt, &ticker, year_start)?;
            for sell in prior_sells {
                match_sell_against_lots(
                    &ticker,
                    lots,
                    sell.date,
                    sell.quantity,
                    sell.price,
                    sell.fees,
                )?;
            }
        }

        let realized = match_sell_against_lots(
            &ticker,
            lots,
            sell_date_parsed,
            sell_qty,
            sell_price,
            sell_fees,
        )?;

        results.push(RealizedGainRow {
            ticker,
            sell_date,
            currency,
            realized_gain: realized,
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Command, arg};
    use rusqlite::Connection;
    use std::str::FromStr;

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE accounts(id INTEGER PRIMARY KEY, name TEXT, type TEXT, currency TEXT);
            CREATE TABLE assets(id INTEGER PRIMARY KEY AUTOINCREMENT, ticker TEXT, name TEXT, currency TEXT);
            CREATE TABLE trades(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL,
                asset_id INTEGER NOT NULL,
                account_id INTEGER NOT NULL,
                quantity TEXT NOT NULL,
                price TEXT NOT NULL,
                fees TEXT NOT NULL DEFAULT '0',
                side TEXT NOT NULL
            );
            CREATE TABLE prices(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                asset_id INTEGER NOT NULL,
                as_of TEXT NOT NULL,
                price TEXT NOT NULL,
                source TEXT NOT NULL
            );
            "#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn record_trade_trims_cli_inputs() {
        let conn = setup_conn();
        conn.execute(
            "INSERT INTO accounts(id, name, type, currency) VALUES (1, 'Broker', 'broker', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets(id, ticker, name, currency) VALUES (1, 'ABC', 'ABC Corp', 'USD')",
            [],
        )
        .unwrap();

        let buy_matches = Command::new("buy")
            .arg(arg!(--date <DATE>).required(true))
            .arg(arg!(--ticker <TICKER>).required(true))
            .arg(arg!(--account <ACCOUNT>).required(true))
            .arg(arg!(--quantity <QTY>).required(true))
            .arg(arg!(--price <PRICE>).required(true))
            .arg(arg!(--fees <FEES>).required(false))
            .try_get_matches_from([
                "buy",
                "--date",
                " 2025-01-01 ",
                "--ticker",
                " ABC ",
                "--account",
                " Broker ",
                "--quantity",
                " 100 ",
                "--price",
                " 10.00 ",
                "--fees",
                " 1.50 ",
            ])
            .unwrap();

        record_trade(&conn, &buy_matches, "buy").unwrap();

        let (date, quantity, price, fees): (String, String, String, String) = conn
            .query_row(
                "SELECT date, quantity, price, fees FROM trades WHERE id=1",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(date, "2025-01-01");
        assert_eq!(quantity, "100");
        assert_eq!(price, "10.00");
        assert_eq!(fees, "1.50");
    }

    #[test]
    fn portfolio_positions_skip_zero_quantity_assets() {
        let conn = setup_conn();
        conn.execute(
            "INSERT INTO accounts(id, name, type, currency) VALUES (1, 'Broker', 'broker', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets(id, ticker, name, currency) VALUES (1, 'XYZ', 'XYZ Corp', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO prices(asset_id, as_of, price, source) VALUES (1, '2025-01-01', '123.45', 'test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES (?1, ?2, ?3, ?4, ?5, '0', ?6)",
            (
                "2025-01-02",
                1,
                1,
                Decimal::from_str("10").unwrap().to_string(),
                Decimal::from_str("100").unwrap().to_string(),
                "buy",
            ),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES (?1, ?2, ?3, ?4, ?5, '0', ?6)",
            (
                "2025-01-03",
                1,
                1,
                Decimal::from_str("10").unwrap().to_string(),
                Decimal::from_str("110").unwrap().to_string(),
                "sell",
            ),
        )
        .unwrap();

        let positions = portfolio_positions(&conn).unwrap();
        assert!(positions.is_empty());
    }

    #[test]
    fn portfolio_positions_compute_decimal_values() {
        let conn = setup_conn();
        conn.execute(
            "INSERT INTO accounts(id, name, type, currency) VALUES (1, 'Broker', 'broker', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets(id, ticker, name, currency) VALUES (1, 'ABC', 'ABC Corp', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO prices(asset_id, as_of, price, source) VALUES (1, '2025-01-01T00:00:00Z', '15.4321', 'manual')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side)
             VALUES ('2024-01-10', 1, 1, '1.1234', '10', '0.10', 'buy')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side)
             VALUES ('2024-02-15', 1, 1, '2.3456', '11', '0.20', 'buy')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side)
             VALUES ('2024-03-20', 1, 1, '0.6789', '12', '0.05', 'sell')",
            [],
        )
        .unwrap();

        let positions = super::portfolio_positions(&conn).unwrap();
        assert_eq!(positions.len(), 1);
        let pos = &positions[0];
        assert_eq!(pos.ticker, "ABC");
        assert_eq!(pos.currency, "USD");

        let expected_qty = Decimal::from_str_exact("2.7901").unwrap();
        let expected_price = Decimal::from_str_exact("15.4321").unwrap();
        let expected_value = expected_qty * expected_price;

        assert_eq!(pos.quantity, expected_qty);
        assert_eq!(pos.last_price, expected_price);
        assert_eq!(pos.market_value, expected_value);
    }

    #[test]
    fn realized_gains_respect_fifo_across_multiple_sells() {
        let conn = setup_conn();
        conn.execute(
            "INSERT INTO accounts(id, name, type, currency) VALUES (1, 'Broker', 'broker', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets(id, ticker, name, currency) VALUES (1, 'ABC', 'ABC Corp', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2020-01-01', 1, 1, '100', '10', '5', 'buy')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2021-06-01', 1, 1, '50', '15', '2', 'buy')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2025-01-10', 1, 1, '80', '20', '4', 'sell')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2025-06-15', 1, 1, '50', '25', '5', 'sell')",
            [],
        )
        .unwrap();

        let rows = realized_gains(&conn, "2025").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ticker, "ABC");
        assert_eq!(rows[0].sell_date, "2025-01-10");
        assert_eq!(rows[0].currency, "USD");
        let expected_first = Decimal::from_str("792").unwrap();
        assert_eq!(rows[0].realized_gain, expected_first);
        let expected_second = Decimal::from_str("592.8").unwrap();
        assert_eq!(rows[1].realized_gain, expected_second);
    }

    #[test]
    fn realized_gains_error_when_lots_missing() {
        let conn = setup_conn();
        conn.execute(
            "INSERT INTO accounts(id, name, type, currency) VALUES (1, 'Broker', 'broker', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets(id, ticker, name, currency) VALUES (1, 'XYZ', 'XYZ Corp', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2025-03-01', 1, 1, '10', '30', '1', 'sell')",
            [],
        )
        .unwrap();

        let err = realized_gains(&conn, "2025").unwrap_err();
        assert!(
            err.to_string()
                .contains("No purchase lots available for sell of XYZ on 2025-03-01")
        );
    }

    #[test]
    fn realized_gains_do_not_use_future_buys() {
        let conn = setup_conn();
        conn.execute(
            "INSERT INTO accounts(id, name, type, currency) VALUES (1, 'Broker', 'broker', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets(id, ticker, name, currency) VALUES (1, 'FUT', 'Future Corp', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2025-12-01', 1, 1, '100', '10', '0', 'buy')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2025-06-01', 1, 1, '50', '20', '0', 'sell')",
            [],
        )
        .unwrap();

        let err = realized_gains(&conn, "2025").unwrap_err();
        assert!(
            err.to_string()
                .contains("No purchase lots dated on or before sell of FUT on 2025-06-01")
        );
    }

    #[test]
    fn realized_gains_account_for_prior_year_sells() {
        let conn = setup_conn();
        conn.execute(
            "INSERT INTO accounts(id, name, type, currency) VALUES (1, 'Broker', 'broker', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets(id, ticker, name, currency) VALUES (1, 'HIST', 'History Corp', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2023-01-01', 1, 1, '100', '10', '0', 'buy')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2024-06-01', 1, 1, '60', '20', '0', 'sell')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2025-02-01', 1, 1, '50', '25', '0', 'sell')",
            [],
        )
        .unwrap();

        let err = realized_gains(&conn, "2025").unwrap_err();
        assert!(err.to_string().contains(
            "Sell of HIST on 2025-02-01 exceeds available lot quantity before or on the sell date"
        ));
    }

    #[test]
    fn realized_gains_handle_negative_sell_quantities() {
        let conn = setup_conn();
        conn.execute(
            "INSERT INTO accounts(id, name, type, currency) VALUES (1, 'Broker', 'broker', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets(id, ticker, name, currency) VALUES (1, 'NEG', 'Neg Lot', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2023-01-01', 1, 1, '100', '10', '0', 'buy')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2024-06-01', 1, 1, '-40', '15', '0', 'sell')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trades(date, asset_id, account_id, quantity, price, fees, side) VALUES ('2025-02-01', 1, 1, '-20', '20', '2', 'sell')",
            [],
        )
        .unwrap();

        let rows = realized_gains(&conn, "2025").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ticker, "NEG");
        assert_eq!(rows[0].sell_date, "2025-02-01");
        assert_eq!(rows[0].currency, "USD");
        let expected_gain = Decimal::from_str("198").unwrap();
        assert_eq!(rows[0].realized_gain, expected_gain);
    }
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

fn price_cmd(conn: &mut Connection, m: &clap::ArgMatches) -> Result<()> {
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

fn fetch_prices(conn: &mut Connection) -> Result<()> {
    let mut stmt = conn.prepare_cached("SELECT id, ticker FROM assets ORDER BY ticker")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;

    let mut assets = Vec::new();
    for row in rows {
        assets.push(row?);
    }

    drop(stmt);

    if assets.is_empty() {
        println!("No assets to fetch");
        return Ok(());
    }

    let symbols = assets
        .iter()
        .map(|(_, ticker)| ticker.as_str())
        .collect::<Vec<_>>();
    let url = format!(
        "https://query1.finance.yahoo.com/v7/finance/quote?symbols={}",
        symbols.join(",")
    );
    let client = http_client()?;
    let resp = client.get(url).send()?.error_for_status()?;
    let yr: YahooResponse = resp.json()?;

    let mut id_by_ticker: HashMap<&str, i64> = HashMap::with_capacity(assets.len());
    for (id, ticker) in &assets {
        id_by_ticker.insert(ticker.as_str(), *id);
    }

    let mut updates = Vec::with_capacity(yr.quoteResponse.result.len());
    for q in yr.quoteResponse.result {
        if let (Some(sym), Some(px)) = (q.symbol, q.regular_market_price) {
            if let Some(&asset_id) = id_by_ticker.get(sym.as_str()) {
                if let Some(px_decimal) = Decimal::from_f64_retain(px) {
                    updates.push((asset_id, px_decimal.to_string()));
                }
            }
        }
    }

    let now = Utc::now().to_rfc3339();
    if updates.is_empty() {
        println!("No Yahoo prices updated at {}", now);
        return Ok(());
    }

    let total_updates = updates.len();

    let tx = conn.transaction()?;
    let mut insert = tx.prepare_cached(
        "INSERT INTO prices(asset_id, as_of, price, source) VALUES (?1, ?2, ?3, 'yahoo')",
    )?;
    for (asset_id, price) in updates {
        insert.execute(params![asset_id, &now, price])?;
    }
    drop(insert);
    tx.commit()?;

    println!("Fetched {} prices at {}", total_updates, now);
    Ok(())
}
