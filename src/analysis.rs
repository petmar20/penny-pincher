//! Compares an item's current prices across stores.

use crate::model::{Price, Source, Variant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AgeSettings {
    /// Prices confirmed more than this many days ago get an inflation adjustment.
    pub stale_after_days: i64,
    /// Prices confirmed more than this many days ago don't count towards good/OK/bad.
    pub ancient_after_days: i64,
    pub annual_inflation_percent: f64,
}

impl Default for AgeSettings {
    fn default() -> Self {
        AgeSettings { stale_after_days: 30, ancient_after_days: 180, annual_inflation_percent: 5.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgeClass {
    Fresh,
    Stale,
    Ancient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judgement {
    None,
    Good,
    Ok,
    Bad,
}

#[derive(Clone, Debug)]
pub struct ComparedPrice {
    pub price: Price,
    pub source_name: String,
    pub variant_name: Option<String>,
    pub age_days: i64,
    pub age_class: AgeClass,
    /// After store discounts or surcharges and the inflation adjustment for old prices.
    pub adjusted_price: f64,
    pub adjusted_price_per_base_unit: f64,
    pub judgement: Judgement,
}

impl ComparedPrice {
    pub fn is_adjusted(&self) -> bool {
        (self.adjusted_price - self.price.price).abs() > 1e-9
    }

    /// "Walmart" or "Walmart · Kerrygold".
    pub fn title(&self) -> String {
        match &self.variant_name {
            Some(variant) => format!("{} · {variant}", self.source_name),
            None => self.source_name.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    /// Unit prices below this (per base unit) are good.
    pub good_below: f64,
    /// Unit prices above this (per base unit) are bad.
    pub bad_above: f64,
}

/// `prices` are the current prices for one item, one per store and variant. The result is cheapest first.
pub fn compare(
    prices: &[Price],
    sources: &[Source],
    variants: &[Variant],
    settings: &AgeSettings,
    now_millis: i64,
) -> (Vec<ComparedPrice>, Option<Thresholds>) {
    let mut compared: Vec<ComparedPrice> = prices
        .iter()
        .filter_map(|price| {
            let source = sources.iter().find(|source| source.id == price.source_id)?;
            let age_days = (now_millis - price.confirmed_at).max(0) / 86_400_000;
            let age_class = if age_days <= settings.stale_after_days {
                AgeClass::Fresh
            } else if age_days <= settings.ancient_after_days {
                AgeClass::Stale
            } else {
                AgeClass::Ancient
            };
            let loyalty_price = price.price * source.loyalty_multiplier;
            let adjusted_price = inflation_adjusted(loyalty_price, age_days, settings);
            Some(ComparedPrice {
                price: price.clone(),
                source_name: source.name.clone(),
                variant_name: price
                    .variant_id
                    .and_then(|id| variants.iter().find(|variant| variant.id == id))
                    .map(|variant| variant.name.clone()),
                age_days,
                age_class,
                adjusted_price,
                adjusted_price_per_base_unit: price.price_per_base_unit(adjusted_price),
                judgement: Judgement::None,
            })
        })
        .collect();

    compared.sort_by(|a, b| {
        a.adjusted_price_per_base_unit
            .total_cmp(&b.adjusted_price_per_base_unit)
            .then_with(|| a.title().to_lowercase().cmp(&b.title().to_lowercase()))
    });

    let recent: Vec<f64> = compared
        .iter()
        .filter(|price| price.age_class != AgeClass::Ancient)
        .map(|price| price.adjusted_price_per_base_unit)
        .collect();

    // With so few prices we only flag ones clearly outside a buffered interquartile range.
    let thresholds = if recent.len() <= 2 {
        None
    } else {
        let buffer = 0.1;
        Some(Thresholds {
            good_below: quantile(&recent, 0.25) * (1.0 - buffer),
            bad_above: quantile(&recent, 0.75) * (1.0 + buffer),
        })
    };

    for price in &mut compared {
        price.judgement = judge(price.adjusted_price_per_base_unit, thresholds);
    }
    (compared, thresholds)
}

pub fn judge(price_per_base_unit: f64, thresholds: Option<Thresholds>) -> Judgement {
    match thresholds {
        None => Judgement::None,
        Some(thresholds) if price_per_base_unit < thresholds.good_below => Judgement::Good,
        Some(thresholds) if price_per_base_unit <= thresholds.bad_above => Judgement::Ok,
        Some(_) => Judgement::Bad,
    }
}

/// Inflation only starts once a price becomes stale, so it doesn't jump on that day.
fn inflation_adjusted(price: f64, age_days: i64, settings: &AgeSettings) -> f64 {
    if age_days <= settings.stale_after_days {
        price
    } else {
        let years = (age_days - settings.stale_after_days) as f64 / 365.25;
        price * (1.0 + settings.annual_inflation_percent / 100.0).powf(years)
    }
}

/// Linear interpolation between the closest ranks. `sorted` must be sorted and non-empty.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    let position = q * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = (position.ceil() as usize).min(sorted.len() - 1);
    let fraction = position - lower as f64;
    sorted[lower] * (1.0 - fraction) + sorted[upper] * fraction
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LoyaltyType;
    use crate::units::{Quantity, Unit};

    const DAY: i64 = 86_400_000;

    fn source(id: i64, name: &str, loyalty_multiplier: f64) -> Source {
        Source {
            id,
            data_set_id: 1,
            name: name.to_string(),
            loyalty_type: LoyaltyType::Discount,
            loyalty_multiplier,
            notes: String::new(),
        }
    }

    fn price(source_id: i64, amount: f64, ounces: f64, confirmed_at: i64) -> Price {
        Price {
            id: source_id,
            item_id: 1,
            source_id,
            variant_id: None,
            price: amount,
            count: 1,
            quantity: Quantity::new(ounces, Unit::Oz),
            confirmed_at,
            notes: String::new(),
            modified_at: confirmed_at,
        }
    }

    #[test]
    fn compares_different_pack_sizes_by_unit_price() {
        let now = 1000 * DAY;
        let sources = [source(1, "Walmart", 1.0), source(2, "Kroger", 1.0)];
        // 16 oz for $4.49 vs 8 oz for $2.00: the small pack is cheaper per ounce.
        let prices = [price(1, 4.49, 16.0, now), price(2, 2.00, 8.0, now)];
        let (compared, thresholds) = compare(&prices, &sources, &[], &AgeSettings::default(), now);
        assert_eq!(compared[0].source_name, "Kroger");
        assert!(thresholds.is_none());
    }

    #[test]
    fn applies_loyalty_discount() {
        let now = 1000 * DAY;
        let sources = [source(1, "Walmart", 1.0), source(2, "Costco", 0.9)];
        let prices = [price(1, 4.00, 16.0, now), price(2, 4.20, 16.0, now)];
        let (compared, _) = compare(&prices, &sources, &[], &AgeSettings::default(), now);
        assert_eq!(compared[0].source_name, "Costco");
        assert!((compared[0].adjusted_price - 3.78).abs() < 1e-9);
    }

    #[test]
    fn applies_surcharge() {
        let now = 1000 * DAY;
        // Instacart-style 25% markup makes a $3.50 shelf price cost $4.375.
        let sources = [source(1, "Walmart", 1.0), source(2, "Delivery", 1.25)];
        let prices = [price(1, 4.00, 16.0, now), price(2, 3.50, 16.0, now)];
        let (compared, _) = compare(&prices, &sources, &[], &AgeSettings::default(), now);
        assert_eq!(compared[0].source_name, "Walmart");
        assert!((compared[1].adjusted_price - 4.375).abs() < 1e-9);
    }

    #[test]
    fn inflates_stale_prices_and_ignores_ancient_ones_for_thresholds() {
        let now = 1000 * DAY;
        let settings = AgeSettings::default();
        let sources: Vec<Source> = (1..=4).map(|id| source(id, &format!("Store {id}"), 1.0)).collect();
        let prices = [
            price(1, 1.0, 1.0, now),
            price(2, 2.0, 1.0, now),
            price(3, 3.0, 1.0, now - 60 * DAY),
            price(4, 0.5, 1.0, now - 400 * DAY),
        ];
        let (compared, thresholds) = compare(&prices, &sources, &[], &settings, now);
        let stale = compared.iter().find(|price| price.source_name == "Store 3").unwrap();
        assert_eq!(stale.age_class, AgeClass::Stale);
        assert!(stale.adjusted_price > 3.0);
        let ancient = compared.iter().find(|price| price.source_name == "Store 4").unwrap();
        assert_eq!(ancient.age_class, AgeClass::Ancient);
        assert!(thresholds.is_some());
        assert_eq!(compared.iter().find(|price| price.source_name == "Store 1").unwrap().judgement, Judgement::Good);
    }

    #[test]
    fn quantile_interpolates() {
        assert_eq!(quantile(&[1.0, 2.0, 3.0, 4.0, 5.0], 0.25), 2.0);
        assert_eq!(quantile(&[1.0, 2.0], 0.5), 1.5);
    }
}
