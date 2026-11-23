# Penny Pincher

A Windows desktop app for tracking grocery prices across stores and finding the best deal, even when stores sell different pack sizes.

Record what something costs at each store you shop at. Penny Pincher works out the unit price (per lb, per oz, per gallon, per item) and ranks the stores, taking into account:

- **Pack sizes and units.** 16 oz for $4.49 and 1 lb for $4.79 compare correctly. Multipacks (4 × 330 ml) are supported.
- **Store discounts and surcharges.** Member discounts, store rewards and markups (such as a delivery service adding 25%) are folded into the price.
- **Brands and variants.** Track Kerrygold and store-brand butter separately and see them ranked together.
- **Old prices.** Prices you haven't seen in a while get an inflation adjustment, and very old ones don't count towards what a good price is.

Each price is marked good, OK or bad compared with the other stores. Products are grouped into categories (Essentials, Baking, Home and any you add), and there's a unit converter for amounts and prices per unit. Everything is stored locally in SQLite, and the app works fully offline.

## Installing

Download `penny-pincher-setup.exe` from the [latest release](https://github.com/petmar20/penny-pincher/releases/latest) and run it. It installs for your Windows account only (no admin needed), adds a Start menu shortcut, and can be removed from Settings > Apps.

The installer isn't code-signed yet, so Windows may warn about it. If you see "Windows protected your PC", choose More info > Run anyway. If nothing happens at all, Smart App Control may be blocking it: check Windows Security > App & browser control.

## Building

Requires Rust (edition 2024) and the Windows SDK (for embedding the icon).

```
cargo build --release
```

The app is `target/release/penny-pincher.exe`. Data lives in `%APPDATA%\Penny Pincher\penny-pincher.db`. Set `PENNY_PINCHER_DB` to use a different file. Use File > Back up to save a copy.

## Updates and releases

The app checks GitHub for a newer release when it starts (quietly, so it still works offline) and from Help > Check for updates. Updating downloads the new `penny-pincher.exe` and swaps it in; it takes effect after a restart.

To publish a release, bump `version` in `Cargo.toml`, commit, then tag and push:

```
git tag v0.2.0
git push origin v0.2.0
```

The Release workflow builds the exe on Windows, runs the tests, builds the installer with Inno Setup (`installer/penny-pincher.iss`), and attaches both `penny-pincher-setup.exe` and `penny-pincher.exe` to a new GitHub release. The updater uses `penny-pincher.exe`.

If the app can't start, it shows a message and writes details to `%LOCALAPPDATA%\Penny Pincher\penny-pincher.log`.

## Code layout

- `src/units.rs`: units of measure and conversion
- `src/money.rs`: currencies and number entry
- `src/model.rs`: products, categories, variants, stores and prices
- `src/analysis.rs`: unit price comparison and good/OK/bad judgements
- `src/db.rs`: SQLite schema, migrations, backup and restore
- `src/converter.rs`: the unit converter
- `src/update.rs`: updating from GitHub releases
- `src/forms.rs`, `src/app.rs`, `src/touch.rs`: the egui user interface

## Credits

A Rust port of [My Price Log](https://github.com/ZornsLemma/my-price-log) by Steven Flintham (MIT License).

The icon is the U.S. Mint's [one cent obverse image](https://commons.wikimedia.org/wiki/File:US_One_Cent_Obv.png) (public domain). `tools/make_icon.py` turns it into `assets/icon.png` and `assets/icon.ico`.
