// Copyright (c) AlphaVelocity.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use rusqlite::{Connection, params};

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
    conn.execute("INSERT INTO settings(key,value) VALUES('base_currency','USD')", []).unwrap();
    conn.execute("INSERT INTO categories(name) VALUES('Groceries')", []).unwrap();
    let cat_id: i64 = conn.query_row("SELECT id FROM categories WHERE name='Groceries'", [], |r| r.get(0)).unwrap();
    conn.execute("INSERT INTO budgets(month, category_id, amount) VALUES('2025-07', ?1, '100.00')", params![cat_id]).unwrap();
    // FX: USD->INR = 80 on 2025-07-01, 83 on 2025-08-01
    conn.execute("INSERT INTO fx_rates(date,base,quote,rate) VALUES ('2025-07-01','USD','INR','80')", []).unwrap();
    conn.execute("INSERT INTO fx_rates(date,base,quote,rate) VALUES ('2025-08-01','USD','INR','83')", []).unwrap();
    conn
}

#[test]
fn envelope_carry_budget_spent() {
    let conn = setup();
    // August spending in INR (foreign): 400 INR on 2025-08-10
    let cat_id: i64 = conn.query_row("SELECT id FROM categories WHERE name='Groceries'", [], |r| r.get(0)).unwrap();
        conn.execute("INSERT INTO transactions(date, amount, category_id, currency) VALUES('2025-08-10','-400',?1,'INR')", params![cat_id]).unwrap();

    // Compute: carryover = 100 USD budget in July; no July spend -> 100
    // August spent: 400 INR at 83 = 4.8193... USD
    // Available = 100 (carry) + 0 (Aug budget) - 4.82 ~= 95.18
    let (carry, budget_m, spent_m) = moneyclip::commands::envelopes::envelope_compute(&conn, cat_id, "2025-08").unwrap();
    assert_eq!(format!("{:.2}", carry.round_dp(2)), "100.00");
    assert_eq!(format!("{:.2}", budget_m.round_dp(2)), "0.00");
    assert_eq!(format!("{:.2}", spent_m.round_dp(2)), "4.82");
}
