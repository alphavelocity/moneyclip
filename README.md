# Moneyclip

Multi-currency **personal finance**, **envelope budgeting**, and **portfolio** CLI.

## Features

- Multi-currency accounts/transactions with **base-currency** reporting
- Envelope budgeting: fund, move, roll over, and see available per category
- Reports: balances, cashflow, spend-by-category (with base conversion)
- Portfolio: trades, cached prices (Yahoo quote endpoint), FIFO capital gains
- Offline-first **SQLite** DB in your OS data dir, auto-initialized

## Build

```bash
cd moneyclip
cargo build --release
```

## Quickstart

```bash
moneyclip init

# Base currency & FX
moneyclip fx set-base --currency INR
moneyclip fx fetch --days 180
moneyclip fx list

# Accounts & categories
moneyclip account add --name "HDFC Savings" --type bank --currency INR
moneyclip account add --name "Revolut USD"  --type bank --currency USD
moneyclip category add --name Groceries
moneyclip category add --name Dining

# Transactions (account currency)
moneyclip tx add --date 2025-08-12 --account "HDFC Savings" --amount -1250.75 --payee "Big Bazaar" --category Groceries
moneyclip tx add --date 2025-08-10 --account "Revolut USD"  --amount -75.30   --payee "Amazon"     --category Groceries

# Envelopes (BASE currency)
moneyclip envelope fund  --month 2025-08 --category Groceries --amount 12000
moneyclip envelope move  --month 2025-08 --from Groceries --to Dining --amount 1000
moneyclip envelope status --month 2025-08

# Budget report (BASE)
moneyclip budget report --month 2025-08 --base

# Other reports (BASE)
moneyclip report balances --base
moneyclip report cashflow --base --months 6
moneyclip report spend-by-category --month 2025-08 --base

# Portfolio (optional)
moneyclip portfolio add-asset --ticker TCS.NS --name "Tata Consultancy Services" --currency INR
moneyclip portfolio price fetch
moneyclip portfolio value --live
moneyclip portfolio tax --year 2025
```

## APIs used

- **Frankfurter** — public API around **ECB** reference rates, including time series endpoints
  (published ~16:00 CET; information-only).
  - Docs: <https://frankfurter.dev> / <https://api.frankfurter.dev>
- **ECB** reference rates notes: updated around 16:00 CET each working day; for information only.
- **Yahoo Finance quote API** for latest equities pricing.
  Endpoint: `https://query1.finance.yahoo.com/v7/finance/quote?symbols=...`.

> Moneyclip caches rates and prices. Treat ECB rates as reference values, not execution prices.

## Notes

- All money math uses `rust_decimal` (no floating rounding) and per-transaction date FX.
- The DB lives in a platform data dir (e.g. Linux `~/.local/share/Moneyclip/moneyclip.sqlite`).

### FX tools

```bash
moneyclip fx convert --date 2025-08-15 --amount 100 --from EUR --to INR
moneyclip report balances --currency EUR
moneyclip report cashflow --currency INR --months 6
moneyclip budget report --month 2025-08 --currency USD
moneyclip envelope status --month 2025-08 --currency EUR
```

### Doctor

```bash
moneyclip doctor   # checks missing FX coverage & inconsistent currencies
```

### JSON / NDJSON output

All major reports accept `--json` (pretty JSON array) or `--jsonl` (one JSON object per line).
This makes it easy to pipe into tools like `jq`:

```bash
moneyclip report cashflow --months 6 --currency INR --json | jq
moneyclip report spend-by-category --month 2025-08 --jsonl
```

### Import rules (auto-categorize)

Create regex-based rules to auto-categorize imports (and `tx add` if no category is provided).
Rules match against the payee and optional memo; first match wins.

```bash
moneyclip category add --name Shopping
moneyclip rules add --pattern "(?i)amazon|amzn" --category Shopping --payee_rewrite "Amazon"
moneyclip import transactions --path statements.csv  # uncategorized rows get classified
moneyclip rules list
```

## License

See [LICENSE](LICENSE) for full license text.
