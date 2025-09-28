// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use moneyclip::{cli, commands::rules};
use rusqlite::{Connection, params};
use tempfile::NamedTempFile;

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(r#"
        CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        INSERT INTO settings(key,value) VALUES('base_currency','USD');
        CREATE TABLE categories(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE);
        CREATE TABLE rules(id INTEGER PRIMARY KEY AUTOINCREMENT, pattern TEXT NOT NULL, category_id INTEGER, payee_rewrite TEXT, note TEXT, created_at TEXT);
    "#).unwrap();
    conn.execute("INSERT INTO categories(name) VALUES('Shopping')", [])
        .unwrap();
    conn
}

#[test]
fn rule_applies_regex_and_rewrite() {
    let conn = setup();
    let cat_id: i64 = conn
        .query_row("SELECT id FROM categories WHERE name='Shopping'", [], |r| {
            r.get(0)
        })
        .unwrap();
    conn.execute("INSERT INTO rules(pattern, category_id, payee_rewrite, note, created_at) VALUES('(?i)amazon|amzn', ?1, 'Amazon', NULL, datetime('now'))", params![cat_id]).unwrap();

    let (c, r) =
        moneyclip::utils::apply_import_rules(&conn, "AMZN Mktp US*AB123", Some("order 123"))
            .unwrap();
    assert_eq!(c, Some(cat_id));
    assert_eq!(r, Some(String::from("Amazon")));
}

#[test]
fn rules_add_rejects_invalid_regex() {
    let conn = setup();
    let cli = cli::build_cli();
    let matches = cli.get_matches_from([
        "moneyclip",
        "rules",
        "add",
        "--pattern",
        " (?P< ",
        "--category",
        " Shopping ",
    ]);

    if let Some(("rules", rules_m)) = matches.subcommand() {
        let err = rules::handle(&conn, rules_m).unwrap_err();
        assert!(err.to_string().contains("Invalid regex pattern"));
    } else {
        panic!("rules command not parsed");
    }
}

#[test]
fn rules_rm_trims_id_argument() {
    let conn = setup();
    conn.execute(
        "INSERT INTO rules(pattern, category_id, payee_rewrite, note, created_at) VALUES('foo', NULL, NULL, NULL, datetime('now'))",
        [],
    )
    .unwrap();

    let cli = cli::build_cli();
    let matches = cli.get_matches_from(["moneyclip", "rules", "rm", "--id", " 1 "]);

    if let Some(("rules", rules_m)) = matches.subcommand() {
        rules::handle(&conn, rules_m).unwrap();
    } else {
        panic!("rules command not parsed");
    }

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM rules", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn rules_cache_refreshes_after_changes() {
    let conn = setup();

    conn.execute(
        "INSERT INTO rules(pattern, category_id, payee_rewrite, note, created_at) VALUES('foo', NULL, 'Foo', NULL, datetime('now'))",
        [],
    )
    .unwrap();
    moneyclip::utils::invalidate_rule_cache(&conn);

    let (cat, rewrite) = moneyclip::utils::apply_import_rules(&conn, "foo store", None).unwrap();
    assert_eq!(cat, None);
    assert_eq!(rewrite, Some(String::from("Foo")));

    conn.execute("DELETE FROM rules", []).unwrap();
    moneyclip::utils::invalidate_rule_cache(&conn);
    let (cat_after_delete, rewrite_after_delete) =
        moneyclip::utils::apply_import_rules(&conn, "foo store", None).unwrap();
    assert_eq!(cat_after_delete, None);
    assert_eq!(rewrite_after_delete, None);

    conn.execute(
        "INSERT INTO rules(pattern, category_id, payee_rewrite, note, created_at) VALUES('bar', NULL, 'Bar', NULL, datetime('now'))",
        [],
    )
    .unwrap();
    moneyclip::utils::invalidate_rule_cache(&conn);
    let (cat_new, rewrite_new) =
        moneyclip::utils::apply_import_rules(&conn, "bar shop", None).unwrap();
    assert_eq!(cat_new, None);
    assert_eq!(rewrite_new, Some(String::from("Bar")));
}

#[test]
fn rule_cache_invalidation_affects_other_connections() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path();

    let conn_a = Connection::open(path).unwrap();
    moneyclip::utils::invalidate_rule_cache(&conn_a);
    conn_a
        .execute_batch(
            r#"
        CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        INSERT INTO settings(key,value) VALUES('base_currency','USD');
        CREATE TABLE categories(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE);
        CREATE TABLE rules(id INTEGER PRIMARY KEY AUTOINCREMENT, pattern TEXT NOT NULL, category_id INTEGER, payee_rewrite TEXT,
 note TEXT, created_at TEXT);
    "#,
        )
        .unwrap();
    conn_a
        .execute("INSERT INTO categories(name) VALUES('Shopping')", [])
        .unwrap();
    conn_a
        .execute(
            "INSERT INTO rules(pattern, category_id, payee_rewrite, note, created_at) VALUES('(?i)amazon', NULL, 'Amazon', NULL, datetime('now'))",
            [],
        )
        .unwrap();

    let (_cat_initial, rewrite_initial) =
        moneyclip::utils::apply_import_rules(&conn_a, "AMAZON MARKETPLACE", None).unwrap();
    assert_eq!(rewrite_initial, Some(String::from("Amazon")));

    let conn_b = Connection::open(path).unwrap();
    let (_cat_b, rewrite_b) =
        moneyclip::utils::apply_import_rules(&conn_b, "AMAZON MARKETPLACE", None).unwrap();
    assert_eq!(rewrite_b, Some(String::from("Amazon")));

    conn_a.execute("DELETE FROM rules", []).unwrap();
    moneyclip::utils::invalidate_rule_cache(&conn_a);

    let (_cat_after, rewrite_after) =
        moneyclip::utils::apply_import_rules(&conn_b, "AMAZON MARKETPLACE", None).unwrap();
    assert_eq!(rewrite_after, None);
}

#[test]
fn rule_cache_detects_external_mutations() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path();

    let conn_a = Connection::open(path).unwrap();
    moneyclip::utils::invalidate_rule_cache(&conn_a);
    conn_a
        .execute_batch(
            r#"
        CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        INSERT INTO settings(key,value) VALUES('base_currency','USD');
        CREATE TABLE categories(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE);
        CREATE TABLE rules(id INTEGER PRIMARY KEY AUTOINCREMENT, pattern TEXT NOT NULL, category_id INTEGER, payee_rewrite TEXT, note TEXT, created_at TEXT);
    "#,
        )
        .unwrap();
    conn_a
        .execute("INSERT INTO categories(name) VALUES('Shopping')", [])
        .unwrap();
    conn_a
        .execute(
            "INSERT INTO rules(pattern, category_id, payee_rewrite, note, created_at) VALUES('(?i)amazon', NULL, 'Amazon', NULL, datetime('now'))",
            [],
        )
        .unwrap();

    let conn_b = Connection::open(path).unwrap();
    let (_cat_initial, rewrite_initial) =
        moneyclip::utils::apply_import_rules(&conn_b, "AMAZON MARKETPLACE", None).unwrap();
    assert_eq!(rewrite_initial, Some(String::from("Amazon")));

    conn_a
        .execute(
            "UPDATE rules SET payee_rewrite='Amazon Fresh' WHERE pattern='(?i)amazon'",
            [],
        )
        .unwrap();

    let (_cat_updated, rewrite_updated) =
        moneyclip::utils::apply_import_rules(&conn_b, "AMAZON MARKETPLACE", None).unwrap();
    assert_eq!(rewrite_updated, Some(String::from("Amazon Fresh")));
}
