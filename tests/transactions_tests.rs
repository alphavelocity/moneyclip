// Copyright (c) AlphaVelocity.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use moneyclip::{cli, commands::transactions};
use rusqlite::{params, Connection};

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE accounts(id INTEGER PRIMARY KEY, name TEXT, type TEXT, currency TEXT);
        CREATE TABLE categories(id INTEGER PRIMARY KEY, name TEXT);
        CREATE TABLE transactions(
            id INTEGER PRIMARY KEY,
            date TEXT NOT NULL,
            account_id INTEGER NOT NULL,
            amount TEXT NOT NULL,
            payee TEXT NOT NULL,
            category_id INTEGER,
            currency TEXT NOT NULL,
            note TEXT
        );
        "#,
    )
    .unwrap();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO categories(id,name) VALUES (1,'Cat1')", [])
        .unwrap();
    for i in 1..=3 {
        conn.execute(
            "INSERT INTO transactions(date,account_id,amount,payee,category_id,currency,note) VALUES (?1,1,'-10','P',1,'USD','')",
            params![format!("2025-01-0{}", i)],
        )
        .unwrap();
    }
    conn
}

#[test]
fn list_limit_respected() {
    let conn = setup();
    let cli = cli::build_cli();
    let matches = cli.get_matches_from(["moneyclip", "tx", "list", "--limit", "2"]);
    if let Some(("tx", tx_m)) = matches.subcommand() {
        if let Some(("list", list_m)) = tx_m.subcommand() {
            let rows = transactions::query_rows(&conn, list_m).unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].date, "2025-01-03");
        } else {
            panic!("no list subcommand");
        }
    } else {
        panic!("no tx subcommand");
    }
}
