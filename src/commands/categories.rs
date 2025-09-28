// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::pretty_table;
use anyhow::Result;
use rusqlite::{Connection, params};

pub fn handle(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("add", sub)) => {
            let name = sub.get_one::<String>("name").unwrap().trim().to_string();
            conn.execute("INSERT INTO categories(name) VALUES (?1)", params![name])?;
            println!("Added category '{}'", name);
        }
        Some(("list", _)) => {
            let mut stmt = conn.prepare("SELECT name FROM categories ORDER BY name")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let mut data = Vec::new();
            for row in rows {
                data.push(vec![row?]);
            }
            println!("{}", pretty_table(&["Category"], data));
        }
        Some(("rm", sub)) => {
            let name = sub.get_one::<String>("name").unwrap().trim().to_string();
            conn.execute("DELETE FROM categories WHERE name=?1", params![name])?;
            println!("Removed category '{}'", name);
        }
        _ => {}
    }
    Ok(())
}
