// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use moneyclip::{cli, commands::exporter};
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

fn base_conn() -> Connection {
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
    conn
}

#[test]
fn export_transactions_streams_pretty_json() {
    let conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'Checking','bank','USD')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO categories(id,name) VALUES (1,'Groceries')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO transactions(date,account_id,amount,payee,category_id,currency,note) VALUES \
        ('2025-01-02',1,'-12.34','Corner Shop',1,'USD','Weekly run')",
        [],
    )
    .unwrap();

    let dir = tempdir().unwrap();
    let out_path = dir.path().join("export.json");
    let out_str = out_path.to_string_lossy().to_string();

    let cli = cli::build_cli();
    let matches = cli.get_matches_from([
        "moneyclip",
        "export",
        "transactions",
        "--format",
        "json",
        "--out",
        &out_str,
    ]);
    if let Some(("export", export_m)) = matches.subcommand() {
        exporter::handle(&conn, export_m).unwrap();
    } else {
        panic!("no export subcommand");
    }

    let contents = std::fs::read_to_string(&out_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(
        parsed,
        json!([
            {
                "date": "2025-01-02",
                "account": "Checking",
                "payee": "Corner Shop",
                "amount": "-12.34",
                "currency": "USD",
                "category": "Groceries",
                "note": "Weekly run"
            }
        ])
    );
}

#[test]
fn export_transactions_rejects_unknown_format() {
    let conn = base_conn();
    let dir = tempdir().unwrap();
    let out_path = dir.path().join("export.unknown");
    let out_str = out_path.to_string_lossy().to_string();

    let cli = cli::build_cli();
    let matches = cli.get_matches_from([
        "moneyclip",
        "export",
        "transactions",
        "--format",
        "xml",
        "--out",
        &out_str,
    ]);
    if let Some(("export", export_m)) = matches.subcommand() {
        assert!(exporter::handle(&conn, export_m).is_err());
    } else {
        panic!("no export subcommand");
    }
    assert!(!out_path.exists());
}
