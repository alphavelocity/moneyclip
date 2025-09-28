// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use moneyclip::{cli, commands::envelopes};
use rusqlite::{Connection, params};
use rust_decimal::Decimal;

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(r#"
        PRAGMA foreign_keys = ON;
        CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE categories(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE);
        CREATE TABLE budgets(id INTEGER PRIMARY KEY AUTOINCREMENT, month TEXT NOT NULL, category_id INTEGER NOT NULL, amount TEXT NOT NULL, UNIQUE(month, category_id));
        CREATE TABLE transactions(id INTEGER PRIMARY KEY AUTOINCREMENT, date TEXT NOT NULL, account_id INTEGER, amount TEXT NOT NULL, payee TEXT, category_id INTEGER, currency TEXT NOT NULL, note TEXT);
        CREATE TABLE fx_rates(date TEXT NOT NULL, base TEXT NOT NULL, quote TEXT NOT NULL, rate TEXT NOT NULL, UNIQUE(date, base, quote));
    "#).unwrap();
    conn.execute(
        "INSERT INTO settings(key,value) VALUES('base_currency','USD')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO categories(name) VALUES('Groceries')", [])
        .unwrap();
    let cat_id: i64 = conn
        .query_row(
            "SELECT id FROM categories WHERE name='Groceries'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "INSERT INTO budgets(month, category_id, amount) VALUES('2025-07', ?1, '100.00')",
        params![cat_id],
    )
    .unwrap();
    // FX: USD->INR = 80 on 2025-07-01, 83 on 2025-08-01
    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES ('2025-07-01','USD','INR','80')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES ('2025-08-01','USD','INR','83')",
        [],
    )
    .unwrap();
    conn
}

#[test]
fn envelope_carry_budget_spent() {
    let conn = setup();
    // August spending in INR (foreign): 400 INR on 2025-08-10
    let cat_id: i64 = conn
        .query_row(
            "SELECT id FROM categories WHERE name='Groceries'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute("INSERT INTO transactions(date, amount, category_id, currency) VALUES('2025-08-10','-400',?1,'INR')", params![cat_id]).unwrap();

    // Compute: carryover = 100 USD budget in July; no July spend -> 100
    // August spent: 400 INR at 83 = 4.8193... USD
    // Available = 100 (carry) + 0 (Aug budget) - 4.82 ~= 95.18
    let (carry, budget_m, spent_m) =
        moneyclip::commands::envelopes::envelope_compute(&conn, cat_id, "2025-08").unwrap();
    assert_eq!(format!("{:.2}", carry.round_dp(2)), "100.00");
    assert_eq!(format!("{:.2}", budget_m.round_dp(2)), "0.00");
    assert_eq!(format!("{:.2}", spent_m.round_dp(2)), "4.82");
}

#[test]
fn envelope_fund_trims_inputs() {
    let conn = setup();
    let cat_id: i64 = conn
        .query_row(
            "SELECT id FROM categories WHERE name='Groceries'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let cli = cli::build_cli();
    let matches = cli.get_matches_from([
        "moneyclip",
        "envelope",
        "fund",
        "--month",
        " 2025-07 ",
        "--category",
        " Groceries ",
        "--amount",
        " 25.00 ",
    ]);
    if let Some(("envelope", env_m)) = matches.subcommand() {
        envelopes::handle(&conn, env_m).unwrap();
    } else {
        panic!("envelope command not parsed");
    }

    let amount: String = conn
        .query_row(
            "SELECT amount FROM budgets WHERE month='2025-07' AND category_id=?1",
            params![cat_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount, "125.00");
}

#[test]
fn envelope_carryover_preserves_decimal_precision() {
    let conn = setup();
    let cat_id: i64 = conn
        .query_row(
            "SELECT id FROM categories WHERE name='Groceries'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    for month in ["2025-01", "2025-02", "2025-03"] {
        conn.execute(
            "INSERT INTO budgets(month, category_id, amount) VALUES(?1, ?2, '0.10')",
            params![month, cat_id],
        )
        .unwrap();
    }

    let (carryover, budget_m, spent_m) =
        envelopes::envelope_compute(&conn, cat_id, "2025-04").unwrap();

    assert_eq!(carryover, Decimal::from_str_exact("0.30").unwrap());
    assert!(budget_m.is_zero());
    assert!(spent_m.is_zero());
}
