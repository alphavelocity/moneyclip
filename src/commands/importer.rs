// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::{apply_import_rules, id_for_category, parse_date, parse_decimal};
use anyhow::{Context, Result, anyhow};
use csv::ReaderBuilder;
use rusqlite::{Connection, params};
use std::collections::{HashMap, hash_map::Entry};

pub fn handle(conn: &mut Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("transactions", sub)) => import_transactions(conn, sub),
        _ => Ok(()),
    }
}

fn import_transactions(conn: &mut Connection, sub: &clap::ArgMatches) -> Result<()> {
    let path = sub.get_one::<String>("path").unwrap().trim();
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("Open CSV {}", path))?;

    let tx = conn.transaction()?;
    let mut account_cache: HashMap<String, (i64, String)> = HashMap::new();
    let mut category_cache: HashMap<String, i64> = HashMap::new();

    for result in rdr.records() {
        let rec = result?;
        let date_raw = rec.get(0).context("date missing")?.trim().to_string();
        let mut payee = rec.get(1).context("payee missing")?.trim().to_string();
        let amount_raw = rec.get(2).context("amount missing")?.trim().to_string();
        let category = rec.get(3).unwrap_or("").trim().to_string();
        let account = rec.get(4).context("account missing")?.trim().to_string();
        let csv_currency = rec.get(5).unwrap_or("").trim();
        let note = rec
            .get(6)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let date = parse_date(&date_raw)
            .with_context(|| format!("Invalid transaction date '{}'", date_raw))?;
        let amount = parse_decimal(&amount_raw)
            .with_context(|| format!("Invalid amount '{}' for {}", amount_raw, payee))?;

        let acct_id: i64;
        let account_currency: &str;
        match account_cache.entry(account.clone()) {
            Entry::Occupied(entry) => {
                let (cached_id, cached_ccy) = entry.into_mut();
                acct_id = *cached_id;
                account_currency = cached_ccy.as_str();
            }
            Entry::Vacant(entry) => {
                let (id, ccy): (i64, String) = tx
                    .query_row(
                        "SELECT id, currency FROM accounts WHERE name=?1",
                        params![&account],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .with_context(|| format!("Account '{}' not found", account))?;
                let (cached_id, cached_ccy) = entry.insert((id, ccy));
                acct_id = *cached_id;
                account_currency = cached_ccy.as_str();
            }
        }
        let mut cat_id = if category.is_empty() {
            None
        } else {
            let cat_id = match category_cache.entry(category.clone()) {
                Entry::Occupied(entry) => *entry.get(),
                Entry::Vacant(entry) => {
                    let fetched = id_for_category(&tx, &category)?;
                    *entry.insert(fetched)
                }
            };
            Some(cat_id)
        };

        let (rule_cat, rewrite) = apply_import_rules(&tx, &payee, note.as_deref())?;
        if cat_id.is_none() {
            cat_id = rule_cat;
        }
        if let Some(newp) = rewrite.filter(|newp| newp != &payee) {
            payee = newp;
        }
        if !csv_currency.is_empty() && !csv_currency.eq_ignore_ascii_case(account_currency) {
            return Err(anyhow!(
                "Currency '{}' does not match account '{}' currency '{}'",
                csv_currency,
                account,
                account_currency
            ));
        }

        tx.execute(
            "INSERT INTO transactions(date, account_id, amount, payee, category_id, currency, note) \
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                date.to_string(),
                acct_id,
                amount.to_string(),
                payee,
                cat_id,
                account_currency,
                note.as_deref()
            ],
        )?;
    }
    tx.commit()?;
    println!("Imported transactions from {}", path);
    Ok(())
}
