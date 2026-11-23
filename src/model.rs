//! The records stored in the database.

use crate::money::{self, Currency};
use crate::units::{QuantityType, Quantity, Unit, UnitFamily};

/// The app's currency and units. There's one, and every product, store and category belongs to it.
#[derive(Clone, Debug, PartialEq)]
pub struct DataSet {
    pub id: i64,
    pub currency_code: String,
    pub allow_metric: bool,
    pub allow_imperial: bool,
    pub allow_us_customary: bool,
}

impl DataSet {
    pub fn new_default() -> Self {
        DataSet {
            id: 0,
            currency_code: money::DEFAULT_CURRENCY.to_string(),
            allow_metric: true,
            allow_imperial: false,
            allow_us_customary: true,
        }
    }

    pub fn currency(&self) -> &'static Currency {
        money::currency(&self.currency_code)
    }

    /// Imperial and US customary are never both enabled, so "pint" is never ambiguous.
    pub fn unit_families(&self) -> Vec<UnitFamily> {
        let mut families = vec![UnitFamily::Item];
        if self.allow_us_customary {
            families.push(UnitFamily::UsCustomary);
        }
        if self.allow_metric {
            families.push(UnitFamily::Metric);
        }
        if self.allow_imperial {
            families.push(UnitFamily::Imperial);
        }
        families
    }

    /// Units the user can pick for a pack size (or, with `include_display_only`, for a unit price).
    pub fn units_for(&self, quantity_type: QuantityType, include_display_only: bool) -> Vec<Unit> {
        let families = self.unit_families();
        Unit::ALL
            .into_iter()
            .filter(|unit| {
                let info = unit.info();
                info.quantity_type == quantity_type
                    && (include_display_only || !info.display_only)
                    && families.iter().any(|family| unit.belongs_to(*family))
            })
            .collect()
    }

    /// Units in the same system as `unit`, used to choose a unit price denominator.
    pub fn units_like(&self, unit: Unit) -> Vec<Unit> {
        let family = self
            .unit_families()
            .into_iter()
            .find(|family| unit.belongs_to(*family));
        match family {
            Some(family) => Unit::ALL
                .into_iter()
                .filter(|candidate| candidate.quantity_type() == unit.quantity_type() && candidate.belongs_to(family))
                .collect(),
            None => self.units_for(unit.quantity_type(), true),
        }
    }

    /// The unit to show for a stored unit, swapping in an enabled one if the unit settings changed
    /// since (e.g. imperial pints after switching to metric).
    pub fn usable_unit(&self, unit: Unit) -> Unit {
        let families = self.unit_families();
        if families.iter().any(|family| unit.belongs_to(*family)) {
            unit
        } else {
            self.units_for(unit.quantity_type(), false)[0]
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: i64,
    pub data_set_id: i64,
    pub name: String,
    /// Also decides whether the item is sold by count, weight or volume.
    pub default_unit: Unit,
    pub allow_multipack: bool,
    pub notes: String,
    pub category_id: Option<i64>,
}

impl Item {
    pub fn quantity_type(&self) -> QuantityType {
        self.default_unit.quantity_type()
    }
}

/// A group of products, such as Baking or Home.
#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    pub id: i64,
    pub data_set_id: i64,
    pub name: String,
}

pub const DEFAULT_CATEGORIES: [&str; 3] = ["Essentials", "Baking", "Home"];

/// A brand or kind of a product, such as Kerrygold butter. Prices can be for one variant.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub id: i64,
    pub item_id: i64,
    pub name: String,
}

/// How a store's prices differ from the shelf price, such as a member discount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoyaltyType {
    None,
    /// Points or credit usable only at the store. A 5% bonus means $100 of goods costs $95.24.
    Bonus,
    /// Money off or money back. A 5% discount means $100 of goods costs $95.
    Discount,
    /// Added at the till, such as a delivery service's markup. 25% means $100 of goods costs $125.
    Surcharge,
}

impl LoyaltyType {
    pub const ALL: [LoyaltyType; 4] = [LoyaltyType::None, LoyaltyType::Bonus, LoyaltyType::Discount, LoyaltyType::Surcharge];

    pub fn id(self) -> i64 {
        match self {
            LoyaltyType::None => 1,
            LoyaltyType::Bonus => 2,
            LoyaltyType::Discount => 3,
            LoyaltyType::Surcharge => 4,
        }
    }

