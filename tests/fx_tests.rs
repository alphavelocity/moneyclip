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

#[test]
fn fx_converts_from_base_via_intermediate() {
    let conn = setup();
    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES (?1,?2,?3,?4)",
        params!["2025-08-01", "USD", "EUR", "0.8"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES (?1,?2,?3,?4)",
        params!["2025-08-01", "EUR", "JPY", "130"],
    )
    .unwrap();

    let amount = Decimal::new(10000, 2); // 100.00 USD
    let converted = moneyclip::utils::fx_convert(
        &conn,
        NaiveDate::from_ymd_opt(2025, 8, 2).unwrap(),
        amount,
        "USD",
        "JPY",
    )
    .unwrap();

    // USD -> EUR -> JPY using latest rates
    assert_eq!(format!("{:.2}", converted), "10400.00");
}

#[test]
fn fx_conversion_errors_without_available_path() {
    let conn = setup();
    let date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
    let amount = Decimal::new(1000, 2);

    let err = moneyclip::utils::fx_convert(&conn, date, amount, "EUR", "JPY")
        .expect_err("missing rates should fail");
    assert!(
        err.to_string().contains("No FX rate path from EUR to JPY"),
        "unexpected error: {}",
        err
    );

    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES (?1,?2,?3,?4)",
        params!["2025-08-01", "USD", "EUR", "0"],
    )
    .unwrap();

    let err = moneyclip::utils::fx_convert(&conn, date, amount, "USD", "EUR")
        .expect_err("zero rate should fail");
    assert!(
        err.to_string().contains("is not positive"),
        "unexpected error for zero rate: {}",
        err
    );
}

#[test]
fn fx_chooses_best_available_path() {
    let conn = setup();
    let date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();

    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES (?1,?2,?3,?4)",
        params!["2025-08-01", "USD", "CAD", "2.0"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES (?1,?2,?3,?4)",
        params!["2025-08-01", "USD", "GBP", "0.5"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES (?1,?2,?3,?4)",
        params!["2025-08-01", "CAD", "GBP", "0.1"],
    )
    .unwrap();

    let amount = Decimal::new(1000, 2); // 10 CAD
    let converted = moneyclip::utils::fx_convert(&conn, date, amount, "CAD", "GBP").unwrap();

    // Direct CAD->GBP yields 1 GBP, but going via USD -> GBP yields 2.5 GBP.
    assert_eq!(format!("{:.2}", converted), "2.50");
}

#[test]
fn fx_cache_refreshes_after_rate_updates() {
    let conn = setup();
    let date = NaiveDate::from_ymd_opt(2025, 1, 2).unwrap();

    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES (?1,?2,?3,?4)",
        params!["2025-01-01", "USD", "EUR", "0.5"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES (?1,?2,?3,?4)",
        params!["2025-01-01", "USD", "JPY", "100"],
    )
    .unwrap();

    let amount = Decimal::ONE;
    let initial = moneyclip::utils::fx_convert(&conn, date, amount, "EUR", "JPY").unwrap();
    assert_eq!(format!("{:.2}", initial), "200.00");

    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES (?1,?2,?3,?4)",
        params!["2025-01-02", "USD", "JPY", "120"],
    )
    .unwrap();

    let refreshed = moneyclip::utils::fx_convert(&conn, date, amount, "EUR", "JPY").unwrap();
    assert_eq!(format!("{:.2}", refreshed), "240.00");
}
