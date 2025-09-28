// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use moneyclip::{cli, commands::transactions};
use rusqlite::{Connection, params};

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
        CREATE TABLE rules(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            pattern TEXT NOT NULL,
            category_id INTEGER,
            payee_rewrite TEXT,
            note TEXT,
            created_at TEXT
        );
        "#,
    )
    .unwrap();
    conn
}

fn setup_with_seed_transactions() -> Connection {
    let conn = base_conn();
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
    let conn = setup_with_seed_transactions();
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

#[test]
fn list_filters_trim_inputs() {
    let conn = setup_with_seed_transactions();
    let cli = cli::build_cli();
    let matches = cli.get_matches_from([
        "moneyclip",
        "tx",
        "list",
        "--limit",
        "1",
        "--month",
        " 2025-01 ",
        "--account",
        " A1 ",
        "--category",
        " Cat1 ",
    ]);
    if let Some(("tx", tx_m)) = matches.subcommand() {
        if let Some(("list", list_m)) = tx_m.subcommand() {
            let rows = transactions::query_rows(&conn, list_m).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].account, "A1");
            assert_eq!(rows[0].category, "Cat1");
            assert!(rows[0].date.starts_with("2025-01"));
        } else {
            panic!("no list subcommand");
        }
    } else {
        panic!("no tx subcommand");
    }
}

#[test]
fn manual_add_applies_rewrite_even_with_manual_category() {
    let conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO categories(id,name) VALUES (1,'ManualCat')", [])
        .unwrap();
    conn.execute("INSERT INTO categories(id,name) VALUES (2,'RuleCat')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO rules(pattern, category_id, payee_rewrite, note, created_at) VALUES('(?i)original', 2, 'Updated Store', NULL, datetime('now'))",
        [],
    )
    .unwrap();

    let cli = cli::build_cli();
    let matches = cli.get_matches_from([
        "moneyclip",
        "tx",
        "add",
        "--date",
        " 2025-02-01 ",
        "--account",
        " A1 ",
        "--amount",
        " -12.34 ",
        "--payee",
        "  Original Shop  ",
        "--category",
        " ManualCat ",
        "--note",
        "  Some memo  ",
    ]);
    if let Some(("tx", tx_m)) = matches.subcommand() {
        transactions::handle(&conn, tx_m).unwrap();
    } else {
        panic!("no tx subcommand");
    }

    let (payee, category_id, note): (String, Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT payee, category_id, note FROM transactions ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(payee, "Updated Store");
    assert_eq!(category_id, Some(1));
    assert_eq!(note.unwrap(), "Some memo");
}

#[test]
fn manual_add_rules_match_note_when_no_category() {
    let conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO categories(id,name) VALUES (1,'FallbackCat')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO categories(id,name) VALUES (2,'RuleCat')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO rules(pattern, category_id, payee_rewrite, note, created_at) VALUES('(?i)receipt needle', 2, 'Note Match', NULL, datetime('now'))",
        [],
    )
    .unwrap();

    let cli = cli::build_cli();
    let matches = cli.get_matches_from([
        "moneyclip",
        "tx",
        "add",
        "--date",
        "2025-02-02",
        "--account",
        "A1",
        "--amount=-8.75",
        "--payee",
        "Gas Station",
        "--note",
        "Receipt needle was handwritten",
    ]);
    if let Some(("tx", tx_m)) = matches.subcommand() {
        transactions::handle(&conn, tx_m).unwrap();
    } else {
        panic!("no tx subcommand");
    }

    let (payee, category_id): (String, Option<i64>) = conn
        .query_row(
            "SELECT payee, category_id FROM transactions ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(payee, "Note Match");
    assert_eq!(category_id, Some(2));
}

#[test]
fn manual_add_errors_on_invalid_rule_regex() {
    let conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO rules(pattern, category_id, payee_rewrite, note, created_at) VALUES('(*invalid', NULL, NULL, NULL, datetime('now'))",
        [],
    )
    .unwrap();

    let cli = cli::build_cli();
    let matches = cli.get_matches_from([
        "moneyclip",
        "tx",
        "add",
        "--date",
        "2025-03-01",
        "--account",
        "A1",
        "--amount=-1.00",
        "--payee",
        "Test",
    ]);
    if let Some(("tx", tx_m)) = matches.subcommand() {
        let err = transactions::handle(&conn, tx_m).unwrap_err();
        assert!(
            err.to_string()
                .contains("Invalid regex pattern '(*invalid'")
        );
    } else {
        panic!("no tx subcommand");
    }

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
