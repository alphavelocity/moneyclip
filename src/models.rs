// Copyright (c) 2025 Soumyadip Sarkar.
// All rights reserved.
//
// This source code is licensed under the license found in the
// LICENSE file in the root directory of this source tree.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub r#type: String,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: i64,
    pub date: NaiveDate,
    pub account_id: i64,
    pub amount: Decimal,
    pub payee: String,
    pub category_id: Option<i64>,
    pub currency: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    pub id: i64,
    pub month: String, // YYYY-MM
    pub category_id: i64,
    pub amount: Decimal, // base currency
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: i64,
    pub ticker: String,
    pub name: String,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: i64,
    pub date: NaiveDate,
    pub asset_id: i64,
    pub account_id: i64,
    pub quantity: Decimal,
    pub price: Decimal,
    pub fees: Decimal,
    pub side: String,
    pub note: Option<String>,
}
