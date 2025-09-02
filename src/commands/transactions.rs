// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::{
    apply_import_rules, id_for_account, id_for_category, maybe_print_json, parse_date,
    parse_decimal, pretty_table,
};
use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

pub fn handle(conn: &Connection, m: &clap::ArgMatches) -> Result<()> {
    match m.subcommand() {
        Some(("add", sub)) => add(conn, sub)?,
        Some(("list", sub)) => list(conn, sub)?,
        _ => {}
    }
    Ok(())
}

fn add(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let date = parse_date(sub.get_one::<String>("date").unwrap())?;
    let account_name = sub.get_one::<String>("account").unwrap();
    let amount = parse_decimal(sub.get_one::<String>("amount").unwrap())?;
    let payee = sub.get_one::<String>("payee").unwrap();
    let category = sub.get_one::<String>("category").map(|s| s.to_string());
    let note = sub.get_one::<String>("note").map(|s| s.to_string());

    let account_id = id_for_account(conn, account_name)?;
    let currency: String = conn.query_row(
        "SELECT currency FROM accounts WHERE id=?1",
        params![account_id],
        |r| r.get(0),
    )?;
    let mut category_id = if let Some(cat) = category {
        Some(id_for_category(conn, &cat)?)
    } else {
        None
    };

    if category_id.is_none() {
        let (rule_cat, rewrite) = apply_import_rules(conn, payee, None)?;
        if category_id.is_none() {
            category_id = rule_cat;
        }
        if let Some(newp) = rewrite {
            println!("Payee rewritten: {} -> {}", payee, newp);
        }
    }

    conn.execute(
        "INSERT INTO transactions(date, account_id, amount, payee, category_id, currency, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            date.to_string(),
            account_id,
            amount.to_string(),
            payee,
            category_id,
            currency,
            note
        ],
    )?;
    println!(
        "Recorded {} on {} at '{}' (acct: {})",
        amount, date, payee, account_name
    );
    Ok(())
}

fn list(conn: &Connection, sub: &clap::ArgMatches) -> Result<()> {
    let json_flag = sub.get_flag("json");
    let jsonl_flag = sub.get_flag("jsonl");
    let data = query_rows(conn, sub)?;
    if !maybe_print_json(json_flag, jsonl_flag, &data)? {
        let rows: Vec<Vec<String>> = data
            .iter()
            .map(|r| {
                vec![
                    r.date.clone(),
                    r.account.clone(),
                    r.payee.clone(),
                    r.amount.clone(),
                    r.currency.clone(),
                    r.category.clone(),
                    r.note.clone(),
                ]
            })
            .collect();
        println!(
            "{}",
            pretty_table(
                &["Date", "Account", "Payee", "Amount", "CCY", "Category", "Note"],
                rows,
            )
        );
    }
    Ok(())
}

#[derive(Serialize)]
pub struct TransactionRow {
    pub date: String,
    pub account: String,
    pub payee: String,
    pub amount: String,
    pub currency: String,
    pub category: String,
    pub note: String,
}

pub fn query_rows(conn: &Connection, sub: &clap::ArgMatches) -> Result<Vec<TransactionRow>> {
    let mut sql = String::from(
        "SELECT t.date, a.name, t.payee, t.amount, t.currency, c.name, t.note FROM transactions t LEFT JOIN accounts a ON t.account_id=a.id LEFT JOIN categories c ON t.category_id=c.id WHERE 1=1",
    );
    let mut params_vec: Vec<String> = Vec::new();

    if let Some(month) = sub.get_one::<String>("month") {
        sql.push_str(" AND substr(t.date,1,7)=?");
        params_vec.push(month.into());
    }
    if let Some(acct) = sub.get_one::<String>("account") {
        sql.push_str(" AND a.name=?");
        params_vec.push(acct.into());
    }
    if let Some(cat) = sub.get_one::<String>("category") {
        sql.push_str(" AND c.name=?");
        params_vec.push(cat.into());
    }
    sql.push_str(" ORDER BY t.date DESC, t.id DESC");
    if let Some(limit) = sub.get_one::<usize>("limit") {
        sql.push_str(" LIMIT ?");
        params_vec.push(limit.to_string());
    }

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = if params_vec.is_empty() {
        stmt.query([])?
    } else {
        let params: Vec<&dyn rusqlite::ToSql> = params_vec
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();
        stmt.query(rusqlite::params_from_iter(params))?
    };

    let mut data = Vec::new();
    while let Some(r) = rows.next()? {
        let date: String = r.get(0)?;
        let account: String = r.get(1)?;
        let payee: String = r.get(2)?;
        let amount: String = r.get(3)?;
        let currency: String = r.get(4)?;
        let category: Option<String> = r.get(5)?;
        let note: Option<String> = r.get(6)?;
        data.push(TransactionRow {
            date,
            account,
            payee,
            amount,
            currency,
            category: category.unwrap_or_default(),
            note: note.unwrap_or_default(),
        });
    }
    Ok(data)
}
