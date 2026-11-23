//! SQLite storage. Everything lives in one local database file.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, params};

use crate::analysis::AgeSettings;
use crate::model::{Category, DEFAULT_CATEGORIES, DataSet, Item, LoyaltyType, Price, Source, Variant};
use crate::units::{Quantity, Unit};

/// Each entry upgrades the schema by one version. Never edit one that has shipped; add a new one.
const MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE data_set (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        currency_code TEXT NOT NULL,
        allow_metric INTEGER NOT NULL,
        allow_imperial INTEGER NOT NULL,
        allow_us_customary INTEGER NOT NULL,
        notes TEXT NOT NULL DEFAULT ''
    );

    CREATE TABLE item (
        id INTEGER PRIMARY KEY,
        data_set_id INTEGER NOT NULL REFERENCES data_set(id) ON DELETE CASCADE,
        name TEXT NOT NULL,
        default_unit INTEGER NOT NULL,
        allow_multipack INTEGER NOT NULL,
        notes TEXT NOT NULL DEFAULT ''
    );
    CREATE INDEX item_data_set ON item(data_set_id);

    CREATE TABLE source (
        id INTEGER PRIMARY KEY,
        data_set_id INTEGER NOT NULL REFERENCES data_set(id) ON DELETE CASCADE,
        name TEXT NOT NULL,
        loyalty_type INTEGER NOT NULL,
        loyalty_multiplier REAL NOT NULL,
        notes TEXT NOT NULL DEFAULT ''
    );
    CREATE INDEX source_data_set ON source(data_set_id);

    -- Every price ever recorded. The newest row for an (item, source) pair is its current price.
    CREATE TABLE price (
        id INTEGER PRIMARY KEY,
        item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
        source_id INTEGER NOT NULL REFERENCES source(id) ON DELETE CASCADE,
        price REAL NOT NULL,
        count INTEGER NOT NULL,
        quantity_in_base_unit REAL NOT NULL,
        user_unit INTEGER NOT NULL,
        confirmed_at INTEGER NOT NULL,
        notes TEXT NOT NULL DEFAULT '',
        modified_at INTEGER NOT NULL
    );
    CREATE INDEX price_item_source ON price(item_id, source_id, modified_at);
    CREATE INDEX price_source ON price(source_id);

    CREATE TABLE setting (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
"#, r#"
    CREATE TABLE category (
        id INTEGER PRIMARY KEY,
        data_set_id INTEGER NOT NULL REFERENCES data_set(id) ON DELETE CASCADE,
        name TEXT NOT NULL
    );
    CREATE INDEX category_data_set ON category(data_set_id);
    INSERT INTO category (data_set_id, name)
        SELECT data_set.id, defaults.name FROM data_set
        CROSS JOIN (SELECT 'Essentials' AS name UNION ALL SELECT 'Baking' UNION ALL SELECT 'Home') AS defaults;

    ALTER TABLE item ADD COLUMN category_id INTEGER REFERENCES category(id) ON DELETE SET NULL;

    CREATE TABLE variant (
        id INTEGER PRIMARY KEY,
        item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
        name TEXT NOT NULL
    );
    CREATE INDEX variant_item ON variant(item_id);

    -- The newest row for an (item, source, variant) is now the current price.
    ALTER TABLE price ADD COLUMN variant_id INTEGER REFERENCES variant(id) ON DELETE CASCADE;
    DROP INDEX price_item_source;
    CREATE INDEX price_item_source ON price(item_id, source_id, variant_id, modified_at);
    CREATE INDEX price_variant ON price(variant_id);
"#];

const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;
const TABLES: [&str; 5] = ["data_set", "item", "source", "price", "setting"];

const PRICE_COLUMNS: &str =
    "price.id, price.item_id, price.source_id, price.price, price.count, price.quantity_in_base_unit, \
     price.user_unit, price.confirmed_at, price.notes, price.modified_at, price.variant_id";

pub struct Db {
    conn: Connection,
}

/// `PENNY_PINCHER_DB` points the app at another database file, e.g. for trying things out.
pub fn default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("PENNY_PINCHER_DB") {
        return PathBuf::from(path);
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("Penny Pincher").join("penny-pincher.db")
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        Db::from_connection(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Db> {
        Db::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Db> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let mut db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i64 = self.conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            bail!("This database was made by a newer version of Penny Pincher.");
        }
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            let transaction = self.conn.transaction()?;
            transaction.execute_batch(sql)?;
            transaction.pragma_update(None, "user_version", index as i64 + 1)?;
            transaction.commit()?;
        }
        Ok(())
    }

    // Collections

    pub fn data_sets(&self) -> Result<Vec<DataSet>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, currency_code, allow_metric, allow_imperial, allow_us_customary, notes
             FROM data_set ORDER BY name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(DataSet {
                id: row.get(0)?,
                name: row.get(1)?,
                currency_code: row.get(2)?,
                allow_metric: row.get(3)?,
                allow_imperial: row.get(4)?,
                allow_us_customary: row.get(5)?,
                notes: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Inserts when `id` is 0, otherwise updates. Returns the id.
    pub fn save_data_set(&self, data_set: &DataSet) -> Result<i64> {
        let values = params![
            data_set.name,
            data_set.currency_code,
            data_set.allow_metric,
            data_set.allow_imperial,
            data_set.allow_us_customary,
            data_set.notes,
            data_set.id,
        ];
        if data_set.id == 0 {
            self.conn.execute(
                "INSERT INTO data_set (name, currency_code, allow_metric, allow_imperial, allow_us_customary, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                &values[..6],
            )?;
            let id = self.conn.last_insert_rowid();
            for name in DEFAULT_CATEGORIES {
                self.save_category(&Category { id: 0, data_set_id: id, name: name.to_string() })?;
            }
            Ok(id)
        } else {
            self.conn.execute(
                "UPDATE data_set SET name = ?1, currency_code = ?2, allow_metric = ?3, allow_imperial = ?4,
                 allow_us_customary = ?5, notes = ?6 WHERE id = ?7",
                values,
            )?;
            Ok(data_set.id)
        }
    }

    pub fn delete_data_set(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM data_set WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn count_items_and_sources(&self, data_set_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT (SELECT COUNT(*) FROM item WHERE data_set_id = ?1) + (SELECT COUNT(*) FROM source WHERE data_set_id = ?1)",
            [data_set_id],
            |row| row.get(0),
        )?)
    }

    // Categories

    pub fn categories(&self, data_set_id: i64) -> Result<Vec<Category>> {
        let mut statement =
            self.conn.prepare("SELECT id, data_set_id, name FROM category WHERE data_set_id = ?1 ORDER BY name COLLATE NOCASE")?;
        let rows = statement.query_map([data_set_id], |row| {
            Ok(Category { id: row.get(0)?, data_set_id: row.get(1)?, name: row.get(2)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn save_category(&self, category: &Category) -> Result<i64> {
        if category.id == 0 {
            self.conn
                .execute("INSERT INTO category (data_set_id, name) VALUES (?1, ?2)", params![category.data_set_id, category.name])?;
            Ok(self.conn.last_insert_rowid())
        } else {
            self.conn.execute("UPDATE category SET name = ?1 WHERE id = ?2", params![category.name, category.id])?;
            Ok(category.id)
        }
    }

    /// Its products stay, without a category.
    pub fn delete_category(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM category WHERE id = ?1", [id])?;
        Ok(())
    }

    // Products

    pub fn items(&self, data_set_id: i64) -> Result<Vec<Item>> {
        let mut statement = self.conn.prepare(
            "SELECT id, data_set_id, name, default_unit, allow_multipack, notes, category_id
             FROM item WHERE data_set_id = ?1 ORDER BY name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([data_set_id], |row| {
            Ok(Item {
                id: row.get(0)?,
                data_set_id: row.get(1)?,
                name: row.get(2)?,
                default_unit: unit_column(row, 3)?,
                allow_multipack: row.get(4)?,
                notes: row.get(5)?,
                category_id: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn save_item(&self, item: &Item) -> Result<i64> {
        let values = params![
            item.data_set_id,
            item.name,
            item.default_unit.id(),
            item.allow_multipack,
            item.notes,
            item.category_id,
            item.id,
        ];
        if item.id == 0 {
            self.conn.execute(
                "INSERT INTO item (data_set_id, name, default_unit, allow_multipack, notes, category_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                &values[..6],
            )?;
            Ok(self.conn.last_insert_rowid())
        } else {
            self.conn.execute(
                "UPDATE item SET data_set_id = ?1, name = ?2, default_unit = ?3, allow_multipack = ?4, notes = ?5,
                 category_id = ?6 WHERE id = ?7",
                values,
            )?;
            Ok(item.id)
        }
    }

    // Variants

    pub fn variants(&self, item_id: i64) -> Result<Vec<Variant>> {
        let mut statement =
            self.conn.prepare("SELECT id, item_id, name FROM variant WHERE item_id = ?1 ORDER BY name COLLATE NOCASE")?;
        let rows = statement.query_map([item_id], |row| Ok(Variant { id: row.get(0)?, item_id: row.get(1)?, name: row.get(2)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Makes the item's variants match `variants`: renames existing ones (by id), adds ones with id 0,
    /// and deletes the rest along with their prices.
    pub fn replace_variants(&mut self, item_id: i64, variants: &[Variant]) -> Result<()> {
        let transaction = self.conn.transaction()?;
        let kept: Vec<i64> = variants.iter().map(|variant| variant.id).filter(|id| *id != 0).collect();
        let existing: Vec<i64> = {
            let mut statement = transaction.prepare("SELECT id FROM variant WHERE item_id = ?1")?;
            statement.query_map([item_id], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        for id in existing.into_iter().filter(|id| !kept.contains(id)) {
            transaction.execute("DELETE FROM variant WHERE id = ?1", [id])?;
        }
        for variant in variants {
            if variant.id == 0 {
                transaction.execute("INSERT INTO variant (item_id, name) VALUES (?1, ?2)", params![item_id, variant.name])?;
            } else {
                transaction
                    .execute("UPDATE variant SET name = ?1 WHERE id = ?2 AND item_id = ?3", params![variant.name, variant.id, item_id])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn count_prices_for_variant(&self, variant_id: i64) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM price WHERE variant_id = ?1", [variant_id], |row| row.get(0))?)
    }

    pub fn delete_item(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM item WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn count_prices_for_item(&self, item_id: i64) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM price WHERE item_id = ?1", [item_id], |row| row.get(0))?)
    }

    // Stores

    pub fn sources(&self, data_set_id: i64) -> Result<Vec<Source>> {
        let mut statement = self.conn.prepare(
            "SELECT id, data_set_id, name, loyalty_type, loyalty_multiplier, notes
             FROM source WHERE data_set_id = ?1 ORDER BY name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([data_set_id], |row| {
            let loyalty_type: i64 = row.get(3)?;
            Ok(Source {
                id: row.get(0)?,
                data_set_id: row.get(1)?,
                name: row.get(2)?,
                loyalty_type: LoyaltyType::from_id(loyalty_type).unwrap_or(LoyaltyType::None),
                loyalty_multiplier: row.get(4)?,
                notes: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn save_source(&self, source: &Source) -> Result<i64> {
        let values = params![
            source.data_set_id,
            source.name,
            source.loyalty_type.id(),
            source.loyalty_multiplier,
            source.notes,
            source.id,
        ];
        if source.id == 0 {
            self.conn.execute(
                "INSERT INTO source (data_set_id, name, loyalty_type, loyalty_multiplier, notes) VALUES (?1, ?2, ?3, ?4, ?5)",
                &values[..5],
            )?;
            Ok(self.conn.last_insert_rowid())
        } else {
            self.conn.execute(
                "UPDATE source SET data_set_id = ?1, name = ?2, loyalty_type = ?3, loyalty_multiplier = ?4, notes = ?5
                 WHERE id = ?6",
                values,
            )?;
            Ok(source.id)
        }
    }

    pub fn delete_source(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM source WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn count_prices_for_source(&self, source_id: i64) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM price WHERE source_id = ?1", [source_id], |row| row.get(0))?)
    }

    // Prices

    /// The newest price for the item for each store and variant that has one.
    pub fn current_prices(&self, item_id: i64) -> Result<Vec<Price>> {
        let sql = format!(
            "SELECT {PRICE_COLUMNS} FROM price
             WHERE price.item_id = ?1 AND price.id = (
                 SELECT newest.id FROM price AS newest
                 WHERE newest.item_id = price.item_id AND newest.source_id = price.source_id
                   AND newest.variant_id IS price.variant_id
                 ORDER BY newest.modified_at DESC, newest.id DESC LIMIT 1
             )"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map([item_id], price_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every price recorded for the item and variant at the store, newest first.
    pub fn price_history(&self, item_id: i64, source_id: i64, variant_id: Option<i64>) -> Result<Vec<Price>> {
        let sql = format!(
            "SELECT {PRICE_COLUMNS} FROM price WHERE item_id = ?1 AND source_id = ?2 AND variant_id IS ?3
             ORDER BY modified_at DESC, id DESC"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params![item_id, source_id, variant_id], price_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Records a price. It becomes the current price for its item, store and variant.
    pub fn add_price(&self, price: &Price) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO price (item_id, source_id, variant_id, price, count, quantity_in_base_unit, user_unit,
                                confirmed_at, notes, modified_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                price.item_id,
                price.source_id,
                price.variant_id,
                price.price,
                price.count,
                price.quantity.in_base_unit(),
                price.quantity.unit.id(),
                price.confirmed_at,
                price.notes,
                price.modified_at,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn delete_price(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM price WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn delete_price_history(&self, item_id: i64, source_id: i64, variant_id: Option<i64>) -> Result<()> {
        self.conn.execute(
            "DELETE FROM price WHERE item_id = ?1 AND source_id = ?2 AND variant_id IS ?3",
            params![item_id, source_id, variant_id],
        )?;
        Ok(())
    }

    // Settings

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM setting WHERE key = ?1", [key], |row| row.get(0))
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO setting (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    pub fn age_settings(&self) -> Result<AgeSettings> {
        let defaults = AgeSettings::default();
        let number = |key: &str| -> Result<Option<f64>> { Ok(self.setting(key)?.and_then(|value| value.parse().ok())) };
        Ok(AgeSettings {
            stale_after_days: number("stale_after_days")?.map_or(defaults.stale_after_days, |days| days as i64),
            ancient_after_days: number("ancient_after_days")?.map_or(defaults.ancient_after_days, |days| days as i64),
            annual_inflation_percent: number("annual_inflation_percent")?.unwrap_or(defaults.annual_inflation_percent),
        })
    }

    pub fn save_age_settings(&self, settings: &AgeSettings) -> Result<()> {
        self.set_setting("stale_after_days", &settings.stale_after_days.to_string())?;
        self.set_setting("ancient_after_days", &settings.ancient_after_days.to_string())?;
        self.set_setting("annual_inflation_percent", &settings.annual_inflation_percent.to_string())?;
        Ok(())
    }

    // Backup and restore

    pub fn back_up_to(&self, path: &Path) -> Result<()> {
        if path.exists() {
            std::fs::remove_file(path).with_context(|| format!("replacing {}", path.display()))?;
        }
        let target = path.to_str().context("The backup path isn't valid text.")?;
        self.conn.execute("VACUUM INTO ?1", [target])?;
        Ok(())
    }

    /// Replaces all data with the backup at `path`.
    pub fn restore_from(&mut self, path: &Path) -> Result<()> {
        check_backup(path)?;
        self.conn
            .restore(rusqlite::MAIN_DB, path, None::<fn(rusqlite::backup::Progress)>)?;
        self.migrate()
    }
}

fn check_backup(path: &Path) -> Result<()> {
    let backup = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let has_table = |name: &str| -> Result<bool> {
        Ok(backup
            .query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |_| Ok(()))
            .optional()?
            .is_some())
    };
    for table in TABLES {
        if !has_table(table)? {
            bail!("This file isn't a Penny Pincher backup.");
        }
    }
    let version: i64 = backup.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        bail!("This backup was made by a newer version of Penny Pincher.");
    }
    Ok(())
}

fn unit_column(row: &Row, index: usize) -> rusqlite::Result<Unit> {
    let id: i64 = row.get(index)?;
    Unit::from_id(id).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Integer, format!("unknown unit {id}").into())
    })
}

fn price_from_row(row: &Row) -> rusqlite::Result<Price> {
    let unit = unit_column(row, 6)?;
    let quantity_in_base_unit: f64 = row.get(5)?;
    Ok(Price {
        id: row.get(0)?,
        item_id: row.get(1)?,
        source_id: row.get(2)?,
        price: row.get(3)?,
        count: row.get(4)?,
        quantity: Quantity::new(quantity_in_base_unit, unit.quantity_type().base_unit()).to(unit),
        confirmed_at: row.get(7)?,
        notes: row.get(8)?,
        modified_at: row.get(9)?,
        variant_id: row.get(10)?,
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn sample(db: &Db) -> (DataSet, Item, Source, Source) {
        let mut data_set = DataSet::new_default();
        data_set.name = "Groceries".into();
        data_set.id = db.save_data_set(&data_set).unwrap();
        let mut item = Item {
            id: 0,
            data_set_id: data_set.id,
            name: "Butter".into(),
            default_unit: Unit::Oz,
            allow_multipack: false,
            notes: String::new(),
            category_id: None,
        };
        item.id = db.save_item(&item).unwrap();
        let mut walmart = Source {
            id: 0,
            data_set_id: data_set.id,
            name: "Walmart".into(),
            loyalty_type: LoyaltyType::None,
            loyalty_multiplier: 1.0,
            notes: String::new(),
        };
        walmart.id = db.save_source(&walmart).unwrap();
        let mut kroger = walmart.clone();
        kroger.id = 0;
        kroger.name = "Kroger".into();
        kroger.id = db.save_source(&kroger).unwrap();
        (data_set, item, walmart, kroger)
    }

    pub fn butter_price(item: &Item, source: &Source, amount: f64, at: i64) -> Price {
        Price {
            id: 0,
            item_id: item.id,
            source_id: source.id,
            variant_id: None,
            price: amount,
            count: 1,
            quantity: Quantity::new(1.0, Unit::Lb),
            confirmed_at: at,
            notes: String::new(),
            modified_at: at,
        }
    }

    #[test]
    fn newest_price_is_current() {
        let db = Db::open_in_memory().unwrap();
        let (_, item, walmart, kroger) = sample(&db);
        db.add_price(&butter_price(&item, &walmart, 4.49, 1000)).unwrap();
        db.add_price(&butter_price(&item, &walmart, 4.79, 2000)).unwrap();
        db.add_price(&butter_price(&item, &kroger, 3.99, 1500)).unwrap();

        let mut current = db.current_prices(item.id).unwrap();
        current.sort_by_key(|price| price.source_id);
        assert_eq!(current.len(), 2);
        assert_eq!(current[0].price, 4.79);
        assert_eq!(current[0].quantity.unit, Unit::Lb);
        assert!((current[0].quantity.value - 1.0).abs() < 1e-9);
        assert_eq!(db.price_history(item.id, walmart.id, None).unwrap().len(), 2);
    }

    #[test]
    fn each_variant_has_its_own_current_price() {
        let mut db = Db::open_in_memory().unwrap();
        let (_, item, walmart, _) = sample(&db);
        db.replace_variants(item.id, &[Variant { id: 0, item_id: item.id, name: "Kerrygold".into() }]).unwrap();
        let kerrygold = db.variants(item.id).unwrap()[0].id;

        db.add_price(&butter_price(&item, &walmart, 4.49, 1000)).unwrap();
        db.add_price(&Price { variant_id: Some(kerrygold), ..butter_price(&item, &walmart, 6.99, 2000) }).unwrap();
        assert_eq!(db.current_prices(item.id).unwrap().len(), 2);
        assert_eq!(db.price_history(item.id, walmart.id, Some(kerrygold)).unwrap().len(), 1);

        // Removing the variant removes its prices.
        db.replace_variants(item.id, &[]).unwrap();
        let current = db.current_prices(item.id).unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].price, 4.49);
    }

    #[test]
    fn new_collections_get_default_categories() {
        let db = Db::open_in_memory().unwrap();
        let (data_set, ..) = sample(&db);
        let names: Vec<String> = db.categories(data_set.id).unwrap().into_iter().map(|category| category.name).collect();
        assert_eq!(names, ["Baking", "Essentials", "Home"]);
    }

    #[test]
    fn deleting_a_store_deletes_its_prices() {
        let db = Db::open_in_memory().unwrap();
        let (_, item, walmart, _) = sample(&db);
        db.add_price(&butter_price(&item, &walmart, 4.49, 1000)).unwrap();
        db.delete_source(walmart.id).unwrap();
        assert!(db.current_prices(item.id).unwrap().is_empty());
    }

    #[test]
    fn settings_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let settings = AgeSettings { stale_after_days: 10, ancient_after_days: 90, annual_inflation_percent: 3.5 };
        db.save_age_settings(&settings).unwrap();
        assert_eq!(db.age_settings().unwrap(), settings);
    }

    #[test]
    fn backup_restores() {
        let folder = std::env::temp_dir().join(format!("penny-pincher-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let backup = folder.join("backup.db");

        let db = Db::open_in_memory().unwrap();
        sample(&db);
        db.back_up_to(&backup).unwrap();

        let mut other = Db::open_in_memory().unwrap();
        other.restore_from(&backup).unwrap();
        assert_eq!(other.data_sets().unwrap()[0].name, "Groceries");
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
