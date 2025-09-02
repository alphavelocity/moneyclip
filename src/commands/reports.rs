// Copyright (c) AlphaVelocity.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::pretty_table;
use anyhow::{Context, Result};
use rusqlite::Connection;

pub fn handle(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("balances", sub)) => balances(conn, sub)?,
        Some(("cashflow", sub)) => cashflow(conn, sub)?,
        Some(("spend-by-category", sub)) => spend_by_category(conn, sub)?,
        _ => {}
    }
    Ok(())
}

fn balances(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let json_flag = sub.get_flag("json");
    let jsonl_flag = sub.get_flag("jsonl");
    let show_base = sub.get_flag("base");
    let out_ccy = sub.get_one::<String>("currency").map(|s| s.to_uppercase());
    let mut stmt = conn.prepare(
        "SELECT a.name, a.currency, IFNULL(SUM(t.amount),0) AS bal
         FROM accounts a
         LEFT JOIN transactions t ON t.account_id=a.id
         GROUP BY a.id ORDER BY a.name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f64>(2)?,
        ))
    })?;
    let mut data = Vec::new();
    if show_base || out_ccy.is_some() {
        let base = crate::utils::get_base_currency(conn)?;
        for row in rows {
            let (name, ccy, bal_f) = row?;
            let today = chrono::Utc::now().date_naive();
            let bal_dec = rust_decimal::Decimal::try_from(bal_f)
                .with_context(|| format!("Invalid balance '{}' for account {}", bal_f, name))?;
            let target = out_ccy.clone().unwrap_or(base.clone());
            let bal_base = crate::utils::fx_convert(conn, today, bal_dec, &ccy, &target)?;
            data.push(vec![
                format!("{} (in {})", name, target),
                target.clone(),
                format!("{:.2}", bal_base),
            ]);
        }
    } else {
        for row in rows {
            let (name, ccy, bal_f) = row?;
            data.push(vec![name, ccy, format!("{:.2}", bal_f)]);
        }
    }
    if !crate::utils::maybe_print_json(json_flag, jsonl_flag, &data)? {
        println!("{}", pretty_table(&["Account", "CCY", "Balance"], data));
    }
    Ok(())
}

fn cashflow(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let json_flag = sub.get_flag("json");
    let jsonl_flag = sub.get_flag("jsonl");
    let show_base = sub.get_flag("base");
    let months: usize = *sub.get_one::<usize>("months").unwrap_or(&12);
    let out_ccy = sub.get_one::<String>("currency").map(|s| s.to_uppercase());
    let mut stmt = conn.prepare(
        "SELECT substr(date,1,7) AS month, date, amount, currency
         FROM transactions
         ORDER BY date DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f64>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;

    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, (rust_decimal::Decimal, rust_decimal::Decimal)> = BTreeMap::new();
    let base = crate::utils::get_base_currency(conn)?;

    for row in rows {
        let (m, d, amt_f, ccy) = row?;
        let date = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")?;
        let amt = rust_decimal::Decimal::try_from(amt_f)
            .with_context(|| format!("Invalid amount '{}' on {}", amt_f, d))?;
        let amt_base = if show_base || out_ccy.is_some() {
            crate::utils::fx_convert(conn, date, amt, &ccy, &base)?
        } else {
            amt
        };
        let entry = map
            .entry(m)
            .or_insert((rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO));
        if amt_base > rust_decimal::Decimal::ZERO {
            entry.0 += amt_base;
        } else {
            entry.1 += -amt_base;
        }
    }
    let mut data = Vec::new();
    for (m, (inc, exp)) in map.iter().rev().take(months) {
        data.push(vec![
            m.clone(),
            format!("{:.2}", inc),
            format!("{:.2}", exp),
        ]);
    }
    if !crate::utils::maybe_print_json(json_flag, jsonl_flag, &data)? {
        println!("{}", pretty_table(&["Month", "Income", "Expense"], data));
    }
    Ok(())
}

fn spend_by_category(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let json_flag = sub.get_flag("json");
    let jsonl_flag = sub.get_flag("jsonl");
    let show_base = sub.get_flag("base");
    let month = sub.get_one::<String>("month").unwrap();
    let out_ccy = sub.get_one::<String>("currency").map(|s| s.to_uppercase());
    if show_base || out_ccy.is_some() {
        let base = crate::utils::get_base_currency(conn)?;
        let mut stmt = conn.prepare("SELECT c.name, t.date, -t.amount as out, t.currency FROM transactions t LEFT JOIN categories c ON t.category_id=c.id WHERE substr(t.date,1,7)=?1 AND t.amount < 0")?;
        let rows = stmt.query_map([month], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        use std::collections::HashMap;
        let mut agg: HashMap<String, rust_decimal::Decimal> = HashMap::new();
        for row in rows {
            let (cat_opt, d, out_f, ccy) = row?;
            let cat = cat_opt.unwrap_or("(uncategorized)".into());
            let date = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")?;
            let out_dec = rust_decimal::Decimal::try_from(out_f)
                .with_context(|| format!("Invalid amount '{}' for {}", out_f, cat))?;
            let target = out_ccy.clone().unwrap_or(base.clone());
            let out_base = crate::utils::fx_convert(conn, date, out_dec, &ccy, &target)?;
            *agg.entry(cat).or_insert(rust_decimal::Decimal::ZERO) += out_base;
        }
        let mut data = Vec::new();
        let mut items: Vec<_> = agg.into_iter().collect();
        items.sort_by(|a, b| b.1.cmp(&a.1));
        for (cat, amt) in items {
            data.push(vec![cat, format!("{:.2}", amt)]);
        }
        let hdr = if let Some(ref t) = out_ccy {
            format!("Spent ({})", t)
        } else {
            "Spent (BASE)".to_string()
        };
        if !crate::utils::maybe_print_json(json_flag, jsonl_flag, &data)? {
            println!("{}", pretty_table(&["Category", &hdr], data));
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT c.name, printf('%.2f', -SUM(t.amount)) AS spent
             FROM transactions t LEFT JOIN categories c ON t.category_id=c.id
             WHERE substr(t.date,1,7)=?1 AND t.amount < 0
             GROUP BY c.name ORDER BY spent DESC",
        )?;
        let rows = stmt.query_map([month], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut data = Vec::new();
        for row in rows {
            let (cat, spent) = row?;
            data.push(vec![cat.unwrap_or("(uncategorized)".into()), spent]);
        }
        println!("{}", pretty_table(&["Category", "Spent"], data));
    }
    Ok(())
}
