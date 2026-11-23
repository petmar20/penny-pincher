//! Editing forms shown in dialogs. Each form edits a draft and reports what the user chose;
//! the app does the database work.

use egui::{Color32, RichText, Ui};

use crate::analysis::{self, AgeSettings, ComparedPrice, Judgement, Thresholds};
use crate::model::{DataSet, Item, LoyaltyType, Price, Source, now_millis};
use crate::money::{self, CURRENCIES, parse_number};
use crate::units::{self, QuantityType, Quantity, Unit, format_number};

pub const NAME_LIMIT: usize = 100;
pub const NOTES_LIMIT: usize = 1000;

pub enum Outcome {
    Open,
    Cancel,
    Save,
    Delete,
}

/// Save and Cancel along the bottom, plus Delete for existing records.
fn buttons(ui: &mut Ui, can_delete: bool, delete_label: &str) -> Outcome {
    let mut outcome = Outcome::Open;
    ui.add_space(8.0);
    ui.separator();
    ui.horizontal(|ui| {
        if can_delete && ui.button(delete_label).clicked() {
            outcome = Outcome::Delete;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Save").clicked() || ui.input(|input| input.key_pressed(egui::Key::Enter) && input.modifiers.ctrl) {
                outcome = Outcome::Save;
            }
            if ui.button("Cancel").clicked() || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                outcome = Outcome::Cancel;
            }
        });
    });
    outcome
}

fn error_text(ui: &mut Ui, error: &Option<String>) {
    if let Some(error) = error {
        ui.colored_label(error_color(ui), error);
    }
}

pub fn error_color(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode { Color32::from_rgb(255, 120, 110) } else { Color32::from_rgb(190, 30, 30) }
}

fn name_field(ui: &mut Ui, name: &mut String) {
    ui.add(egui::TextEdit::singleline(name).char_limit(NAME_LIMIT).desired_width(320.0));
}

fn notes_field(ui: &mut Ui, notes: &mut String) {
    ui.add(egui::TextEdit::multiline(notes).char_limit(NOTES_LIMIT).desired_rows(2).desired_width(320.0));
}

fn is_name_taken<'a>(name: &str, id: i64, others: impl IntoIterator<Item = (i64, &'a str)>) -> bool {
    others
        .into_iter()
        .any(|(other_id, other_name)| other_id != id && other_name.trim().eq_ignore_ascii_case(name.trim()))
}

// Collections

pub struct DataSetForm {
    pub draft: DataSet,
    pub error: Option<String>,
}

impl DataSetForm {
    pub fn new(data_set: DataSet) -> Self {
        DataSetForm { draft: data_set, error: None }
    }

    pub fn show(&mut self, ui: &mut Ui, others: &[DataSet]) -> Outcome {
        let is_new = self.draft.id == 0;
        ui.heading(if is_new { "New collection" } else { "Edit collection" });
        ui.add_space(6.0);
        egui::Grid::new("data_set_form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Name");
            name_field(ui, &mut self.draft.name);
            ui.end_row();

            ui.label("Currency");
            egui::ComboBox::from_id_salt("currency")
                .selected_text(self.draft.currency_code.as_str())
                .show_ui(ui, |ui| {
                    for currency in CURRENCIES {
                        ui.selectable_value(&mut self.draft.currency_code, currency.code.to_string(), currency.code);
                    }
                });
            ui.end_row();

            ui.label("Units");
            ui.vertical(|ui| {
                ui.checkbox(&mut self.draft.allow_us_customary, "US (oz, lb, fl oz, gal)");
                ui.checkbox(&mut self.draft.allow_metric, "Metric (g, kg, ml, L)");
                ui.checkbox(&mut self.draft.allow_imperial, "Imperial (UK pints and gallons)");
            });
            ui.end_row();

            ui.label("Notes");
            notes_field(ui, &mut self.draft.notes);
            ui.end_row();
        });
        error_text(ui, &self.error);

        let outcome = buttons(ui, !is_new, "Delete collection");
        if matches!(outcome, Outcome::Save) {
            self.error = self.validate(others);
            if self.error.is_some() {
                return Outcome::Open;
            }
            self.draft.name = self.draft.name.trim().to_string();
        }
        outcome
    }

    fn validate(&self, others: &[DataSet]) -> Option<String> {
        let draft = &self.draft;
        if draft.name.trim().is_empty() {
            return Some("Enter a name.".into());
        }
        if is_name_taken(&draft.name, draft.id, others.iter().map(|other| (other.id, other.name.as_str()))) {
            return Some("There's already a collection with this name.".into());
        }
        if !draft.allow_metric && !draft.allow_imperial && !draft.allow_us_customary {
            return Some("Pick at least one kind of unit.".into());
        }
        if draft.allow_imperial && draft.allow_us_customary {
            return Some("US and imperial units can't be used together.".into());
        }
        None
    }
}

