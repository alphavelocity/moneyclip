// Copyright (c) AlphaVelocity.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use crate::utils::pretty_table;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

pub fn handle(conn: &Connection) -> Result<()> {
    let mut rows = Vec::new();

    // 1) Unknown currencies
    let mut stmt = conn.prepare(
        "SELECT DISTINCT currency FROM transactions EXCEPT SELECT currency FROM accounts",
    )?;
    let mut cur = stmt.query([])?;
    while let Some(r) = cur.next()? {
        let c: String = r.get(0)?;
        rows.push(vec!["txn_currency_no_account".into(), c]);
    }

    // 2) FX coverage gaps: transactions with currency != base lacking a rate on or before date
    let base = crate::utils::get_base_currency(conn)?;
    let mut stmt2 =
        conn.prepare("SELECT date, currency FROM transactions WHERE currency != ?1 ORDER BY date")?;
    let mut cur2 = stmt2.query([&base])?;
    while let Some(r) = cur2.next()? {
        let d: String = r.get(0)?;
        let ccy: String = r.get(1)?;
        let _date = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")?;
        let mut st = conn.prepare("SELECT 1 FROM fx_rates WHERE base=?1 AND quote=?2 AND date<=?3 ORDER BY date DESC LIMIT 1")?;
        let ok: Option<i32> = st.query_row((&base, &ccy, &d), |r| r.get(0)).optional()?;
        if ok.is_none() {
            rows.push(vec!["missing_fx".into(), format!("{} {}", d, ccy)]);
        }
    }

    if rows.is_empty() {
        println!("✅ doctor: no issues found");
    } else {
        println!("{}", pretty_table(&["Issue", "Detail"], rows));
    }
    Ok(())
}
