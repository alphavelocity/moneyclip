// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use moneyclip::{cli, commands::importer};
use rusqlite::Connection;
use std::io::Write;
use tempfile::NamedTempFile;

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

#[test]
fn importer_trims_cli_path_argument() {
    let mut conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();

    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "date,payee,amount,category,account,currency,note\n2025-02-03,Shop,-5.00,,A1,USD,"
    )
    .unwrap();
    file.flush().unwrap();

    let path = file.path().to_str().unwrap().to_string();
    let padded = format!("  {}  ", path);
    let cli = cli::build_cli();
    let matches = cli.get_matches_from(["moneyclip", "import", "transactions", "--path", &padded]);
    if let Some(("import", import_m)) = matches.subcommand() {
        importer::handle(&mut conn, import_m).unwrap();
    } else {
        panic!("no import subcommand");
    }

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn importer_applies_rewrite_even_with_category() {
    let mut conn = base_conn();
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

    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "date,payee,amount,category,account,currency,note\n2025-02-03,Original Shop,-20.00,ManualCat,A1,USD,"
    )
    .unwrap();
    file.flush().unwrap();

    let path = file.path().to_str().unwrap().to_string();
    let cli = cli::build_cli();
    let matches = cli.get_matches_from(["moneyclip", "import", "transactions", "--path", &path]);
    if let Some(("import", import_m)) = matches.subcommand() {
        importer::handle(&mut conn, import_m).unwrap();
    } else {
        panic!("no import subcommand");
    }

    let (payee, category_id, amount): (String, Option<i64>, String) = conn
        .query_row(
            "SELECT payee, category_id, amount FROM transactions ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(payee, "Updated Store");
    assert_eq!(category_id, Some(1));
    assert_eq!(amount, "-20.00");
}

#[test]
fn importer_trims_fields_and_preserves_manual_category() {
    let mut conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO categories(id,name) VALUES (1,'ManualCat')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO rules(pattern, category_id, payee_rewrite, note, created_at) VALUES('(?i)original', NULL, 'Updated Store', NULL, datetime('now'))",
        [],
    )
    .unwrap();

    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "date,payee,amount,category,account,currency,note\n2025-02-03,  Original Shop  ,-20.00, ManualCat , A1 ,USD,  memo text  "
    )
    .unwrap();
    file.flush().unwrap();

    let path = file.path().to_str().unwrap().to_string();
    let cli = cli::build_cli();
    let matches = cli.get_matches_from(["moneyclip", "import", "transactions", "--path", &path]);
    if let Some(("import", import_m)) = matches.subcommand() {
        importer::handle(&mut conn, import_m).unwrap();
    } else {
        panic!("no import subcommand");
    }

    let (payee, category_id, amount, note): (String, Option<i64>, String, Option<String>) = conn
        .query_row(
            "SELECT payee, category_id, amount, note FROM transactions ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(payee, "Updated Store");
    assert_eq!(category_id, Some(1));
    assert_eq!(amount, "-20.00");
    assert_eq!(note.unwrap(), "memo text");
}

