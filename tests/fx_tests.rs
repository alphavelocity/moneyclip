// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use chrono::NaiveDate;
use rusqlite::{Connection, params};
use rust_decimal::Decimal;

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(r#"
        CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE fx_rates(date TEXT NOT NULL, base TEXT NOT NULL, quote TEXT NOT NULL, rate TEXT NOT NULL, UNIQUE(date, base, quote));
    "#).unwrap();
    conn.execute(
        "INSERT INTO settings(key,value) VALUES('base_currency','USD')",
        [],
    )
    .unwrap();
    conn
}

#[test]
fn fx_triangulation_and_reciprocal() {
    let conn = setup();
    // USD->INR and USD->EUR available
    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES (?1,?2,?3,?4)",
        params!["2025-08-01", "USD", "INR", "83"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES (?1,?2,?3,?4)",
        params!["2025-08-01", "USD", "EUR", "0.90"],
    )
    .unwrap();

    // EUR 90 -> INR ? via USD hub
    let amt = Decimal::new(9000, 2);
    let res = moneyclip::utils::fx_convert(
        &conn,
        NaiveDate::from_ymd_opt(2025, 8, 15).unwrap(),
        amt,
        "EUR",
        "INR",
    )
    .unwrap();
    // 90 EUR -> USD = 90 / 0.90 = 100 USD; -> INR = 100 * 83 = 8300
    assert_eq!(format!("{:.2}", res.round_dp(2)), "8300.00");

    // Reciprocal: INR -> USD using only USD->INR
    let amt_inr = Decimal::new(16600, 2); // 166.00 INR
    let res2 = moneyclip::utils::fx_convert(
        &conn,
        NaiveDate::from_ymd_opt(2025, 8, 15).unwrap(),
        amt_inr,
        "INR",
        "USD",
    )
    .unwrap();
    // 166 / 83 = 2.0 USD
    assert_eq!(format!("{:.2}", res2.round_dp(2)), "2.00");
}