// Products

pub struct ItemForm {
    pub draft: Item,
    has_prices: bool,
    pub error: Option<String>,
}

impl ItemForm {
    pub fn new(item: Item, has_prices: bool) -> Self {
        ItemForm { draft: item, has_prices, error: None }
    }

    pub fn show(&mut self, ui: &mut Ui, data_set: &DataSet, others: &[Item]) -> Outcome {
        let is_new = self.draft.id == 0;
        ui.heading(if is_new { "New product" } else { "Edit product" });
        ui.add_space(6.0);
        egui::Grid::new("item_form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Name");
            name_field(ui, &mut self.draft.name);
            ui.end_row();

            ui.label("Sold by");
            ui.vertical(|ui| {
                ui.add_enabled_ui(!self.has_prices, |ui| {
                    ui.horizontal(|ui| {
                        let mut quantity_type = self.draft.quantity_type();
                        for option in QuantityType::ALL {
                            ui.radio_value(&mut quantity_type, option, option.label());
                        }
                        if quantity_type != self.draft.quantity_type() {
                            self.draft.default_unit = data_set.units_for(quantity_type, false)[0];
                        }
                    });
                });
                if self.has_prices {
                    ui.weak("Can't change once the product has prices.");
                }
            });
            ui.end_row();

            ui.label("Usual unit");
            unit_picker(ui, "default_unit", data_set, self.draft.quantity_type(), &mut self.draft.default_unit);
            ui.end_row();

            ui.label("");
            ui.checkbox(&mut self.draft.allow_multipack, "Sometimes sold in multipacks");
            ui.end_row();

            ui.label("Notes");
            notes_field(ui, &mut self.draft.notes);
            ui.end_row();
        });
        error_text(ui, &self.error);

        let outcome = buttons(ui, !is_new, "Delete product");
        if matches!(outcome, Outcome::Save) {
            let name = self.draft.name.trim();
            self.error = if name.is_empty() {
                Some("Enter a name.".into())
            } else if is_name_taken(name, self.draft.id, others.iter().map(|other| (other.id, other.name.as_str()))) {
                Some("There's already a product with this name.".into())
            } else {
                None
            };
            if self.error.is_some() {
                return Outcome::Open;
            }
            self.draft.name = name.to_string();
        }
        outcome
    }
}

fn unit_picker(ui: &mut Ui, id: &str, data_set: &DataSet, quantity_type: QuantityType, unit: &mut Unit) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(unit_label(*unit))
        .show_ui(ui, |ui| {
            for option in data_set.units_for(quantity_type, false) {
                ui.selectable_value(unit, option, unit_label(option));
            }
        });
}

fn unit_label(unit: Unit) -> String {
    match unit {
        Unit::Each => "items".to_string(),
        unit => format!("{} ({})", unit.symbol(), unit.info().name),
    }
}

// Stores

pub struct SourceForm {
    pub draft: Source,
    percent: String,
    pub error: Option<String>,
}

impl SourceForm {
    pub fn new(source: Source) -> Self {
        let percent = match source.loyalty_type {
            LoyaltyType::None => String::new(),
            loyalty_type => format_number(loyalty_type.percent_from_multiplier(source.loyalty_multiplier), 2),
        };
        SourceForm { draft: source, percent, error: None }
    }

