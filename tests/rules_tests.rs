// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use rusqlite::{params, Connection};

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
