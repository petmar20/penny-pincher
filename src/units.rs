//! Units of measure and conversion between them.
//!
//! Every quantity is weight, volume or a count of items. We never convert between those
//! (no density guesses), only between units of the same kind.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum QuantityType {
    Item,
    Weight,
    Volume,
}

impl QuantityType {
    pub const ALL: [QuantityType; 3] = [QuantityType::Item, QuantityType::Weight, QuantityType::Volume];

    pub fn label(self) -> &'static str {
        match self {
            QuantityType::Item => "Item",
            QuantityType::Weight => "Weight",
            QuantityType::Volume => "Volume",
        }
    }

    /// Prices are stored with their quantity in this unit.
    pub fn base_unit(self) -> Unit {
        match self {
            QuantityType::Item => Unit::Each,
            QuantityType::Weight => Unit::G,
            QuantityType::Volume => Unit::Ml,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum UnitFamily {
    Item,
    Metric,
    Imperial,
    UsCustomary,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Unit {
    Each,
    Each10,
    Each100,
    Oz,
    Lb,
    UsFloz,
    UsPint,
    UsGal,
    G,
    G100,
    Kg,
    Ml,
    Ml100,
    L,
    ImperialFloz,
    ImperialPint,
    ImperialGal,
}

pub struct UnitInfo {
    /// Stored in the database, so never reuse or change one.
    pub id: i64,
    pub families: &'static [UnitFamily],
    pub quantity_type: QuantityType,
    pub symbol: &'static str,
    pub name: &'static str,
    pub max_decimals: usize,
    pub to_base: f64,
    /// Only used as a unit price denominator ("per 100 g"), never for entering a pack size.
    pub display_only: bool,
}

use QuantityType::{Item, Volume, Weight};
use UnitFamily::{Imperial, Metric, UsCustomary};

const ITEM_FAMILY: &[UnitFamily] = &[UnitFamily::Item];
const METRIC: &[UnitFamily] = &[Metric];
const IMPERIAL: &[UnitFamily] = &[Imperial];
const US: &[UnitFamily] = &[UsCustomary];
const IMPERIAL_OR_US: &[UnitFamily] = &[Imperial, UsCustomary];

impl Unit {
    /// Listed in the order they appear in unit pickers.
    pub const ALL: [Unit; 17] = [
        Unit::Each,
        Unit::Each10,
        Unit::Each100,
        Unit::Oz,
        Unit::Lb,
        Unit::UsFloz,
        Unit::UsPint,
        Unit::UsGal,
        Unit::G,
        Unit::G100,
        Unit::Kg,
        Unit::Ml,
        Unit::Ml100,
        Unit::L,
        Unit::ImperialFloz,
        Unit::ImperialPint,
        Unit::ImperialGal,
    ];

    pub fn info(self) -> UnitInfo {
        let (id, families, quantity_type, symbol, name, max_decimals, to_base, display_only) = match self {
            Unit::Each => (101, ITEM_FAMILY, Item, "each", "each", 0, 1.0, false),
            Unit::Each10 => (102, ITEM_FAMILY, Item, "10", "10", 1, 10.0, true),
            Unit::Each100 => (103, ITEM_FAMILY, Item, "100", "100", 2, 100.0, true),
            Unit::G => (201, METRIC, Weight, "g", "gram", 0, 1.0, false),
            Unit::G100 => (202, METRIC, Weight, "100 g", "100 grams", 2, 100.0, true),
            Unit::Kg => (203, METRIC, Weight, "kg", "kilogram", 3, 1000.0, false),
            Unit::Oz => (211, IMPERIAL_OR_US, Weight, "oz", "ounce", 3, 28.349523125, false),
            Unit::Lb => (212, IMPERIAL_OR_US, Weight, "lb", "pound", 3, 453.59237, false),
            Unit::Ml => (301, METRIC, Volume, "ml", "millilitre", 0, 1.0, false),
            Unit::Ml100 => (302, METRIC, Volume, "100 ml", "100 millilitres", 2, 100.0, true),
            Unit::L => (303, METRIC, Volume, "L", "litre", 3, 1000.0, false),
            Unit::ImperialFloz => (311, IMPERIAL, Volume, "fl oz", "fluid ounce", 3, 28.4130625, false),
            Unit::ImperialPint => (312, IMPERIAL, Volume, "pt", "pint", 3, 568.26125, false),
            Unit::ImperialGal => (313, IMPERIAL, Volume, "gal", "gallon", 3, 4546.09, false),
            Unit::UsFloz => (321, US, Volume, "fl oz", "fluid ounce", 3, 29.5735295625, false),
            Unit::UsPint => (322, US, Volume, "pt", "pint", 3, 473.176473, false),
            Unit::UsGal => (323, US, Volume, "gal", "gallon", 3, 3785.411784, false),
        };
        UnitInfo { id, families, quantity_type, symbol, name, max_decimals, to_base, display_only }
    }

    pub fn id(self) -> i64 {
        self.info().id
    }

    pub fn from_id(id: i64) -> Option<Unit> {
        Unit::ALL.into_iter().find(|unit| unit.id() == id)
    }

    pub fn quantity_type(self) -> QuantityType {
        self.info().quantity_type
    }

    pub fn symbol(self) -> &'static str {
        self.info().symbol
    }

    pub fn belongs_to(self, family: UnitFamily) -> bool {
        self.info().families.contains(&family)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Quantity {
    pub value: f64,
    pub unit: Unit,
}

impl Quantity {
    pub fn new(value: f64, unit: Unit) -> Self {
        Quantity { value, unit }
    }

    /// Panics if the units measure different things (weight vs volume), which is a programming error.
    pub fn to(self, unit: Unit) -> Quantity {
        assert_eq!(self.unit.quantity_type(), unit.quantity_type(), "cannot convert {:?} to {:?}", self.unit, unit);
        Quantity::new(self.value * self.unit.info().to_base / unit.info().to_base, unit)
    }

    pub fn in_base_unit(self) -> f64 {
        self.to(self.unit.quantity_type().base_unit()).value
    }

    /// "500 g", "1.5 L", or just "6" for items.
    pub fn display(self) -> String {
        let number = format_number(self.value, self.unit.info().max_decimals);
        match self.unit {
            Unit::Each => number,
            unit => format!("{number} {}", unit.symbol()),
        }
    }
}

/// Formats with up to `max_decimals` places, dropping trailing zeros: 1.500 shows as "1.5".
pub fn format_number(value: f64, max_decimals: usize) -> String {
    let text = format!("{value:.max_decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    }
}

/// Price per one `denominator`, given a price per one base unit.
pub fn unit_price_in(price_per_base_unit: f64, denominator: Unit) -> f64 {
    price_per_base_unit * denominator.info().to_base
}

/// Picks the unit price denominator that shows the prices with about three significant figures,
/// e.g. "$0.25/oz" rather than "$0.02/g" or "$397.00/kg".
pub fn friendly_denominator(candidates: &[Unit], prices_per_base_unit: &[f64], currency_decimals: u32) -> Option<Unit> {
    if prices_per_base_unit.is_empty() {
        return None;
    }
    let mut best: Option<(f64, Unit)> = None;
    for &candidate in candidates {
        let score = prices_per_base_unit
            .iter()
            .map(|&price| score_unit_price(unit_price_in(price, candidate), currency_decimals))
            .fold(f64::MIN, f64::max);
        if best.is_none_or(|(best_score, _)| score < best_score) {
            best = Some((score, candidate));
        }
    }
    best.map(|(_, unit)| unit)
}

/// Lower is better. Too few significant figures is penalised more than too many digits.
fn score_unit_price(unit_price: f64, currency_decimals: u32) -> f64 {
    let numerator = unit_price.max(0.0001);
    let digits_before_decimal_point = numerator.log10().floor() + 1.0;
    let effective_significant_figures = digits_before_decimal_point + currency_decimals as f64;
    let delta = effective_significant_figures - 3.0;
    if delta > 0.0 { delta * 0.5 } else { -delta }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_round_trip() {
        for unit in Unit::ALL {
            assert_eq!(Unit::from_id(unit.id()), Some(unit));
        }
    }

    #[test]
    fn converts_pounds_to_ounces() {
        let quantity = Quantity::new(1.0, Unit::Lb).to(Unit::Oz);
        assert!((quantity.value - 16.0).abs() < 1e-9);
    }

    #[test]
    fn us_gallon_is_128_fluid_ounces() {
        let quantity = Quantity::new(1.0, Unit::UsGal).to(Unit::UsFloz);
        assert!((quantity.value - 128.0).abs() < 1e-9);
    }

    #[test]
    fn formats_without_trailing_zeros() {
        assert_eq!(Quantity::new(1.5, Unit::L).display(), "1.5 L");
        assert_eq!(Quantity::new(6.0, Unit::Each).display(), "6");
        assert_eq!(format_number(2.0, 3), "2");
    }

    #[test]
    fn friendly_denominator_keeps_three_significant_figures() {
        // $4.49 for 16 oz of butter shows as $4.49/lb rather than $0.28/oz.
        let butter_per_gram = 4.49 / Quantity::new(16.0, Unit::Oz).in_base_unit();
        assert_eq!(friendly_denominator(&[Unit::Oz, Unit::Lb], &[butter_per_gram], 2), Some(Unit::Lb));

        // $29.99 for 1 oz of saffron shows per ounce, not $479.84/lb.
        let spice_per_gram = 29.99 / Quantity::new(1.0, Unit::Oz).in_base_unit();
        assert_eq!(friendly_denominator(&[Unit::Oz, Unit::Lb], &[spice_per_gram], 2), Some(Unit::Oz));
    }
}
