//! Converts an amount, or a price per unit, between units of the same kind.

use egui::{RichText, Ui};

use crate::money::{Currency, parse_number};
use crate::units::{QuantityType, Quantity, Unit, format_number};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Amount,
    UnitPrice,
}

pub struct Converter {
    mode: Mode,
    value: String,
    unit: Unit,
}

impl Default for Converter {
    fn default() -> Self {
        Converter { mode: Mode::Amount, value: "1".into(), unit: Unit::Lb }
    }
}

/// Every weight and volume unit, including US and UK pints side by side.
fn convertible_units() -> impl Iterator<Item = Unit> {
    Unit::ALL
        .into_iter()
        .filter(|unit| unit.quantity_type() != QuantityType::Item && !unit.info().display_only)
}

/// Spells out US and UK units, which share symbols.
fn label(unit: Unit) -> String {
    match unit {
        Unit::UsFloz => "US fl oz".into(),
        Unit::UsPint => "US pint".into(),
        Unit::UsGal => "US gallon".into(),
        Unit::ImperialFloz => "UK fl oz".into(),
        Unit::ImperialPint => "UK pint".into(),
        Unit::ImperialGal => "UK gallon".into(),
        unit => format!("{} ({})", unit.symbol(), unit.info().name),
    }
}

/// A price per unit converts the opposite way to an amount: 1 lb is 16 oz, but $4.80/lb is $0.30/oz.
fn convert(mode: Mode, value: f64, from: Unit, to: Unit) -> f64 {
    match mode {
        Mode::Amount => Quantity::new(value, from).to(to).value,
        Mode::UnitPrice => value / Quantity::new(1.0, from).to(to).value,
    }
}

impl Converter {
    /// Returns true when the user closes it.
    pub fn show(&mut self, ui: &mut Ui, currency: &Currency) -> bool {
        ui.heading("Unit converter");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, Mode::Amount, "Amount");
            ui.selectable_value(&mut self.mode, Mode::UnitPrice, "Price per unit");
        });
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if self.mode == Mode::UnitPrice {
                ui.label(currency.symbol.trim());
            }
            ui.add(egui::TextEdit::singleline(&mut self.value).desired_width(120.0).char_limit(12));
            if self.mode == Mode::UnitPrice {
                ui.label("per");
            }
            egui::ComboBox::from_id_salt("converter_unit")
                .selected_text(label(self.unit))
                .height(500.0)
                .show_ui(ui, |ui| {
                    for unit in convertible_units() {
                        ui.selectable_value(&mut self.unit, unit, label(unit));
                    }
                });
        });
        ui.add_space(8.0);

        match parse_number(&self.value).filter(|value| *value >= 0.0) {
            None => {
                ui.weak("Enter a number.");
            }
            Some(value) => {
                egui::Grid::new("conversions").num_columns(2).spacing([24.0, 10.0]).striped(true).show(ui, |ui| {
                    for unit in convertible_units().filter(|unit| unit.quantity_type() == self.unit.quantity_type()) {
                        if unit == self.unit {
                            continue;
                        }
                        let converted = format_number(convert(self.mode, value, self.unit, unit), 4);
                        let result = match self.mode {
                            Mode::Amount => converted,
                            Mode::UnitPrice => format!("{}{converted}", currency.symbol),
                        };
                        ui.label(RichText::new(result).strong());
                        ui.label(if self.mode == Mode::UnitPrice { format!("per {}", label(unit)) } else { label(unit) });
                        ui.end_row();
                    }
                });
            }
        }

        ui.add_space(8.0);
        ui.separator();
        let mut close = false;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.button("Close").clicked();
            });
        });
        close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_amounts_and_unit_prices() {
        assert!((convert(Mode::Amount, 1.0, Unit::Lb, Unit::Oz) - 16.0).abs() < 1e-9);
        assert!((convert(Mode::UnitPrice, 4.80, Unit::Lb, Unit::Oz) - 0.30).abs() < 1e-9);
        assert!((convert(Mode::Amount, 1.0, Unit::UsGal, Unit::L) - 3.785411784).abs() < 1e-9);
    }
}
