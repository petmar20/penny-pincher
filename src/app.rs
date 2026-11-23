//! The main window.

use std::path::PathBuf;

use anyhow::Result;
use egui::{RichText, Ui};

use crate::analysis::{self, AgeClass, AgeSettings, ComparedPrice, Thresholds};
use crate::db::{self, Db};
use crate::forms::{
    self, DataSetForm, ItemForm, Outcome, PriceContext, PriceForm, SettingsForm, SourceForm, format_unit_price,
    judgement_badge,
};
use crate::model::{DataSet, Item, LoyaltyType, Price, Source, now_millis};
use crate::units::{self, Unit};

pub struct PennyApp {
    db: Db,
    age_settings: AgeSettings,
    data_sets: Vec<DataSet>,
    data_set_id: Option<i64>,
    items: Vec<Item>,
    sources: Vec<Source>,
    item_id: Option<i64>,
    search: String,
    /// Current prices for the selected product, cheapest first.
    comparison: Vec<ComparedPrice>,
    thresholds: Option<Thresholds>,
    unit_price_denominator: Option<Unit>,
    dialog: Option<Dialog>,
    notice: Option<Notice>,
}

enum Dialog {
    DataSet(DataSetForm),
    Item(ItemForm),
    Source(SourceForm),
    Price(PriceForm),
    History { item_id: i64, source_id: i64 },
    Stores,
    Settings(SettingsForm),
    Confirm(Confirm),
    About,
}

struct Confirm {
    question: String,
    detail: Option<String>,
    button: &'static str,
    action: ConfirmAction,
}

enum ConfirmAction {
    DeleteDataSet(i64),
    DeleteItem(i64),
    DeleteSource(i64),
    DeletePrice { id: i64, item_id: i64, source_id: i64 },
    DeletePriceHistory { item_id: i64, source_id: i64 },
    Restore(PathBuf),
}

struct Notice {
    text: String,
    is_error: bool,
    undo_price_id: Option<i64>,
}

impl PennyApp {
    pub fn new() -> Result<Self> {
        let db = Db::open(&db::default_path())?;
        if db.data_sets()?.is_empty() {
            let mut groceries = DataSet::new_default();
            groceries.name = "Groceries".into();
            db.save_data_set(&groceries)?;
        }
        let saved_id = |key: &str| -> Result<Option<i64>> { Ok(db.setting(key)?.and_then(|value| value.parse().ok())) };
        let data_set_id = saved_id("data_set_id")?;
        let item_id = saved_id("item_id")?;

        let mut app = PennyApp {
            age_settings: db.age_settings()?,
            db,
            data_sets: Vec::new(),
            data_set_id,
            items: Vec::new(),
            sources: Vec::new(),
            item_id,
            search: String::new(),
            comparison: Vec::new(),
            thresholds: None,
            unit_price_denominator: None,
            dialog: None,
            notice: None,
        };
        app.reload()?;
        Ok(app)
    }

    /// Re-reads everything shown on screen. The data is small enough that this is instant.
    fn reload(&mut self) -> Result<()> {
        self.data_sets = self.db.data_sets()?;
        if !self.data_sets.iter().any(|data_set| Some(data_set.id) == self.data_set_id) {
            self.data_set_id = self.data_sets.first().map(|data_set| data_set.id);
        }

        self.items.clear();
        self.sources.clear();
        if let Some(data_set_id) = self.data_set_id {
            self.items = self.db.items(data_set_id)?;
            self.sources = self.db.sources(data_set_id)?;
        }
        if !self.items.iter().any(|item| Some(item.id) == self.item_id) {
            self.item_id = None;
        }

        self.comparison.clear();
        self.thresholds = None;
        self.unit_price_denominator = None;
        if let (Some(item), Some(data_set)) = (self.item().cloned(), self.data_set().cloned()) {
            let prices = self.db.current_prices(item.id)?;
            let (comparison, thresholds) = analysis::compare(&prices, &self.sources, &self.age_settings, now_millis());
            let per_base_unit: Vec<f64> = comparison.iter().map(|compared| compared.adjusted_price_per_base_unit).collect();
            let candidates = data_set.units_like(data_set.usable_unit(item.default_unit));
            self.unit_price_denominator = units::friendly_denominator(&candidates, &per_base_unit, data_set.currency().decimals);
            self.comparison = comparison;
            self.thresholds = thresholds;
        }

        let id_text = |id: Option<i64>| id.map(|id| id.to_string()).unwrap_or_default();
        self.db.set_setting("data_set_id", &id_text(self.data_set_id))?;
        self.db.set_setting("item_id", &id_text(self.item_id))?;
        Ok(())
    }

