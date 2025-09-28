// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use anyhow::Result;

use moneyclip::{cli, commands, db};

fn main() -> Result<()> {
    let cli = cli::build_cli();
    let matches = cli.get_matches();

    let mut conn = db::open_or_init()?;

    match matches.subcommand() {
        Some(("init", _)) => {
            println!("Database initialized at {}", db::db_path()?.display());
        }
        Some(("account", sub)) => commands::accounts::handle(&conn, sub)?,
        Some(("category", sub)) => commands::categories::handle(&conn, sub)?,
        Some(("tx", sub)) => commands::transactions::handle(&conn, sub)?,
        Some(("budget", sub)) => commands::budgets::handle(&conn, sub)?,
        Some(("report", sub)) => commands::reports::handle(&conn, sub)?,
        Some(("portfolio", sub)) => commands::portfolio::handle(&mut conn, sub)?,
        Some(("import", sub)) => commands::importer::handle(&mut conn, sub)?,
        Some(("export", sub)) => commands::exporter::handle(&conn, sub)?,
        Some(("fx", sub)) => commands::fx::handle(&mut conn, sub)?,
        Some(("doctor", _)) => commands::doctor::handle(&conn)?,
        Some(("envelope", sub)) => commands::envelopes::handle(&conn, sub)?,
        Some(("rules", sub)) => commands::rules::handle(&conn, sub)?,
        _ => {
            cli::build_cli().print_help()?;
            println!();
        }
    }
    Ok(())
}
