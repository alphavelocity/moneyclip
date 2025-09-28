// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::{id_for_category, invalidate_rule_cache, pretty_table};
use anyhow::{Result, anyhow};
use regex::Regex;
use rusqlite::{Connection, params};

pub fn handle(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("add", sub)) => {
            let pattern_raw = sub.get_one::<String>("pattern").unwrap();
            let pattern = pattern_raw.trim();
            Regex::new(pattern)
                .map_err(|err| anyhow!("Invalid regex pattern '{}': {}", pattern, err))?;

            let cat = sub
                .get_one::<String>("category")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let rewrite = sub
                .get_one::<String>("payee_rewrite")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let cat_id = if let Some(ref c) = cat {
                Some(id_for_category(conn, c)?)
            } else {
                None
            };
            conn.execute(
                "INSERT INTO rules(pattern, category_id, payee_rewrite) VALUES (?1,?2,?3)",
                params![pattern, cat_id, rewrite],
            )?;
            invalidate_rule_cache(conn);
            println!(
                "Added rule: /{}/ -> category {:?}, rewrite {:?}",
                pattern, cat_id, rewrite
            );
        }
        Some(("list", _)) => {
            let mut stmt = conn.prepare("SELECT id, pattern, COALESCE((SELECT name FROM categories WHERE id=category_id),'') as category, COALESCE(payee_rewrite,'') FROM rules ORDER BY id DESC")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            let mut data = Vec::new();
            for row in rows {
                let (id, pat, cat, rew) = row?;
                data.push(vec![id.to_string(), pat, cat, rew]);
            }
            println!(
                "{}",
                pretty_table(&["ID", "Pattern", "Category", "Payee Rewrite"], data)
            );
        }
        Some(("rm", sub)) => {
            let raw = sub.get_one::<String>("id").unwrap();
            let id = raw.trim().parse::<i64>()?;
            conn.execute("DELETE FROM rules WHERE id=?1", params![id])?;
            invalidate_rule_cache(conn);
            println!("Removed rule {}", id);
        }
        _ => {}
    }
    Ok(())
}
