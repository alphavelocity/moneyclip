// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::{id_for_category, parse_decimal, parse_month, pretty_table};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use rust_decimal::Decimal;

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
    let month = parse_month(sub.get_one::<String>("month").unwrap().trim())?;
    let cat = sub
        .get_one::<String>("category")
        .unwrap()
        .trim()
        .to_string();
    let amount = parse_decimal(sub.get_one::<String>("amount").unwrap().trim())?;
    let cat_id = id_for_category(conn, &cat)?;
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
    if let Some(month_raw) = sub.get_one::<String>("month") {
        let month = month_raw.trim();
        if !month.is_empty() {
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
            return Ok(());
        }
    }
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
    Ok(())
}

fn report(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let json_flag = sub.get_flag("json");
    let jsonl_flag = sub.get_flag("jsonl");
    let month = sub.get_one::<String>("month").unwrap().trim().to_string();
    let out_ccy = sub
        .get_one::<String>("currency")
        .map(|s| s.trim().to_uppercase());
    let base_ccy = crate::utils::get_base_currency(conn)?;

    let data = build_budget_report(conn, &month, &base_ccy, out_ccy.as_deref())?;
    let display_ccy = out_ccy.as_deref().unwrap_or(&base_ccy);

    if !crate::utils::maybe_print_json(json_flag, jsonl_flag, &data)? {
        let hdr_budget = format!("Budget ({})", display_ccy);
        let hdr_spent = format!("Spent ({})", display_ccy);
        println!(
            "{}",
            pretty_table(&["Category", &hdr_budget, &hdr_spent], data)
        );
    }
    Ok(())
}

fn build_budget_report(
    conn: &Connection,
    month: &str,
    base_ccy: &str,
    out_ccy: Option<&str>,
) -> Result<Vec<Vec<String>>> {
    let categories = {
        let mut stmt = conn.prepare_cached("SELECT id, name FROM categories ORDER BY name")?;
        let mut rows = stmt.query([])?;
        let mut cats = Vec::new();
        while let Some(row) = rows.next()? {
            cats.push((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
        }
        cats
    };

    let mut budget_stmt =
        conn.prepare_cached("SELECT amount FROM budgets WHERE category_id=?1 AND month=?2")?;
    let mut tx_stmt = conn.prepare_cached(
        "SELECT date, amount, currency FROM transactions WHERE category_id=?1 AND amount<0 AND substr(date,1,7)=?2",
    )?;

    let month_end = crate::utils::month_end(month)?;
    let mut data = Vec::with_capacity(categories.len());

    for (cid, cname) in categories {
        let budget_s: Option<String> = budget_stmt
            .query_row(params![cid, month], |r| r.get(0))
            .optional()?;
        let budget_dec = match budget_s {
            Some(ref s) => s
                .parse::<Decimal>()
                .with_context(|| format!("Invalid budget amount '{}' for {}", s, month))?,
            None => Decimal::ZERO,
        };

        let mut trs = tx_stmt.query(params![cid, month])?;
        let mut spent_base = Decimal::ZERO;
        while let Some(r) = trs.next()? {
            let d: String = r.get(0)?;
            let amt_s: String = r.get(1)?;
            let ccy: String = r.get(2)?;
            let date = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")?;
            let amt = amt_s
                .parse::<Decimal>()
                .with_context(|| format!("Invalid amount '{}' in transactions", amt_s))?;
            let conv = crate::utils::fx_convert(conn, date, amt.abs(), &ccy, base_ccy)?;
            spent_base += conv;
        }

        let spent_disp = if let Some(target) = out_ccy {
            let converted =
                crate::utils::fx_convert(conn, month_end, spent_base, base_ccy, target)?;
            format!("{:.2}", converted)
        } else {
            format!("{:.2}", spent_base)
        };

        let budget_disp = if let Some(target) = out_ccy {
            let converted =
                crate::utils::fx_convert(conn, month_end, budget_dec, base_ccy, target)?;
            format!("{:.2}", converted)
        } else {
            format!("{:.2}", budget_dec)
        };

        data.push(vec![cname, budget_disp, spent_disp]);
    }

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::build_budget_report;
    use rusqlite::{Connection, params};

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE categories(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE);
            CREATE TABLE budgets(id INTEGER PRIMARY KEY AUTOINCREMENT, month TEXT NOT NULL, category_id INTEGER NOT NULL, amount TEXT NOT NULL, UNIQUE(month, category_id));
            CREATE TABLE transactions(id INTEGER PRIMARY KEY AUTOINCREMENT, date TEXT NOT NULL, account_id INTEGER, amount TEXT NOT NULL, payee TEXT, category_id INTEGER, currency TEXT NOT NULL, note TEXT);
            CREATE TABLE fx_rates(id INTEGER PRIMARY KEY AUTOINCREMENT, date TEXT NOT NULL, base TEXT NOT NULL, quote TEXT NOT NULL, rate TEXT NOT NULL, UNIQUE(date, base, quote));
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO settings(key,value) VALUES('base_currency','USD')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO categories(name) VALUES('Dining')", [])
            .unwrap();
        let cat_id: i64 = conn
            .query_row("SELECT id FROM categories WHERE name='Dining'", [], |r| {
                r.get(0)
            })
            .unwrap();
        conn.execute(
            "INSERT INTO budgets(month, category_id, amount) VALUES('2025-08', ?1, '100.00')",
            params![cat_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions(date, amount, category_id, currency) VALUES('2025-08-10','-20',?1,'USD')",
            params![cat_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO fx_rates(date, base, quote, rate) VALUES('2025-08-31','USD','EUR','0.80')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn budget_report_converts_currency() {
        let conn = setup_conn();
        let rows_base = build_budget_report(&conn, "2025-08", "USD", None).unwrap();
        assert_eq!(
            rows_base,
            vec![vec![
                String::from("Dining"),
                String::from("100.00"),
                String::from("20.00"),
            ]]
        );

        let rows_eur = build_budget_report(&conn, "2025-08", "USD", Some("EUR")).unwrap();
        assert_eq!(
            rows_eur,
            vec![vec![
                String::from("Dining"),
                String::from("80.00"),
                String::from("16.00"),
            ]]
        );
    }
}