    fn data_set(&self) -> Option<&DataSet> {
        self.data_sets.iter().find(|data_set| Some(data_set.id) == self.data_set_id)
    }

    fn item(&self) -> Option<&Item> {
        self.items.iter().find(|item| Some(item.id) == self.item_id)
    }

    /// Runs a database change, then refreshes the screen. Errors show in the status bar.
    fn change(&mut self, action: impl FnOnce(&mut Self) -> Result<()>) {
        let result = action(self).and_then(|()| self.reload());
        if let Err(error) = result {
            self.notice = Some(Notice { text: format!("{error:#}"), is_error: true, undo_price_id: None });
        }
    }

    fn say(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice { text: text.into(), is_error: false, undo_price_id: None });
    }

    fn price_context(&self) -> Option<PriceContext<'_>> {
        Some(PriceContext {
            data_set: self.data_set()?,
            item: self.item()?,
            sources: &self.sources,
            comparison: &self.comparison,
            thresholds: self.thresholds,
        })
    }

    fn open_price_form(&mut self, source_id: i64) {
        if let Some(context) = self.price_context() {
            self.dialog = Some(Dialog::Price(PriceForm::new(&context, source_id)));
        }
    }

    fn new_item(&self, data_set: &DataSet) -> Item {
        Item {
            id: 0,
            data_set_id: data_set.id,
            name: String::new(),
            default_unit: data_set.units_for(units::QuantityType::Weight, false)[0],
            allow_multipack: false,
            notes: String::new(),
        }
    }

    fn new_source(&self, data_set_id: i64) -> Source {
        Source {
            id: 0,
            data_set_id,
            name: String::new(),
            loyalty_type: LoyaltyType::None,
            loyalty_multiplier: 1.0,
            notes: String::new(),
        }
    }

    // Layout

    fn menu_bar(&mut self, ui: &mut Ui) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Back up…").clicked() {
                        ui.close();
                        self.back_up();
                    }
                    if ui.button("Restore…").clicked() {
                        ui.close();
                        self.choose_restore_file();
                    }
                    ui.separator();
                    if ui.button("Settings…").clicked() {
                        ui.close();
                        self.dialog = Some(Dialog::Settings(SettingsForm::new(&self.age_settings)));
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Collection", |ui| {
                    if ui.button("New collection…").clicked() {
                        ui.close();
                        self.dialog = Some(Dialog::DataSet(DataSetForm::new(DataSet::new_default())));
                    }
                    if let Some(data_set) = self.data_set().cloned()
                        && ui.button("Edit collection…").clicked()
                    {
                        ui.close();
                        self.dialog = Some(Dialog::DataSet(DataSetForm::new(data_set)));
                    }
                });
                ui.menu_button("Stores", |ui| {
                    if ui.button("Manage stores…").clicked() {
                        ui.close();
                        self.dialog = Some(Dialog::Stores);
                    }
                    if let Some(data_set_id) = self.data_set_id
                        && ui.button("New store…").clicked()
                    {
                        ui.close();
                        self.dialog = Some(Dialog::Source(SourceForm::new(self.new_source(data_set_id))));
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About Penny Pincher").clicked() {
                        ui.close();
                        self.dialog = Some(Dialog::About);
                    }
                });

                ui.separator();
                if self.data_sets.len() > 1 {
                    let mut selected = self.data_set_id;
                    let name = self.data_set().map(|data_set| data_set.name.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("data_set").selected_text(name).show_ui(ui, |ui| {
                        for data_set in &self.data_sets {
                            ui.selectable_value(&mut selected, Some(data_set.id), &data_set.name);
                        }
                    });
                    if selected != self.data_set_id {
                        self.data_set_id = selected;
                        self.item_id = None;
                        self.change(|_| Ok(()));
                    }
                } else if let Some(data_set) = self.data_set() {
                    ui.label(RichText::new(&data_set.name).strong());
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let Some(notice) = &self.notice else { return };
        let mut undo = None;
        let mut dismiss = false;
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if notice.is_error {
                    ui.colored_label(forms::error_color(ui), &notice.text);
                } else {
                    ui.label(&notice.text);
                }
                if let Some(id) = notice.undo_price_id
                    && ui.button("Undo").clicked()
                {
                    undo = Some(id);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    dismiss = ui.small_button("✕").clicked();
                });
            });
        });
        if dismiss {
            self.notice = None;
        }
        if let Some(id) = undo {
            self.change(|app| app.db.delete_price(id));
            self.say("Undone.");
        }
    }

    fn products_panel(&mut self, ui: &mut Ui) {
        let Some(data_set) = self.data_set().cloned() else { return };
        let mut clicked = None;
        egui::Panel::left("products").resizable(true).default_size(240.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Products");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Add").clicked() {
                        self.dialog = Some(Dialog::Item(ItemForm::new(self.new_item(&data_set), false)));
                    }
                });
            });
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(f32::INFINITY));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let search = self.search.trim().to_lowercase();
                for item in self.items.iter().filter(|item| item.name.to_lowercase().contains(&search)) {
                    let selected = Some(item.id) == self.item_id;
                    let label = egui::Button::selectable(selected, &item.name).min_size(egui::vec2(ui.available_width(), 0.0));
                    if ui.add(label).clicked() {
                        clicked = Some(item.id);
                    }
                }
            });
        });
        if let Some(item_id) = clicked {
            self.item_id = Some(item_id);
            self.change(|_| Ok(()));
        }
    }

    fn central_panel(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let Some(data_set) = self.data_set().cloned() else {
                ui.label("Add a collection to get started.");
                if ui.button("New collection").clicked() {
                    self.dialog = Some(Dialog::DataSet(DataSetForm::new(DataSet::new_default())));
                }
                return;
            };
            let Some(item) = self.item().cloned() else {
                if self.items.is_empty() {
                    ui.label("Add the things you buy, like butter or milk.");
                    if ui.button("Add product").clicked() {
                        self.dialog = Some(Dialog::Item(ItemForm::new(self.new_item(&data_set), false)));
                    }
                } else {
                    ui.label("Pick a product on the left.");
                }
                return;
            };
            self.product_view(ui, &data_set, &item);
        });
    }

    fn product_view(&mut self, ui: &mut Ui, data_set: &DataSet, item: &Item) {
        ui.horizontal(|ui| {
            ui.heading(&item.name);
            ui.weak(format!("sold by {}", item.quantity_type().label().to_lowercase()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.sources.is_empty() && ui.button("Add price").clicked() {
                    let source_id = self
                        .sources
                        .iter()
                        .find(|source| !self.comparison.iter().any(|compared| compared.price.source_id == source.id))
                        .unwrap_or(&self.sources[0])
                        .id;
                    self.open_price_form(source_id);
                }
                if ui.button("Edit product").clicked() {
                    let has_prices = !self.comparison.is_empty() || self.db.count_prices_for_item(item.id).unwrap_or(0) > 0;
                    self.dialog = Some(Dialog::Item(ItemForm::new(item.clone(), has_prices)));
                }
            });
        });
        if !item.notes.is_empty() {
            ui.weak(&item.notes);
        }
        ui.add_space(8.0);

        if self.sources.is_empty() {
            ui.label("Add the stores you shop at.");
            if ui.button("Add store").clicked() {
                self.dialog = Some(Dialog::Source(SourceForm::new(self.new_source(data_set.id))));
            }
            return;
        }

        let currency = data_set.currency();
        let denominator = self.unit_price_denominator.unwrap_or(data_set.usable_unit(item.default_unit));
        let mut open_price = None;
        let mut confirm = None;
        let mut history = None;

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            egui::Grid::new("comparison").num_columns(6).spacing([18.0, 10.0]).striped(true).show(ui, |ui| {
                for header in ["Store", "Shelf price", "Unit price", "Last seen", "", ""] {
                    ui.label(RichText::new(header).strong());
                }
                ui.end_row();

                for compared in &self.comparison {
                    let price = &compared.price;
                    let store = ui.label(&compared.source_name);
                    if let Some(source) = self.sources.iter().find(|source| source.id == price.source_id)
                        && source.loyalty_type != LoyaltyType::None
                    {
                        let percent = source.loyalty_type.percent_from_multiplier(source.loyalty_multiplier);
                        store.on_hover_text(format!("{}: {}%", source.loyalty_type.label(), units::format_number(percent, 2)));
                    }

                    let shelf = ui.label(price.describe(currency));
                    if !price.notes.is_empty() {
                        shelf.on_hover_text(&price.notes);
                    }

                    let unit_price = ui.label(RichText::new(format_unit_price(currency, compared.adjusted_price_per_base_unit, denominator)).strong());
                    if compared.is_adjusted() {
                        let shelf_unit_price = format_unit_price(currency, price.price_per_base_unit(price.price), denominator);
                        unit_price.on_hover_text(format!("{shelf_unit_price} at the shelf price, adjusted for loyalty rewards and age."));
                    }

                    age_label(ui, compared, &self.age_settings);
                    judgement_badge(ui, compared.judgement);

                    ui.horizontal(|ui| {
                        if ui.button("New price").clicked() {
                            open_price = Some(price.source_id);
                        }
                        if ui.button("Still the same").on_hover_text("You saw this price today").clicked() {
                            confirm = Some(price.clone());
                        }
                        if ui.button("History").clicked() {
                            history = Some(price.source_id);
                        }
                    });
                    ui.end_row();
                }

                for source in &self.sources {
                    if self.comparison.iter().any(|compared| compared.price.source_id == source.id) {
                        continue;
                    }
                    ui.label(&source.name);
                    ui.weak("No price yet");
                    ui.label("");
                    ui.label("");
                    ui.label("");
                    if ui.button("Add price").clicked() {
                        open_price = Some(source.id);
                    }
                    ui.end_row();
                }
            });

            if self.comparison.iter().any(ComparedPrice::is_adjusted) {
                ui.add_space(8.0);
                ui.weak("Unit prices include loyalty rewards, and old prices are adjusted for inflation.");
            }
        });

        if let Some(source_id) = open_price {
            self.open_price_form(source_id);
        }
        if let Some(price) = confirm {
            let now = now_millis();
            let seen_again = Price { id: 0, confirmed_at: now, modified_at: now, ..price };
            let mut new_id = None;
            self.change(|app| {
                new_id = Some(app.db.add_price(&seen_again)?);
                Ok(())
            });
            if let Some(id) = new_id {
                self.notice = Some(Notice { text: "Marked as seen today.".into(), is_error: false, undo_price_id: Some(id) });
            }
        }
        if let Some(source_id) = history {
            self.dialog = Some(Dialog::History { item_id: item.id, source_id });
        }
    }

    // File menu

    fn back_up(&mut self) {
        let file_name = format!("penny-pincher-{}.db", chrono::Local::now().format("%Y-%m-%d"));
        let Some(path) = rfd::FileDialog::new().add_filter("Penny Pincher backup", &["db"]).set_file_name(file_name).save_file() else {
            return;
        };
        match self.db.back_up_to(&path) {
            Ok(()) => self.say(format!("Backed up to {}.", path.display())),
            Err(error) => self.notice = Some(Notice { text: format!("{error:#}"), is_error: true, undo_price_id: None }),
        }
    }

    fn choose_restore_file(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Penny Pincher backup", &["db"]).pick_file() else {
            return;
        };
        self.dialog = Some(Dialog::Confirm(Confirm {
            question: "Replace all your data with this backup?".into(),
            detail: Some(path.display().to_string()),
            button: "Restore",
            action: ConfirmAction::Restore(path),
        }));
    }

    // Dialogs

    fn show_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let keep_open = match &mut dialog {
            Dialog::DataSet(form) => self.data_set_dialog(ctx, form),
            Dialog::Item(form) => self.item_dialog(ctx, form),
            Dialog::Source(form) => self.source_dialog(ctx, form),
            Dialog::Price(form) => self.price_dialog(ctx, form),
            Dialog::History { item_id, source_id } => self.history_dialog(ctx, *item_id, *source_id),
            Dialog::Stores => self.stores_dialog(ctx),
            Dialog::Settings(form) => self.settings_dialog(ctx, form),
            Dialog::Confirm(confirm) => self.confirm_dialog(ctx, confirm),
            Dialog::About => about_dialog(ctx),
        };
        // A dialog can hand over to another one, such as Delete opening a confirmation.
        if keep_open && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }

    fn data_set_dialog(&mut self, ctx: &egui::Context, form: &mut DataSetForm) -> bool {
        let others = self.data_sets.clone();
        match modal(ctx, |ui| form.show(ui, &others)).0 {
            Outcome::Open => true,
            Outcome::Cancel => false,
            Outcome::Save => {
                let draft = form.draft.clone();
                self.change(|app| {
                    app.data_set_id = Some(app.db.save_data_set(&draft)?);
                    if draft.id == 0 {
                        app.item_id = None;
                    }
                    Ok(())
                });
                false
            }
            Outcome::Delete => {
                let has_contents = self.db.count_items_and_sources(form.draft.id).unwrap_or(1) > 0;
                self.dialog = Some(Dialog::Confirm(Confirm {
                    question: format!("Delete the {} collection?", form.draft.name),
                    detail: has_contents.then(|| "Its products, stores and prices will be deleted too.".into()),
                    button: "Delete",
                    action: ConfirmAction::DeleteDataSet(form.draft.id),
                }));
                false
            }
        }
    }

    fn item_dialog(&mut self, ctx: &egui::Context, form: &mut ItemForm) -> bool {
        let Some(data_set) = self.data_set().cloned() else { return false };
        let others = self.items.clone();
        match modal(ctx, |ui| form.show(ui, &data_set, &others)).0 {
            Outcome::Open => true,
            Outcome::Cancel => false,
            Outcome::Save => {
                let draft = form.draft.clone();
                self.change(|app| {
                    app.item_id = Some(app.db.save_item(&draft)?);
                    Ok(())
                });
                false
            }
            Outcome::Delete => {
                let has_prices = self.db.count_prices_for_item(form.draft.id).unwrap_or(1) > 0;
                self.dialog = Some(Dialog::Confirm(Confirm {
                    question: format!("Delete {}?", form.draft.name),
                    detail: has_prices.then(|| "Its prices will be deleted too.".into()),
                    button: "Delete",
                    action: ConfirmAction::DeleteItem(form.draft.id),
                }));
                false
            }
        }
    }

    fn source_dialog(&mut self, ctx: &egui::Context, form: &mut SourceForm) -> bool {
        let others = self.sources.clone();
        match modal(ctx, |ui| form.show(ui, &others)).0 {
            Outcome::Open => true,
            Outcome::Cancel => false,
            Outcome::Save => {
                let draft = form.draft.clone();
                self.change(|app| app.db.save_source(&draft).map(|_| ()));
                false
            }
            Outcome::Delete => {
                let has_prices = self.db.count_prices_for_source(form.draft.id).unwrap_or(1) > 0;
                self.dialog = Some(Dialog::Confirm(Confirm {
                    question: format!("Delete {}?", form.draft.name),
                    detail: has_prices.then(|| "Its prices will be deleted too.".into()),
                    button: "Delete",
                    action: ConfirmAction::DeleteSource(form.draft.id),
                }));
                false
            }
        }
    }

    fn price_dialog(&mut self, ctx: &egui::Context, form: &mut PriceForm) -> bool {
        let Some(context) = self.price_context() else { return false };
        let (outcome, price) = modal(ctx, |ui| form.show(ui, &context)).0;
        match (outcome, price) {
            (Outcome::Save, Some(price)) => {
                let mut new_id = None;
                self.change(|app| {
                    new_id = Some(app.db.add_price(&price)?);
                    Ok(())
                });
                if let Some(id) = new_id {
                    self.notice = Some(Notice { text: "Price saved.".into(), is_error: false, undo_price_id: Some(id) });
                }
                false
            }
            (Outcome::Cancel, _) => false,
            _ => true,
        }
    }

    fn history_dialog(&mut self, ctx: &egui::Context, item_id: i64, source_id: i64) -> bool {
        let (Some(data_set), Some(item)) = (self.data_set().cloned(), self.item().cloned()) else { return false };
        let source_name = self.sources.iter().find(|source| source.id == source_id).map(|source| source.name.clone()).unwrap_or_default();
        let entries = match self.db.price_history(item_id, source_id) {
            Ok(entries) => entries,
            Err(_) => return false,
        };
        let currency = data_set.currency();
        let denominator = self.unit_price_denominator.unwrap_or(data_set.usable_unit(item.default_unit));
        let mut delete = None;
        let mut delete_all = false;
        let mut close = false;

        let (_, should_close) = modal(ctx, |ui| {
            ui.heading(format!("{} at {}", item.name, source_name));
            ui.add_space(6.0);
            if entries.is_empty() {
                ui.label("No prices yet.");
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                egui::Grid::new("history").num_columns(5).spacing([16.0, 8.0]).striped(true).show(ui, |ui| {
                    for header in ["Seen", "Shelf price", "Unit price", "Notes", ""] {
                        ui.label(RichText::new(header).strong());
                    }
                    ui.end_row();
                    for (index, entry) in entries.iter().enumerate() {
                        let seen = chrono::DateTime::from_timestamp_millis(entry.confirmed_at)
                            .map(|time| time.with_timezone(&chrono::Local).format("%b %-d, %Y").to_string())
                            .unwrap_or_default();
                        ui.label(if index == 0 { RichText::new(seen).strong() } else { RichText::new(seen) });
                        ui.label(entry.describe(currency));
                        ui.label(format_unit_price(currency, entry.price_per_base_unit(entry.price), denominator));
                        ui.label(&entry.notes);
                        let hint = if index == 0 { "Delete this price. The one below becomes current." } else { "Delete this price" };
                        if ui.small_button("Delete").on_hover_text(hint).clicked() {
                            delete = Some(entry.id);
                        }
                        ui.end_row();
                    }
                });
            });
            ui.weak("Unit prices here are shelf prices, without loyalty rewards.");
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                if !entries.is_empty() && ui.button("Delete all").clicked() {
                    delete_all = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                });
            });
        });

        if let Some(id) = delete {
            self.dialog = Some(Dialog::Confirm(Confirm {
                question: format!("Delete this {} price?", item.name),
                detail: None,
                button: "Delete",
                action: ConfirmAction::DeletePrice { id, item_id, source_id },
            }));
            return false;
        }
        if delete_all {
            self.dialog = Some(Dialog::Confirm(Confirm {
                question: format!("Delete every {} price at {}?", item.name, source_name),
                detail: None,
                button: "Delete all",
                action: ConfirmAction::DeletePriceHistory { item_id, source_id },
            }));
            return false;
        }
        !(close || should_close)
    }

    fn stores_dialog(&mut self, ctx: &egui::Context) -> bool {
        let Some(data_set_id) = self.data_set_id else { return false };
        let mut edit = None;
        let mut add = false;
        let mut close = false;
        let (_, should_close) = modal(ctx, |ui| {
            ui.heading("Stores");
            ui.add_space(6.0);
            if self.sources.is_empty() {
                ui.label("No stores yet.");
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                egui::Grid::new("stores").num_columns(3).spacing([16.0, 8.0]).striped(true).show(ui, |ui| {
                    for source in &self.sources {
                        ui.label(&source.name);
                        match source.loyalty_type {
                            LoyaltyType::None => ui.label(""),
                            loyalty_type => {
                                let percent = loyalty_type.percent_from_multiplier(source.loyalty_multiplier);
                                ui.weak(format!("{} {}%", loyalty_type.label(), units::format_number(percent, 2)))
                            }
                        };
                        if ui.small_button("Edit").clicked() {
                            edit = Some(source.clone());
                        }
                        ui.end_row();
                    }
                });
            });
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                add = ui.button("New store").clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                });
            });
        });
        if let Some(source) = edit {
            self.dialog = Some(Dialog::Source(SourceForm::new(source)));
            return false;
        }
        if add {
            self.dialog = Some(Dialog::Source(SourceForm::new(self.new_source(data_set_id))));
            return false;
        }
        !(close || should_close)
    }

    fn settings_dialog(&mut self, ctx: &egui::Context, form: &mut SettingsForm) -> bool {
        match modal(ctx, |ui| form.show(ui)).0 {
            (Outcome::Save, Some(settings)) => {
                self.change(|app| {
                    app.db.save_age_settings(&settings)?;
                    app.age_settings = settings;
                    Ok(())
                });
                false
            }
            (Outcome::Cancel, _) => false,
            _ => true,
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context, confirm: &Confirm) -> bool {
        let mut confirmed = false;
        let mut cancelled = false;
        let (_, should_close) = modal(ctx, |ui| {
            ui.label(RichText::new(&confirm.question).strong());
            if let Some(detail) = &confirm.detail {
                ui.label(detail);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    confirmed = ui.button(confirm.button).clicked();
                    cancelled = ui.button("Cancel").clicked();
                });
            });
        });
        if !confirmed {
            return !(cancelled || should_close);
        }

        match &confirm.action {
            ConfirmAction::DeleteDataSet(id) => self.change(|app| app.db.delete_data_set(*id)),
            ConfirmAction::DeleteItem(id) => self.change(|app| app.db.delete_item(*id)),
            ConfirmAction::DeleteSource(id) => self.change(|app| app.db.delete_source(*id)),
            ConfirmAction::DeletePrice { id, item_id, source_id } => {
                self.change(|app| app.db.delete_price(*id));
                self.dialog = Some(Dialog::History { item_id: *item_id, source_id: *source_id });
            }
            ConfirmAction::DeletePriceHistory { item_id, source_id } => {
                self.change(|app| app.db.delete_price_history(*item_id, *source_id));
            }
            ConfirmAction::Restore(path) => {
                let path = path.clone();
                self.change(|app| {
                    app.db.restore_from(&path)?;
                    app.age_settings = app.db.age_settings()?;
                    Ok(())
                });
                if self.notice.as_ref().is_none_or(|notice| !notice.is_error) {
                    self.say("Restored from backup.");
                }
            }
        }
        false
    }
}