    pub fn show(&mut self, ui: &mut Ui, others: &[Source]) -> Outcome {
        let is_new = self.draft.id == 0;
        ui.heading(if is_new { "New store" } else { "Edit store" });
        ui.add_space(6.0);
        egui::Grid::new("source_form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Name");
            name_field(ui, &mut self.draft.name);
            ui.end_row();

            ui.label("Loyalty scheme");
            ui.vertical(|ui| {
                for option in LoyaltyType::ALL {
                    ui.radio_value(&mut self.draft.loyalty_type, option, option.label());
                    if !option.hint().is_empty() {
                        ui.weak(option.hint());
                    }
                }
            });
            ui.end_row();

            if self.draft.loyalty_type != LoyaltyType::None {
                ui.label("Reward");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.percent).desired_width(60.0).char_limit(8));
                    ui.label("%");
                });
                ui.end_row();
            }

            ui.label("Notes");
            notes_field(ui, &mut self.draft.notes);
            ui.end_row();
        });
        error_text(ui, &self.error);

        let outcome = buttons(ui, !is_new, "Delete store");
        if matches!(outcome, Outcome::Save) {
            self.error = self.apply(others);
            if self.error.is_some() {
                return Outcome::Open;
            }
        }
        outcome
    }

    fn apply(&mut self, others: &[Source]) -> Option<String> {
        let name = self.draft.name.trim().to_string();
        if name.is_empty() {
            return Some("Enter a name.".into());
        }
        if is_name_taken(&name, self.draft.id, others.iter().map(|other| (other.id, other.name.as_str()))) {
            return Some("There's already a store with this name.".into());
        }
        let multiplier = match self.draft.loyalty_type {
            LoyaltyType::None => 1.0,
            loyalty_type => match parse_number(&self.percent) {
                Some(percent) if (0.0..100.0).contains(&percent) => loyalty_type.multiplier_from_percent(percent),
                _ => return Some("Enter a reward between 0 and 100%.".into()),
            },
        };
        self.draft.name = name;
        self.draft.loyalty_multiplier = multiplier;
        None
    }
}

// Prices

/// What the price form needs to know about the product it's pricing.
pub struct PriceContext<'a> {
    pub data_set: &'a DataSet,
    pub item: &'a Item,
    pub sources: &'a [Source],
    pub comparison: &'a [ComparedPrice],
    pub thresholds: Option<Thresholds>,
}

pub struct PriceForm {
    pub source_id: i64,
    price: String,
    count: String,
    size: String,
    unit: Unit,
    notes: String,
    seen_today: bool,
    pub error: Option<String>,
}

impl PriceForm {
    /// Starts from the store's current price when it has one, so only what changed needs typing.
    pub fn new(context: &PriceContext, source_id: i64) -> Self {
        let mut form = PriceForm {
            source_id,
            price: String::new(),
            count: String::new(),
            size: String::new(),
            unit: context.data_set.usable_unit(context.item.default_unit),
            notes: String::new(),
            seen_today: true,
            error: None,
        };
        form.fill_from_current(context);
        form
    }

