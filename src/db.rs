// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use anyhow::{Context, Result};
use directories::ProjectDirs;
use once_cell::sync::Lazy;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;

static APP: Lazy<(&str, &str, &str)> =
    Lazy::new(|| ("com.alphavelocity", "Moneyclip", "moneyclip"));

pub fn db_path() -> Result<PathBuf> {
    let proj = ProjectDirs::from(APP.0, APP.1, APP.2)
        .context("Could not determine platform-specific data dir")?;
    let data_dir = proj.data_dir();
    fs::create_dir_all(data_dir).context("Failed to create data dir")?;
    Ok(data_dir.join("moneyclip.sqlite"))
}

pub fn open_or_init() -> Result<Connection> {
    let path = db_path()?;
    let mut conn =
        Connection::open(&path).with_context(|| format!("Open DB at {}", path.display()))?;
    init_schema(&mut conn)?;
    Ok(conn)
}

fn init_schema(conn: &mut Connection) -> Result<()> {
    conn.execute_batch(
        r#"
    PRAGMA foreign_keys = ON;

    CREATE TABLE IF NOT EXISTS settings(
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS accounts(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL UNIQUE,
        type TEXT NOT NULL,
        currency TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE IF NOT EXISTS categories(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL UNIQUE
    );

    CREATE TABLE IF NOT EXISTS transactions(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        date TEXT NOT NULL,
        account_id INTEGER NOT NULL,
        amount TEXT NOT NULL,
        payee TEXT NOT NULL,
        category_id INTEGER,
        currency TEXT NOT NULL,
        note TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE,
        FOREIGN KEY(category_id) REFERENCES categories(id) ON DELETE SET NULL
    );
    CREATE INDEX IF NOT EXISTS idx_transactions_date ON transactions(date);

    CREATE TABLE IF NOT EXISTS budgets(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        month TEXT NOT NULL,
        category_id INTEGER NOT NULL,
        amount TEXT NOT NULL, -- stored in BASE currency
        UNIQUE(month, category_id),
        FOREIGN KEY(category_id) REFERENCES categories(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS assets(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        ticker TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL,
        currency TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS trades(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        date TEXT NOT NULL,
        asset_id INTEGER NOT NULL,
        account_id INTEGER NOT NULL,
        quantity TEXT NOT NULL,
        price TEXT NOT NULL,
        fees TEXT NOT NULL DEFAULT '0',
        side TEXT NOT NULL CHECK(side IN ('buy','sell')),
        note TEXT,
        FOREIGN KEY(asset_id) REFERENCES assets(id) ON DELETE CASCADE,
        FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_trades_date ON trades(date);

    CREATE TABLE IF NOT EXISTS prices(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        asset_id INTEGER NOT NULL,
        as_of TEXT NOT NULL,
        price TEXT NOT NULL,
        source TEXT NOT NULL,
        UNIQUE(asset_id, as_of),
        FOREIGN KEY(asset_id) REFERENCES assets(id) ON DELETE CASCADE
    );

    -- FX rates: store base->quote rate (1 base = rate quote) per day
    CREATE TABLE IF NOT EXISTS fx_rates(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        date TEXT NOT NULL,
        base TEXT NOT NULL,
        quote TEXT NOT NULL,
        rate TEXT NOT NULL,
        UNIQUE(date, base, quote)
    );

    CREATE TABLE IF NOT EXISTS rules(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        pattern TEXT NOT NULL,
        category_id INTEGER,
        payee_rewrite TEXT,
        note TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        FOREIGN KEY(category_id) REFERENCES categories(id) ON DELETE SET NULL
    );
    "#,
    )?;
    Ok(())
}
