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
            let name = sub.get_one::<String>("name").unwrap();
            let typ = sub.get_one::<String>("type").unwrap();
            let ccy = sub.get_one::<String>("currency").unwrap().to_uppercase();
            conn.execute(
                "INSERT INTO accounts(name, type, currency) VALUES (?1, ?2, ?3)",
                params![name, typ, ccy],
            )?;
            println!("Added account '{}' ({}, {})", name, typ, ccy);
        }
        Some(("list", _)) => {
            let mut stmt = conn
                .prepare("SELECT name, type, currency, created_at FROM accounts ORDER BY name")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            let mut data = Vec::new();
            for row in rows {
                let (n, t, c, cr) = row?;
                data.push(vec![n, t, c, cr]);
            }
            println!(
                "{}",
                pretty_table(&["Name", "Type", "Currency", "Created"], data)
            );
        }
        Some(("rm", sub)) => {
            let name = sub.get_one::<String>("name").unwrap();
            conn.execute("DELETE FROM accounts WHERE name=?1", params![name])?;
            println!("Removed account '{}'", name);
        }
        _ => {}
    }
    Ok(())
}