#[test]
fn importer_rejects_invalid_date() {
    let mut conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();

    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "date,payee,amount,category,account,currency,note\n2025-13-03,Shop,abc,,A1,USD,"
    )
    .unwrap();
    file.flush().unwrap();

    let path = file.path().to_str().unwrap().to_string();
    let cli = cli::build_cli();
    let matches = cli.get_matches_from(["moneyclip", "import", "transactions", "--path", &path]);
    if let Some(("import", import_m)) = matches.subcommand() {
        let err = importer::handle(&mut conn, import_m).unwrap_err();
        assert!(
            err.to_string()
                .contains("Invalid transaction date '2025-13-03'")
        );
    } else {
        panic!("no import subcommand");
    }

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn importer_errors_on_invalid_rule_regex() {
    let mut conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO rules(pattern, category_id, payee_rewrite, note, created_at) VALUES('(?P<', NULL, NULL, NULL, datetime('now'))",
        [],
    )
    .unwrap();

    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "date,payee,amount,category,account,currency,note\n2025-02-03,Shop,-5.00,,A1,USD,"
    )
    .unwrap();
    file.flush().unwrap();

    let path = file.path().to_str().unwrap().to_string();
    let cli = cli::build_cli();
    let matches = cli.get_matches_from(["moneyclip", "import", "transactions", "--path", &path]);
    if let Some(("import", import_m)) = matches.subcommand() {
        let err = importer::handle(&mut conn, import_m).unwrap_err();
        assert!(err.to_string().contains("Invalid regex pattern '(?P<'"));
    } else {
        panic!("no import subcommand");
    }

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn importer_rejects_invalid_amount() {
    let mut conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();

    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "date,payee,amount,category,account,currency,note\n2025-02-03,Shop,abc,,A1,USD,"
    )
    .unwrap();
    file.flush().unwrap();

    let path = file.path().to_str().unwrap().to_string();
    let cli = cli::build_cli();
    let matches = cli.get_matches_from(["moneyclip", "import", "transactions", "--path", &path]);
    if let Some(("import", import_m)) = matches.subcommand() {
        let err = importer::handle(&mut conn, import_m).unwrap_err();
        assert!(err.to_string().contains("Invalid amount 'abc' for Shop"));
    } else {
        panic!("no import subcommand");
    }

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn importer_rejects_currency_mismatch() {
    let mut conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();

    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "date,payee,amount,category,account,currency,note\n2025-02-03,Shop,-5.00,,A1,EUR,"
    )
    .unwrap();
    file.flush().unwrap();

    let path = file.path().to_str().unwrap().to_string();
    let cli = cli::build_cli();
    let matches = cli.get_matches_from(["moneyclip", "import", "transactions", "--path", &path]);
    if let Some(("import", import_m)) = matches.subcommand() {
        let err = importer::handle(&mut conn, import_m).unwrap_err();
        assert!(
            err.to_string()
                .contains("Currency 'EUR' does not match account 'A1' currency 'USD'")
        );
    } else {
        panic!("no import subcommand");
    }

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn importer_rolls_back_when_row_fails() {
    let mut conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();

    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "date,payee,amount,category,account,currency,note\n2025-02-03,Shop,-5.00,,A1,USD,\n2025-02-04,Other,-7.00,,A1,EUR,"
    )
    .unwrap();
    file.flush().unwrap();

    let path = file.path().to_str().unwrap().to_string();
    let cli = cli::build_cli();
    let matches = cli.get_matches_from(["moneyclip", "import", "transactions", "--path", &path]);
    if let Some(("import", import_m)) = matches.subcommand() {
        let err = importer::handle(&mut conn, import_m).unwrap_err();
        assert!(
            err.to_string()
                .contains("Currency 'EUR' does not match account 'A1' currency 'USD'")
        );
    } else {
        panic!("no import subcommand");
    }

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn importer_allows_case_insensitive_currency_match() {
    let mut conn = base_conn();
    conn.execute(
        "INSERT INTO accounts(id,name,type,currency) VALUES (1,'A1','bank','USD')",
        [],
    )
    .unwrap();

    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "date,payee,amount,category,account,currency,note\n2025-02-03,Shop,-5.00,,A1,usd,"
    )
    .unwrap();
    file.flush().unwrap();

    let path = file.path().to_str().unwrap().to_string();
    let cli = cli::build_cli();
    let matches = cli.get_matches_from(["moneyclip", "import", "transactions", "--path", &path]);
    if let Some(("import", import_m)) = matches.subcommand() {
        importer::handle(&mut conn, import_m).unwrap();
    } else {
        panic!("no import subcommand");
    }

    let (count, currency): (i64, String) = conn
        .query_row("SELECT COUNT(*), currency FROM transactions", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(currency, "USD");
}
