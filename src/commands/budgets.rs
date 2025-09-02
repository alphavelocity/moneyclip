// Copyright (c) AlphaVelocity.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::{id_for_category, parse_decimal, parse_month, pretty_table};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

pub fn handle(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("set", sub)) => set(conn, sub)?,
        Some(("list", sub)) => list(conn, sub)?,
        Some(("report", sub)) => report(conn, sub)?,
        _ => {}
    }
    Ok(())
}

fn set(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let month = parse_month(sub.get_one::<String>("month").unwrap())?;
    let cat = sub.get_one::<String>("category").unwrap();
    let amount = parse_decimal(sub.get_one::<String>("amount").unwrap())?;
    let cat_id = id_for_category(conn, cat)?;
    conn.execute(
        "INSERT INTO budgets(month, category_id, amount) VALUES (?1,?2,?3)
         ON CONFLICT(month, category_id) DO UPDATE SET amount=excluded.amount",
        params![month, cat_id, amount.to_string()],
    )?;
    println!("Budget set for {} / {} = {}", month, cat, amount);
    Ok(())
}

fn list(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let mut sql = String::from(
        "SELECT b.month, c.name, b.amount FROM budgets b JOIN categories c ON b.category_id=c.id",
    );
    if let Some(month) = sub.get_one::<String>("month") {
        sql.push_str(" WHERE b.month=?1 ORDER BY c.name");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![month], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut data = Vec::new();
        for row in rows {
            let (m, c, a) = row?;
            data.push(vec![m, c, a]);
        }
        println!(
            "{}",
            pretty_table(&["Month", "Category", "Budget (BASE)"], data)
        );
    } else {
        sql.push_str(" ORDER BY b.month DESC, c.name");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut data = Vec::new();
        for row in rows {
            let (m, c, a) = row?;
            data.push(vec![m, c, a]);
        }
        println!(
            "{}",
            pretty_table(&["Month", "Category", "Budget (BASE)"], data)
        );
    }
    Ok(())
}

fn report(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let json_flag = sub.get_flag("json");
    let jsonl_flag = sub.get_flag("jsonl");
    let month = sub.get_one::<String>("month").unwrap();
    let out_ccy = sub.get_one::<String>("currency").map(|s| s.to_uppercase());

    // For each category compute base spent and compare with budget
    let mut cats_stmt = conn.prepare("SELECT id, name FROM categories ORDER BY name")?;
    let cats = cats_stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;

    let mut data = Vec::new();
    for c in cats {
        let (cid, cname) = c?;
        let budget_s: Option<String> = conn
            .query_row(
                "SELECT amount FROM budgets WHERE category_id=?1 AND month=?2",
                params![cid, month],
                |r| r.get(0),
            )
            .optional()?;
        let budget = budget_s.unwrap_or("0".into());

        let mut tstmt = conn.prepare("SELECT date, -amount, currency FROM transactions WHERE category_id=?1 AND amount<0 AND substr(date,1,7)=?2")?;
        let mut trs = tstmt.query(params![cid, month])?;
        let mut spent_base = rust_decimal::Decimal::ZERO;
        let base = crate::utils::get_base_currency(conn)?;
        while let Some(r) = trs.next()? {
            let d: String = r.get(0)?;
            let amt_s: String = r.get(1)?;
            let ccy: String = r.get(2)?;
            let date = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")?;
            let amt = amt_s
                .parse::<rust_decimal::Decimal>()
                .with_context(|| format!("Invalid amount '{}' in transactions", amt_s))?;
            let conv = crate::utils::fx_convert(conn, date, amt, &ccy, &base)?;
            spent_base += conv;
        }
        let disp = if let Some(ref ccy) = out_ccy {
            let dt = crate::utils::month_end(month)?;
            let v = crate::utils::fx_convert(
                conn,
                dt,
                spent_base,
                &crate::utils::get_base_currency(conn)?,
                ccy,
            )?;
            format!("{:.2}", v)
        } else {
            format!("{:.2}", spent_base)
        };
        data.push(vec![cname, budget, disp]);
    }

    if !crate::utils::maybe_print_json(json_flag, jsonl_flag, &data)? {
        println!(
            "{}",
            pretty_table(&["Category", "Budget (BASE)", "Spent (BASE)"], data)
        );
    }
    Ok(())
}