    fn current<'a>(&self, context: &'a PriceContext) -> Option<&'a ComparedPrice> {
        context.comparison.iter().find(|compared| compared.price.source_id == self.source_id)
    }

    fn fill_from_current(&mut self, context: &PriceContext) {
        let Some(current) = self.current(context) else {
            self.price.clear();
            self.notes.clear();
            return;
        };
        let price = &current.price;
        let currency = context.data_set.currency();
        let unit = context.data_set.usable_unit(price.quantity.unit);
        self.price = format!("{:.*}", currency.decimals as usize, price.price);
        self.count = if price.count == 1 { String::new() } else { price.count.to_string() };
        self.size = format_number(price.quantity.to(unit).value, unit.info().max_decimals);
        self.unit = unit;
        self.notes = price.notes.clone();
    }

    /// The price as typed, if it's complete and valid.
    fn parse(&self, context: &PriceContext) -> Result<Price, String> {
        let price = parse_number(&self.price).filter(|price| *price >= 0.0).ok_or("Enter the price.")?;
        let count = if self.count.trim().is_empty() {
            1
        } else {
            match parse_number(&self.count) {
                Some(count) if count >= 1.0 && count.fract() == 0.0 => count as i64,
                _ => return Err("The number of packs must be a whole number.".into()),
            }
        };
        let size = if context.item.quantity_type() == QuantityType::Item && self.size.trim().is_empty() {
            1.0
        } else {
            parse_number(&self.size).filter(|size| *size > 0.0).ok_or("Enter the size.")?
        };
        let now = now_millis();
        let confirmed_at = match self.current(context) {
            Some(current) if !self.seen_today => current.price.confirmed_at,
            _ => now,
        };
        Ok(Price {
            id: 0,
            item_id: context.item.id,
            source_id: self.source_id,
            price,
            count,
            quantity: Quantity::new(size, self.unit),
            confirmed_at,
            notes: self.notes.trim().to_string(),
            modified_at: now,
        })
    }

    pub fn show(&mut self, ui: &mut Ui, context: &PriceContext) -> (Outcome, Option<Price>) {
        let currency = context.data_set.currency();
        ui.heading(format!("Price of {}", context.item.name));
        ui.add_space(6.0);

        egui::Grid::new("price_form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Store");
            let previous_source = self.source_id;
            let source_name = |id: i64| context.sources.iter().find(|source| source.id == id).map_or("", |source| source.name.as_str());
            egui::ComboBox::from_id_salt("price_source")
                .selected_text(source_name(self.source_id))
                .show_ui(ui, |ui| {
                    for source in context.sources {
                        ui.selectable_value(&mut self.source_id, source.id, &source.name);
                    }
                });
            if self.source_id != previous_source {
                self.fill_from_current(context);
            }
            ui.end_row();

            ui.label("Price");
            let price_field = ui.add(egui::TextEdit::singleline(&mut self.price).desired_width(120.0).char_limit(12).hint_text(currency.symbol.trim()));
            if ui.memory(|memory| memory.focused().is_none()) {
                price_field.request_focus();
            }
            ui.end_row();

            if context.item.allow_multipack {
                ui.label("Packs");
                ui.add(egui::TextEdit::singleline(&mut self.count).desired_width(120.0).char_limit(6).hint_text("1"));
                ui.end_row();
            }

            ui.label(if context.item.allow_multipack { "Size of each" } else { "Size" });
            ui.horizontal(|ui| {
                let hint = if context.item.quantity_type() == QuantityType::Item { "1" } else { "" };
                ui.add(egui::TextEdit::singleline(&mut self.size).desired_width(120.0).char_limit(12).hint_text(hint));
                unit_picker(ui, "price_unit", context.data_set, context.item.quantity_type(), &mut self.unit);
            });
            ui.end_row();

            ui.label("Notes");
            notes_field(ui, &mut self.notes);
            ui.end_row();

            if self.current(context).is_some() {
                ui.label("");
                ui.checkbox(&mut self.seen_today, "I saw this price today")
                    .on_hover_text("Untick this to fix a typo without changing when the price was last seen.");
                ui.end_row();
            }
        });

        ui.add_space(6.0);
        let parsed = self.parse(context);
        if let Ok(price) = &parsed {
            self.show_verdict(ui, context, price);
        }
        error_text(ui, &self.error);

        let outcome = buttons(ui, false, "");
        match outcome {
            Outcome::Save => match parsed {
                Ok(price) => (Outcome::Save, Some(price)),
                Err(error) => {
                    self.error = Some(error);
                    (Outcome::Open, None)
                }
            },
            outcome => (outcome, None),
        }
    }

    /// The new unit price, and how it compares with the other stores.
    fn show_verdict(&self, ui: &mut Ui, context: &PriceContext, price: &Price) {
        let currency = context.data_set.currency();
        let Some(source) = context.sources.iter().find(|source| source.id == self.source_id) else { return };
        let effective = price.price * source.loyalty_multiplier;
        let per_base_unit = price.price_per_base_unit(effective);

        let mut all_prices: Vec<f64> = context
            .comparison
            .iter()
            .map(|compared| compared.adjusted_price_per_base_unit)
            .collect();
        all_prices.push(per_base_unit);
        let candidates = context.data_set.units_like(self.unit);
        let denominator = units::friendly_denominator(&candidates, &all_prices, currency.decimals).unwrap_or(self.unit);

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label("Unit price");
                ui.label(RichText::new(format_unit_price(currency, per_base_unit, denominator)).strong());
                let judgement = analysis::judge(per_base_unit, context.thresholds);
                judgement_badge(ui, judgement);
            });
            if let Some(current) = self.current(context) {
                ui.label(format!(
                    "Before: {}",
                    format_unit_price(currency, current.adjusted_price_per_base_unit, denominator)
                ));
            }
            let cheapest_elsewhere = context
                .comparison
                .iter()
                .find(|compared| compared.price.source_id != self.source_id && compared.age_class != analysis::AgeClass::Ancient);
            if let Some(cheapest) = cheapest_elsewhere {
                let difference = per_base_unit / cheapest.adjusted_price_per_base_unit - 1.0;
                let comparison = if difference.abs() < 0.005 {
                    format!("Same as {}", cheapest.source_name)
                } else if difference < 0.0 {
                    format!("{:.0}% cheaper than {}", -difference * 100.0, cheapest.source_name)
                } else {
                    format!("{:.0}% more than {}", difference * 100.0, cheapest.source_name)
                };
                ui.label(format!(
                    "{comparison} ({})",
                    format_unit_price(currency, cheapest.adjusted_price_per_base_unit, denominator)
                ));
            }
            if source.loyalty_type != LoyaltyType::None {
                ui.weak("Includes this store's loyalty reward.");
            }
        });
    }
}

