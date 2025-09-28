// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use anyhow::{Result, bail};
use rusqlite::Connection;
use serde::Serialize;
use serde::ser::{SerializeSeq, Serializer};
use serde_json::ser::PrettyFormatter;
use std::fs::File;
use std::io::{BufWriter, Write};

pub fn handle(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("transactions", sub)) => export_transactions(conn, sub),
        _ => Ok(()),
    }
}

fn export_transactions(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let fmt = sub
        .get_one::<String>("format")
        .unwrap()
        .trim()
        .to_lowercase();
    let out = sub.get_one::<String>("out").unwrap().trim().to_string();

    let mut stmt = conn.prepare_cached(concat!(
        "SELECT t.date, a.name as account, t.payee, t.amount, t.currency, c.name as category, t.note\n",
        " FROM transactions t\n",
        " LEFT JOIN accounts a ON t.account_id=a.id\n",
        " LEFT JOIN categories c ON t.category_id=c.id\n",
        " ORDER BY t.date, t.id",
    ))?;
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
            let mut wtr = csv::Writer::from_path(&out)?;
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
            let file = File::create(&out)?;
            let mut writer = BufWriter::new(file);
            let formatter = PrettyFormatter::with_indent(b"  ");
            let mut serializer = serde_json::Serializer::with_formatter(&mut writer, formatter);
            let mut seq = serializer.serialize_seq(None)?;
            for row in rows {
                let (date, account, payee, amount, currency, category, note) = row?;
                seq.serialize_element(&ExportedTransaction {
                    date,
                    account,
                    payee,
                    amount,
                    currency,
                    category,
                    note,
                })?;
            }
            seq.end()?;
            writer.flush()?;
        }
        other => bail!("Unknown format: {} (use csv|json)", other),
    }
    println!("Exported transactions to {}", out);
    Ok(())
}

#[derive(Serialize)]
struct ExportedTransaction {
    date: String,
    account: String,
    payee: String,
    amount: String,
    currency: String,
    category: Option<String>,
    note: Option<String>,
}