impl eframe::App for PennyApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.menu_bar(ui);
        self.status_bar(ui);
        self.products_panel(ui);
        self.central_panel(ui);
        let ctx = ui.ctx().clone();
        self.show_dialog(&ctx);
    }
}

/// Shows a dialog over the window. The bool is true when the user clicked outside it or pressed Escape.
fn modal<R>(ctx: &egui::Context, content: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let response = egui::Modal::new(egui::Id::unique("dialog")).show(ctx, |ui| {
        ui.set_min_width(380.0);
        content(ui)
    });
    let should_close = response.should_close();
    (response.inner, should_close)
}

fn about_dialog(ctx: &egui::Context) -> bool {
    let mut close = false;
    let (_, should_close) = modal(ctx, |ui| {
        ui.heading("Penny Pincher");
        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        ui.add_space(6.0);
        ui.label("Keep track of prices and find the best deal, even when stores sell different sizes.");
        ui.add_space(6.0);
        ui.label("Based on My Price Log by Steven Flintham (MIT License).");
        ui.label("Penny photo: United States Mint (public domain).");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.button("Close").clicked();
            });
        });
    });
    !(close || should_close)
}

fn age_label(ui: &mut Ui, compared: &ComparedPrice, settings: &AgeSettings) {
    let text = describe_age(compared.age_days);
    match compared.age_class {
        AgeClass::Fresh => {
            ui.label(text);
        }
        AgeClass::Stale => {
            let color = if ui.visuals().dark_mode { egui::Color32::from_rgb(230, 190, 90) } else { egui::Color32::from_rgb(150, 100, 0) };
            ui.colored_label(color, text).on_hover_text(format!(
                "Seen more than {} days ago, so it's adjusted for inflation.",
                settings.stale_after_days
            ));
        }
        AgeClass::Ancient => {
            ui.colored_label(forms::error_color(ui), text).on_hover_text(format!(
                "Seen more than {} days ago. Check it next time you're there.",
                settings.ancient_after_days
            ));
        }
    }
}

fn describe_age(days: i64) -> String {
    match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        2..=59 => format!("{days} days ago"),
        60..=729 => format!("{} months ago", days / 30),
        _ => format!("{} years ago", days / 365),
    }
}
