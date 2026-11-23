# Penny Pincher

A Windows desktop app for tracking grocery prices across stores and finding the best deal, even when stores sell different pack sizes.

Record what something costs at each store you shop at. Penny Pincher works out the unit price (per lb, per oz, per gallon, per item) and ranks the stores, taking into account:

- **Pack sizes and units.** 16 oz for $4.49 and 1 lb for $4.79 compare correctly. Multipacks (4 × 330 ml) are supported.
- **Loyalty schemes.** Store discounts and store rewards are folded into the price.
- **Old prices.** Prices you haven't seen in a while get an inflation adjustment, and very old ones don't count towards what a good price is.

Each price is marked good, OK or bad compared with the other stores. Everything is stored locally in SQLite, and the app works fully offline.

## Building

Requires Rust (edition 2024) and the Windows SDK (for embedding the icon).

```
cargo build --release
```

The app is `target/release/penny-pincher.exe`. Data lives in `%APPDATA%\Penny Pincher\penny-pincher.db`. Set `PENNY_PINCHER_DB` to use a different file. Use File > Back up to save a copy.

## Code layout

- `src/units.rs`: units of measure and conversion
- `src/money.rs`: currencies and number entry
- `src/model.rs`: collections, products, stores and prices
- `src/analysis.rs`: unit price comparison and good/OK/bad judgements
- `src/db.rs`: SQLite schema, migrations, backup and restore
- `src/forms.rs`, `src/app.rs`: the egui user interface

## Credits

A Rust port of [My Price Log](https://github.com/ZornsLemma/my-price-log) by Steven Flintham (MIT License).

The icon is the U.S. Mint's [one cent obverse image](https://commons.wikimedia.org/wiki/File:US_One_Cent_Obv.png) (public domain). `tools/make_icon.py` turns it into `assets/icon.png` and `assets/icon.ico`.
