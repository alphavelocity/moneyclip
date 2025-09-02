// Copyright (c) AlphaVelocity.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use rusqlite::{params, Connection};

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
    conn.execute("INSERT INTO categories(name) VALUES('Dining')", [])
        .unwrap();
    let cat_id: i64 = conn
        .query_row("SELECT id FROM categories WHERE name='Dining'", [], |r| {
            r.get(0)
        })
        .unwrap();
    conn.execute(
        "INSERT INTO budgets(month, category_id, amount) VALUES('2025-08', ?1, '50.00')",
        params![cat_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO fx_rates(date,base,quote,rate) VALUES ('2025-08-01','USD','EUR','0.90')",
        [],
    )
    .unwrap();
    conn
}

#[test]
fn budget_spent_in_base_from_foreign() {
    let conn = setup();
    let cat_id: i64 = conn
        .query_row("SELECT id FROM categories WHERE name='Dining'", [], |r| {
            r.get(0)
        })
        .unwrap();
    // Spend 9 EUR on 2025-08-10 => 9 / 0.90 = 10 USD
    conn.execute("INSERT INTO transactions(date, amount, category_id, currency) VALUES('2025-08-10','-9',?1,'EUR')", params![cat_id]).unwrap();

    // Query via SQL to sum: expect 10 base spent
    let mut tstmt = conn.prepare("SELECT date, amount, currency FROM transactions WHERE category_id=?1 AND amount<0 AND substr(date,1,7)=?2").unwrap();
    let mut rows = tstmt.query(params![cat_id, "2025-08"]).unwrap();
    let mut total = rust_decimal::Decimal::ZERO;
    while let Some(r) = rows.next().unwrap() {
        let d: String = r.get(0).unwrap();
        let a_s: String = r.get(1).unwrap();
        let ccy: String = r.get(2).unwrap();
        let date = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").unwrap();
        let amt = a_s.parse::<rust_decimal::Decimal>().unwrap();
        // amounts are stored as negative for expenses; use positive magnitude for spend
        let amt = -amt;
        let base = moneyclip::utils::get_base_currency(&conn).unwrap();
        let conv = moneyclip::utils::fx_convert(&conn, date, amt, &ccy, &base).unwrap();
        total += conv;
    }
    // Format with two decimal places to ensure trailing zeros
    assert_eq!(format!("{:.2}", total), "10.00");
}