    pub fn from_id(id: i64) -> Option<LoyaltyType> {
        LoyaltyType::ALL.into_iter().find(|loyalty_type| loyalty_type.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            LoyaltyType::None => "None",
            LoyaltyType::Bonus => "Store rewards",
            LoyaltyType::Discount => "Discount",
            LoyaltyType::Surcharge => "Surcharge",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            LoyaltyType::None => "",
            LoyaltyType::Bonus => "Points or credit you can only spend at this store",
            LoyaltyType::Discount => "Money off your basket, or money back",
            LoyaltyType::Surcharge => "Added to every price, like a delivery markup",
        }
    }

    /// The largest percentage that makes sense for this kind of adjustment.
    pub fn max_percent(self) -> f64 {
        match self {
            LoyaltyType::Surcharge => 1000.0,
            _ => 99.99,
        }
    }

    pub fn multiplier_from_percent(self, percent: f64) -> f64 {
        match self {
            LoyaltyType::None => 1.0,
            LoyaltyType::Bonus => 100.0 / (100.0 + percent),
            LoyaltyType::Discount => 1.0 - percent / 100.0,
            LoyaltyType::Surcharge => 1.0 + percent / 100.0,
        }
    }

    pub fn percent_from_multiplier(self, multiplier: f64) -> f64 {
        match self {
            LoyaltyType::None => 0.0,
            LoyaltyType::Bonus => 100.0 / multiplier - 100.0,
            LoyaltyType::Discount => 100.0 * (1.0 - multiplier),
            LoyaltyType::Surcharge => 100.0 * (multiplier - 1.0),
        }
    }
}

/// A store, website or anywhere else that sells things.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub id: i64,
    pub data_set_id: i64,
    pub name: String,
    pub loyalty_type: LoyaltyType,
    /// Multiply a shelf price by this to get what it really costs after rewards or surcharges.
    pub loyalty_multiplier: f64,
    pub notes: String,
}

/// One recorded price. The rows for an item, store and variant form its history; the newest is current.
#[derive(Clone, Debug, PartialEq)]
pub struct Price {
    pub id: i64,
    pub item_id: i64,
    pub source_id: i64,
    /// None when the price isn't for a particular brand or kind.
    pub variant_id: Option<i64>,
    pub price: f64,
    /// Packs in a multipack: "$5 for 4 x 330 ml" has a count of 4.
    pub count: i64,
    /// Size of one pack, in the unit the user typed it in.
    pub quantity: Quantity,
    /// When the user last saw this price on the shelf, in milliseconds since 1970.
    pub confirmed_at: i64,
    pub notes: String,
    pub modified_at: i64,
}

impl Price {
    pub fn price_per_base_unit(&self, effective_price: f64) -> f64 {
        effective_price / (self.count as f64 * self.quantity.in_base_unit())
    }

    /// "$3.49 for 16 oz" or "$5.00 for 4 × 330 ml".
    pub fn describe(&self, currency: &Currency) -> String {
        let price = currency.format(self.price);
        if self.count == 1 {
            format!("{price} for {}", self.quantity.display())
        } else {
            format!("{price} for {} × {}", self.count, self.quantity.display())
        }
    }
}

pub fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loyalty_percentages_round_trip() {
        assert!((LoyaltyType::Surcharge.multiplier_from_percent(25.0) - 1.25).abs() < 1e-9);
        for loyalty_type in [LoyaltyType::Bonus, LoyaltyType::Discount, LoyaltyType::Surcharge] {
            let multiplier = loyalty_type.multiplier_from_percent(5.0);
            assert!((loyalty_type.percent_from_multiplier(multiplier) - 5.0).abs() < 1e-9);
        }
        assert!((LoyaltyType::Bonus.multiplier_from_percent(5.0) * 100.0 - 95.238).abs() < 0.001);
    }

    #[test]
    fn swaps_units_that_are_no_longer_enabled() {
        let mut data_set = DataSet::new_default();
        data_set.allow_us_customary = false;
        assert_eq!(data_set.usable_unit(Unit::UsPint), Unit::Ml);
        assert_eq!(data_set.usable_unit(Unit::Kg), Unit::Kg);
    }
}
