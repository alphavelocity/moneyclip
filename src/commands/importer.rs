// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::{apply_import_rules, id_for_account, id_for_category};
use anyhow::{Context, Result};
use csv::ReaderBuilder;
use rusqlite::{params, Connection};

pub fn handle(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("transactions", sub)) => import_transactions(conn, sub),
        _ => Ok(()),
    }
}

fn import_transactions(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let path = sub.get_one::<String>("path").unwrap();
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("Open CSV {}", path))?;

    for result in rdr.records() {
        let rec = result?;
        let date = rec.get(0).context("date missing")?.to_string();
        let mut payee = rec.get(1).context("payee missing")?.to_string();
        let amount = rec.get(2).context("amount missing")?.to_string();
        let category = rec.get(3).unwrap_or("").to_string();
        let account = rec.get(4).context("account missing")?.to_string();
        let currency = rec.get(5).unwrap_or("").to_string();
        let note = rec.get(6).unwrap_or("").to_string();

        let acct_id = id_for_account(conn, &account)?;
        let mut cat_id = if category.is_empty() {
            None
        } else {
            Some(id_for_category(conn, &category)?)
        };
        if cat_id.is_none() {
            let (rule_cat, rewrite) = apply_import_rules(
                conn,
                &payee,
                if note.is_empty() { None } else { Some(&note) },
            )?;
            if cat_id.is_none() {
                cat_id = rule_cat;
            }
            if let Some(newp) = rewrite {
                payee = newp;
            }
        }
        let ccy: String = if currency.is_empty() {
            conn.query_row(
                "SELECT currency FROM accounts WHERE id=?1",
                params![acct_id],
                |r| r.get::<_, String>(0),
            )?
        } else {
            currency
        };

        conn.execute(
            "INSERT INTO transactions(date, account_id, amount, payee, category_id, currency, note)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                date,
                acct_id,
                amount,
                payee,
                cat_id,
                ccy,
                if note.is_empty() {
                    None::<String>
                } else {
                    Some(note)
                }
            ],
        )?;
    }
    println!("Imported transactions from {}", path);
    Ok(())
}