pub fn format_unit_price(currency: &money::Currency, per_base_unit: f64, denominator: Unit) -> String {
    let amount = currency.format(units::unit_price_in(per_base_unit, denominator));
    match denominator {
        Unit::Each => format!("{amount} each"),
        unit => format!("{amount}/{}", unit.symbol()),
    }
}

pub fn judgement_badge(ui: &mut Ui, judgement: Judgement) {
    let dark = ui.visuals().dark_mode;
    let (text, color) = match judgement {
        Judgement::None => return,
        Judgement::Good => ("Good price", if dark { Color32::from_rgb(110, 210, 120) } else { Color32::from_rgb(20, 130, 40) }),
        Judgement::Ok => ("OK price", if dark { Color32::from_rgb(230, 190, 90) } else { Color32::from_rgb(150, 100, 0) }),
        Judgement::Bad => ("Bad price", error_color(ui)),
    };
    ui.label(RichText::new(text).color(color).strong());
}

// Settings

pub struct SettingsForm {
    stale: String,
    ancient: String,
    inflation: String,
    pub error: Option<String>,
}

impl SettingsForm {
    pub fn new(settings: &AgeSettings) -> Self {
        SettingsForm {
            stale: settings.stale_after_days.to_string(),
            ancient: settings.ancient_after_days.to_string(),
            inflation: format_number(settings.annual_inflation_percent, 2),
            error: None,
        }
    }

    pub fn show(&mut self, ui: &mut Ui) -> (Outcome, Option<AgeSettings>) {
        ui.heading("Settings");
        ui.add_space(6.0);
        let setting = |ui: &mut Ui, label: &str, hint: &str, value: &mut String, suffix: &str| {
            ui.label(RichText::new(label).strong());
            ui.weak(hint);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(value).desired_width(80.0).char_limit(6));
                ui.label(suffix);
            });
            ui.add_space(6.0);
        };
        setting(ui, "Old prices", "Prices you haven't seen for this long get an inflation adjustment.", &mut self.stale, "days");
        setting(ui, "Very old prices", "Prices this old aren't used to rate the others.", &mut self.ancient, "days");
        setting(ui, "Inflation", "How much old prices go up each year.", &mut self.inflation, "% a year");
        error_text(ui, &self.error);

        match buttons(ui, false, "") {
            Outcome::Save => match self.parse() {
                Ok(settings) => (Outcome::Save, Some(settings)),
                Err(error) => {
                    self.error = Some(error);
                    (Outcome::Open, None)
                }
            },
            outcome => (outcome, None),
        }
    }

    fn parse(&self) -> Result<AgeSettings, String> {
        let whole_days = |text: &str| parse_number(text).filter(|days| *days >= 1.0 && days.fract() == 0.0).map(|days| days as i64);
        let stale = whole_days(&self.stale).ok_or("Old prices needs a whole number of days.")?;
        let ancient = whole_days(&self.ancient).ok_or("Very old prices needs a whole number of days.")?;
        if ancient <= stale {
            return Err("Very old prices must be older than old prices.".into());
        }
        let inflation = parse_number(&self.inflation)
            .filter(|percent| (0.0..=100.0).contains(percent))
            .ok_or("Inflation must be between 0 and 100%.")?;
        Ok(AgeSettings { stale_after_days: stale, ancient_after_days: ancient, annual_inflation_percent: inflation })
    }
}
