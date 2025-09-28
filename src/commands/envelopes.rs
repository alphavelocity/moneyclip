// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::{
    fx_convert, get_base_currency, id_for_category, parse_decimal, parse_month, pretty_table,
};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use rust_decimal::Decimal;

pub fn handle(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("fund", sub)) => fund(conn, sub)?,
        Some(("move", sub)) => move_between(conn, sub)?,
        Some(("status", sub)) => status(conn, sub)?,
        _ => {}
    }
    Ok(())
}

fn fund(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let month = parse_month(sub.get_one::<String>("month").unwrap().trim())?;
    let cat = sub
        .get_one::<String>("category")
        .unwrap()
        .trim()
        .to_string();
    let amount = parse_decimal(sub.get_one::<String>("amount").unwrap().trim())?;
    let cat_id = id_for_category(conn, &cat)?;

    let existing: Option<String> = conn
        .query_row(
            "SELECT amount FROM budgets WHERE month=?1 AND category_id=?2",
            params![&month, cat_id],
            |r| r.get(0),
        )
        .optional()?;
    let new_amt = if let Some(s) = existing {
        let cur = s
            .parse::<Decimal>()
            .with_context(|| format!("Invalid budget amount '{}' for {}", s, month))?;
        (cur + amount).to_string()
    } else {
        amount.to_string()
    };
    conn.execute(
        "INSERT INTO budgets(month, category_id, amount) VALUES (?1,?2,?3)
         ON CONFLICT(month, category_id) DO UPDATE SET amount=excluded.amount",
        params![&month, cat_id, &new_amt],
    )?;
    println!("Funded {} {} for {}", amount, get_base_currency(conn)?, cat);
    Ok(())
}

fn move_between(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let month = parse_month(sub.get_one::<String>("month").unwrap().trim())?;
    let from = sub.get_one::<String>("from").unwrap().trim().to_string();
    let to = sub.get_one::<String>("to").unwrap().trim().to_string();
    let amount = parse_decimal(sub.get_one::<String>("amount").unwrap().trim())?;
    let from_id = id_for_category(conn, &from)?;
    let to_id = id_for_category(conn, &to)?;

    let get_amt = |id: i64| -> Result<Decimal> {
        let v: Option<String> = conn
            .query_row(
                "SELECT amount FROM budgets WHERE month=?1 AND category_id=?2",
                params![&month, id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(s) = v {
            Ok(s.parse::<Decimal>()
                .with_context(|| format!("Invalid budget amount '{}' for {}", s, month))?)
        } else {
            Ok(Decimal::ZERO)
        }
    };
    let from_amt = get_amt(from_id)?;
    let to_amt = get_amt(to_id)?;

    let new_from = (from_amt - amount).to_string();
    let new_to = (to_amt + amount).to_string();

    conn.execute(
        "INSERT INTO budgets(month, category_id, amount) VALUES (?1,?2,?3)
         ON CONFLICT(month, category_id) DO UPDATE SET amount=excluded.amount",
        params![&month, from_id, &new_from],
    )?;
    conn.execute(
        "INSERT INTO budgets(month, category_id, amount) VALUES (?1,?2,?3)
         ON CONFLICT(month, category_id) DO UPDATE SET amount=excluded.amount",
        params![&month, to_id, &new_to],
    )?;
    println!(
        "Moved {} {} from {} to {}",
        amount,
        get_base_currency(conn)?,
        from,
        to
    );
    Ok(())
}

fn status(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let json_flag = sub.get_flag("json");
    let jsonl_flag = sub.get_flag("jsonl");
    let month = sub.get_one::<String>("month").unwrap().trim().to_string();
    let out_ccy = sub
        .get_one::<String>("currency")
        .map(|s| s.trim().to_uppercase());
    let mut stmt_c = conn.prepare("SELECT id, name FROM categories ORDER BY name")?;
    let cats = stmt_c.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;

    let mut rows = Vec::new();
    for c in cats {
        let (cat_id, cat_name) = c?;
        let (carry, budget_m, spent_m) = envelope_compute(conn, cat_id, &month)?;
        let available = carry + budget_m - spent_m;
        let dt = crate::utils::month_end(&month)?;
        let base = crate::utils::get_base_currency(conn)?;
        let disp_c = |v: rust_decimal::Decimal| -> Result<String> {
            if let Some(ref c) = out_ccy {
                Ok(format!(
                    "{:.2}",
                    crate::utils::fx_convert(conn, dt, v, &base, c)?
                ))
            } else {
                Ok(format!("{:.2}", v))
            }
        };
        rows.push(vec![
            cat_name,
            disp_c(carry)?,
            disp_c(budget_m)?,
            disp_c(spent_m)?,
            disp_c(available)?,
        ]);
    }
    if !crate::utils::maybe_print_json(json_flag, jsonl_flag, &rows)? {
        println!(
            "{}",
            pretty_table(
                &["Category", "Carryover", "Budget", "Spent", "Available"],
                rows
            )
        );
    }
    Ok(())
}

pub fn envelope_compute(
    conn: &Connection,
    category_id: i64,
    month: &str,
) -> Result<(Decimal, Decimal, Decimal)> {
    let base = crate::utils::get_base_currency(conn)?;

    let mut carryover = {
        let mut stmt =
            conn.prepare_cached("SELECT amount FROM budgets WHERE category_id=?1 AND month<?2")?;
        let mut rows = stmt.query(params![category_id, month])?;
        let mut total = Decimal::ZERO;
        while let Some(row) = rows.next()? {
            let amount: String = row.get(0)?;
            let value = amount
                .parse::<Decimal>()
                .with_context(|| format!("Invalid budget amount '{}' before {}", amount, month))?;
            total += value;
        }
        total
    };

    let mut stmt_t = conn.prepare("SELECT date, amount, currency FROM transactions WHERE category_id=?1 AND amount<0 AND substr(date,1,7)<?2")?;
    let mut cur = stmt_t.query(params![category_id, month])?;
    while let Some(r) = cur.next()? {
        let d: String = r.get(0)?;
        let a_s: String = r.get(1)?;
        let ccy: String = r.get(2)?;
        let date = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")?;
        let amt_abs = a_s
            .parse::<Decimal>()
            .with_context(|| format!("Invalid amount '{}' in transactions", a_s))?
            .abs();
        let conv = fx_convert(conn, date, amt_abs, &ccy, &base)?;
        carryover -= conv;
    }

    let budget_m_s: Option<String> = conn
        .query_row(
            "SELECT amount FROM budgets WHERE category_id=?1 AND month=?2",
            params![category_id, month],
            |r| r.get(0),
        )
        .optional()?;
    let budget_m = match budget_m_s {
        Some(s) => s
            .parse::<Decimal>()
            .with_context(|| format!("Invalid budget amount '{}' for {}", s, month))?,
        None => Decimal::ZERO,
    };

    let mut stmt_ms = conn.prepare("SELECT date, amount, currency FROM transactions WHERE category_id=?1 AND amount<0 AND substr(date,1,7)=?2")?;
    let mut cur2 = stmt_ms.query(params![category_id, month])?;
    let mut spent_m = Decimal::ZERO;
    while let Some(r) = cur2.next()? {
        let d: String = r.get(0)?;
        let a_s: String = r.get(1)?;
        let ccy: String = r.get(2)?;
        let date = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")?;
        let amt_abs = a_s
            .parse::<Decimal>()
            .with_context(|| format!("Invalid amount '{}' in transactions", a_s))?
            .abs();
        let conv = fx_convert(conn, date, amt_abs, &ccy, &base)?;
        spent_m += conv;
    }

    Ok((carryover, budget_m, spent_m))
}
