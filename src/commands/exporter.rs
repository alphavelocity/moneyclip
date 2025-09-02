// Copyright (c) AlphaVelocity.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

pub fn handle(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("transactions", sub)) => export_transactions(conn, sub),
        _ => Ok(()),
    }
}

fn export_transactions(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let fmt = sub.get_one::<String>("format").unwrap().to_lowercase();
    let out = sub.get_one::<String>("out").unwrap();

    let mut stmt = conn.prepare(
        "SELECT t.date, a.name as account, t.payee, t.amount, t.currency, c.name as category, t.note
         FROM transactions t
         LEFT JOIN accounts a ON t.account_id=a.id
         LEFT JOIN categories c ON t.category_id=c.id
         ORDER BY t.date, t.id")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })?;

    match fmt.as_str() {
        "csv" => {
            let mut wtr = csv::Writer::from_path(out)?;
            wtr.write_record([
                "date", "account", "payee", "amount", "currency", "category", "note",
            ])?;
            for row in rows {
                let (d, a, p, amt, ccy, cat, note) = row?;
                wtr.write_record([
                    d,
                    a,
                    p,
                    amt,
                    ccy,
                    cat.unwrap_or_default(),
                    note.unwrap_or_default(),
                ])?;
            }
            wtr.flush()?;
        }
        "json" => {
            let mut items = Vec::new();
            for row in rows {
                let (d, a, p, amt, ccy, cat, note) = row?;
                items.push(json!({
                    "date": d, "account": a, "payee": p, "amount": amt, "currency": ccy, "category": cat, "note": note
                }));
            }
            std::fs::write(out, serde_json::to_string_pretty(&items)?)?;
        }
        _ => {
            eprintln!("Unknown format: {} (use csv|json)", fmt);
        }
    }
    println!("Exported transactions to {}", out);
    Ok(())
}
