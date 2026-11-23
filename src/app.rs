//! The main window.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use anyhow::Result;
use egui::{RichText, Ui};

use crate::analysis::{self, AgeClass, AgeSettings, ComparedPrice, Thresholds};
use crate::converter::Converter;
use crate::db::{self, Db};
use crate::forms::{
    self, CategoryForm, ItemForm, Outcome, PriceContext, PriceForm, SettingsForm, SourceForm,
    format_unit_price, judgement_badge,
};
use crate::model::{Category, DataSet, Item, LoyaltyType, Price, Source, Variant, now_millis};
use crate::units::{self, Unit};
use crate::update::{self, Release};

pub struct PennyApp {
    db: Db,
    age_settings: AgeSettings,
    /// Currency and units.
    data_set: DataSet,
    categories: Vec<Category>,
    items: Vec<Item>,
    sources: Vec<Source>,
    item_id: Option<i64>,
    /// Variants of the selected product.
    variants: Vec<Variant>,
    search: String,
    /// Current prices for the selected product, cheapest first.
    comparison: Vec<ComparedPrice>,
    thresholds: Option<Thresholds>,
    unit_price_denominator: Option<Unit>,
    dialog: Option<Dialog>,
    notice: Option<Notice>,
    /// Background update checks and downloads report back on this channel.
    update_events: (Sender<UpdateEvent>, Receiver<UpdateEvent>),
    available_update: Option<Release>,
    checked_for_update_at_start: bool,
}

enum UpdateEvent {
    Checked { result: Result<Option<Release>>, asked: bool },
    Installed { version: String, result: Result<()> },
}

enum Dialog {
    Item(ItemForm),
    Source(SourceForm),
    Category(CategoryForm),
    Price(PriceForm),
    History(PriceKey),
    Stores,
    Categories,
    Converter(Converter),
    Settings(SettingsForm),
    Confirm(Confirm),
    About,
}

/// Identifies one line of prices: a product (and maybe a variant of it) at a store.
#[derive(Clone, Copy)]
struct PriceKey {
    item_id: i64,
    source_id: i64,
    variant_id: Option<i64>,
}

struct Confirm {
    question: String,
    detail: Option<String>,
    button: &'static str,
    action: ConfirmAction,
}

enum ConfirmAction {
    DeleteItem(i64),
    DeleteSource(i64),
    DeleteCategory(i64),
    DeletePrice { id: i64, key: PriceKey },
    DeletePriceHistory(PriceKey),
    Restore(PathBuf),
}

/// A message in the status bar, with an optional button.
struct Notice {
    text: String,
    is_error: bool,
    action: Option<NoticeAction>,
}

#[derive(Clone, Copy)]
enum NoticeAction {
    UndoPrice(i64),
    InstallUpdate,
    Restart,
}

impl PennyApp {
    pub fn new() -> Result<Self> {
        let db = Db::open(&db::default_path())?;
        let item_id = db.setting("item_id")?.and_then(|value| value.parse().ok());

        let mut app = PennyApp {
            age_settings: db.age_settings()?,
            data_set: db.data_set()?,
            db,
            categories: Vec::new(),
            items: Vec::new(),
            sources: Vec::new(),
            item_id,
            variants: Vec::new(),
            search: String::new(),
            comparison: Vec::new(),
            thresholds: None,
            unit_price_denominator: None,
            dialog: None,
            notice: None,
            update_events: mpsc::channel(),
            available_update: None,
            checked_for_update_at_start: false,
        };
        app.reload()?;
        Ok(app)
    }

    /// Re-reads everything shown on screen. The data is small enough that this is instant.
    fn reload(&mut self) -> Result<()> {
        // A restored backup may have a different settings row.
        self.data_set = self.db.data_set()?;
        let data_set_id = self.data_set.id;
        self.categories = self.db.categories(data_set_id)?;
        self.items = self.db.items(data_set_id)?;
        self.sources = self.db.sources(data_set_id)?;
        if !self.items.iter().any(|item| Some(item.id) == self.item_id) {
            self.item_id = None;
        }

        self.variants.clear();
        self.comparison.clear();
        self.thresholds = None;
        self.unit_price_denominator = None;
        if let Some(item) = self.item().cloned() {
            let data_set = &self.data_set;
            self.variants = self.db.variants(item.id)?;
            let prices = self.db.current_prices(item.id)?;
            let (comparison, thresholds) =
                analysis::compare(&prices, &self.sources, &self.variants, &self.age_settings, now_millis());
            let per_base_unit: Vec<f64> = comparison.iter().map(|compared| compared.adjusted_price_per_base_unit).collect();
            let candidates = data_set.units_like(data_set.usable_unit(item.default_unit));
            self.unit_price_denominator = units::friendly_denominator(&candidates, &per_base_unit, data_set.currency().decimals);
            self.comparison = comparison;
            self.thresholds = thresholds;
        }

        let item_id = self.item_id.map(|id| id.to_string()).unwrap_or_default();
        self.db.set_setting("item_id", &item_id)?;
        Ok(())
    }

    fn item(&self) -> Option<&Item> {
        self.items.iter().find(|item| Some(item.id) == self.item_id)
    }

    /// Runs a database change, then refreshes the screen. Errors show in the status bar.
    fn change(&mut self, action: impl FnOnce(&mut Self) -> Result<()>) {
        let result = action(self).and_then(|()| self.reload());
        if let Err(error) = result {
            self.report_error(error);
        }
    }

    fn say(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice { text: text.into(), is_error: false, action: None });
    }

    fn notify(&mut self, text: impl Into<String>, action: NoticeAction) {
        self.notice = Some(Notice { text: text.into(), is_error: false, action: Some(action) });
    }

    fn report_error(&mut self, error: anyhow::Error) {
        self.notice = Some(Notice { text: format!("{error:#}"), is_error: true, action: None });
    }

    fn price_context(&self) -> Option<PriceContext<'_>> {
        Some(PriceContext {
            data_set: &self.data_set,
            item: self.item()?,
            sources: &self.sources,
            variants: &self.variants,
            comparison: &self.comparison,
            thresholds: self.thresholds,
        })
    }

    fn open_price_form(&mut self, source_id: i64, variant_id: Option<i64>) {
        if let Some(context) = self.price_context() {
            self.dialog = Some(Dialog::Price(PriceForm::new(&context, source_id, variant_id)));
        }
    }

    fn new_item(&self) -> Item {
        Item {
            id: 0,
            data_set_id: self.data_set.id,
            name: String::new(),
            default_unit: self.data_set.units_for(units::QuantityType::Weight, false)[0],
            allow_multipack: false,
            notes: String::new(),
            category_id: None,
        }
    }

    fn open_item_form(&mut self, item: Item) {
        match self.item_form(item) {
            Ok(form) => self.dialog = Some(Dialog::Item(form)),
            Err(error) => self.report_error(error),
        }
    }

    fn item_form(&self, item: Item) -> Result<ItemForm> {
        if item.id == 0 {
            return Ok(ItemForm::new(item, false, Vec::new(), Vec::new()));
        }
        let has_prices = self.db.count_prices_for_item(item.id)? > 0;
        let variants = self.db.variants(item.id)?;
        let mut variants_with_prices = Vec::new();
        for variant in &variants {
            if self.db.count_prices_for_variant(variant.id)? > 0 {
                variants_with_prices.push(variant.id);
            }
        }
        Ok(ItemForm::new(item, has_prices, variants, variants_with_prices))
    }

    fn new_source(&self) -> Source {
        Source {
            id: 0,
            data_set_id: self.data_set.id,
            name: String::new(),
            loyalty_type: LoyaltyType::None,
            loyalty_multiplier: 1.0,
            notes: String::new(),
        }
    }

    fn new_category(&self) -> Category {
        Category { id: 0, data_set_id: self.data_set.id, name: String::new() }
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
                        self.dialog = Some(Dialog::Settings(SettingsForm::new(&self.data_set, &self.age_settings)));
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Categories", |ui| {
                    if ui.button("Manage categories…").clicked() {
                        ui.close();
                        self.dialog = Some(Dialog::Categories);
                    }
                    if ui.button("New category…").clicked() {
                        ui.close();
                        self.dialog = Some(Dialog::Category(CategoryForm::new(self.new_category())));
                    }
                });
                ui.menu_button("Stores", |ui| {
                    if ui.button("Manage stores…").clicked() {
                        ui.close();
                        self.dialog = Some(Dialog::Stores);
                    }
                    if ui.button("New store…").clicked() {
                        ui.close();
                        self.dialog = Some(Dialog::Source(SourceForm::new(self.new_source())));
                    }
                });
                if ui.button("Unit converter").clicked() {
                    self.dialog = Some(Dialog::Converter(Converter::default()));
                }
                ui.menu_button("Help", |ui| {
                    if ui.button("Check for updates").clicked() {
                        ui.close();
                        self.check_for_update(ui.ctx(), true);
                    }
                    if ui.button("About Penny Pincher").clicked() {
                        ui.close();
                        self.dialog = Some(Dialog::About);
                    }
                });
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let Some(notice) = &self.notice else { return };
        let mut clicked = None;
        let mut dismiss = false;
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if notice.is_error {
                    ui.colored_label(forms::error_color(ui), &notice.text);
                } else {
                    ui.label(&notice.text);
                }
                if let Some(action) = notice.action {
                    let label = match action {
                        NoticeAction::UndoPrice(_) => "Undo",
                        NoticeAction::InstallUpdate => "Update",
                        NoticeAction::Restart => "Restart",
                    };
                    if ui.button(label).clicked() {
                        clicked = Some(action);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    dismiss = ui.button("✕").clicked();
                });
            });
        });
        if dismiss {
            self.notice = None;
        }
        match clicked {
            None => {}
            Some(NoticeAction::UndoPrice(id)) => {
                self.change(|app| app.db.delete_price(id));
                self.say("Undone.");
            }
            Some(NoticeAction::InstallUpdate) => self.install_update(ui.ctx()),
            Some(NoticeAction::Restart) => match update::restart() {
                Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                Err(error) => self.report_error(error.context("Couldn't restart")),
            },
        }
    }

    // Updates

    fn check_for_update(&mut self, ctx: &egui::Context, asked: bool) {
        if asked {
            self.say("Checking for updates…");
        }
        let events = self.update_events.0.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = events.send(UpdateEvent::Checked { result: update::check_for_update(), asked });
            ctx.request_repaint();
        });
    }

    fn install_update(&mut self, ctx: &egui::Context) {
        let Some(release) = self.available_update.take() else { return };
        self.say(format!("Downloading version {}…", release.version));
        let events = self.update_events.0.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = update::install(&release);
            let _ = events.send(UpdateEvent::Installed { version: release.version, result });
            ctx.request_repaint();
        });
    }

    fn handle_update_events(&mut self) {
        while let Ok(event) = self.update_events.1.try_recv() {
            match event {
                UpdateEvent::Checked { result: Ok(Some(release)), .. } => {
                    self.notify(format!("Version {} is available.", release.version), NoticeAction::InstallUpdate);
                    self.available_update = Some(release);
                }
                UpdateEvent::Checked { result: Ok(None), asked: true } => self.say("You have the latest version."),
                UpdateEvent::Checked { result: Err(error), asked: true } => {
                    self.report_error(error.context("Couldn't check for updates"));
                }
                // The automatic check stays quiet when there's nothing new or no internet.
                UpdateEvent::Checked { asked: false, .. } => {}
                UpdateEvent::Installed { version, result: Ok(()) } => {
                    self.notify(format!("Updated to version {version}. Restart to use it."), NoticeAction::Restart);
                }
                UpdateEvent::Installed { result: Err(error), .. } => self.report_error(error),
            }
        }
    }

    fn products_panel(&mut self, ui: &mut Ui) {
        let mut clicked = None;
        let mut add = false;
        egui::Panel::left("products").resizable(true).default_size(240.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Products");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Add").clicked() {
                        add = true;
                    }
                });
            });
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(f32::INFINITY));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let search = self.search.trim().to_lowercase();
                let matches = |item: &&Item| item.name.to_lowercase().contains(&search);

                // Each category in turn, then products without one.
                let mut groups: Vec<(&str, Vec<&Item>)> = self
                    .categories
                    .iter()
                    .map(|category| {
                        let items = self.items.iter().filter(|item| item.category_id == Some(category.id)).filter(matches).collect();
                        (category.name.as_str(), items)
                    })
                    .collect();
                let uncategorised: Vec<&Item> = self
                    .items
                    .iter()
                    .filter(|item| !self.categories.iter().any(|category| Some(category.id) == item.category_id))
                    .filter(matches)
                    .collect();
                let show_headings = groups.iter().any(|(_, items)| !items.is_empty());
                groups.push(("Other", uncategorised));

                for (name, items) in groups {
                    if items.is_empty() {
                        continue;
                    }
                    if show_headings {
                        ui.add_space(4.0);
                        ui.label(RichText::new(name).small().strong().weak());
                    }
                    for item in items {
                        let selected = Some(item.id) == self.item_id;
                        let label = egui::Button::selectable(selected, &item.name)
                            .min_size(egui::vec2(ui.available_width(), crate::touch::TAP_HEIGHT));
                        if ui.add(label).clicked() {
                            clicked = Some(item.id);
                        }
                    }
                }
            });
        });
        if add {
            self.open_item_form(self.new_item());
        }
        if let Some(item_id) = clicked {
            self.item_id = Some(item_id);
            self.change(|_| Ok(()));
        }
    }

    fn central_panel(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let data_set = self.data_set.clone();
            let Some(item) = self.item().cloned() else {
                if self.items.is_empty() {
                    ui.label("Add the things you buy, like butter or milk.");
                    if ui.button("Add product").clicked() {
                        self.open_item_form(self.new_item());
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
                    // Start on a store that doesn't have a price yet, since that's the likely reason for adding one.
                    let source_id = self
                        .sources
                        .iter()
                        .find(|source| !self.comparison.iter().any(|compared| compared.price.source_id == source.id))
                        .unwrap_or(&self.sources[0])
                        .id;
                    self.open_price_form(source_id, None);
                }
                if ui.button("Edit product").clicked() {
                    self.open_item_form(item.clone());
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
                self.dialog = Some(Dialog::Source(SourceForm::new(self.new_source())));
            }
            return;
        }

        let currency = data_set.currency();
        let denominator = self.unit_price_denominator.unwrap_or(data_set.usable_unit(item.default_unit));
        let mut open_price = None;
        let mut confirm = None;
        let mut history = None;

        if self.comparison.is_empty() {
            ui.label("No prices yet. Use Add price when you see it in a store.");
            return;
        }

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            // One card per store and variant, cheapest first. Everything is visible without hovering, for touch screens.
            for compared in &self.comparison {
                let price = &compared.price;
                let key = PriceKey { item_id: price.item_id, source_id: price.source_id, variant_id: price.variant_id };
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(compared.title()).size(21.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            judgement_badge(ui, compared.judgement);
                            let unit_price = format_unit_price(currency, compared.adjusted_price_per_base_unit, denominator);
                            ui.label(RichText::new(unit_price).size(21.0).strong());
                        });
                    });
                    let details = |ui: &mut Ui| {
                        ui.spacing_mut().interact_size.y = 24.0;
                        ui.label(price.describe(currency));
                        ui.weak("·");
                        age_label(ui, compared);
                        if compared.is_adjusted() {
                            let shelf_unit_price = format_unit_price(currency, price.price_per_base_unit(price.price), denominator);
                            ui.weak(format!("· {shelf_unit_price} before adjustments"));
                        }
                        if let Some(source) = self.sources.iter().find(|source| source.id == price.source_id)
                            && source.loyalty_type != LoyaltyType::None
                        {
                            let percent = source.loyalty_type.percent_from_multiplier(source.loyalty_multiplier);
                            ui.weak(format!("· {} {}%", source.loyalty_type.label(), units::format_number(percent, 2)));
                        }
                        if !price.notes.is_empty() {
                            ui.weak(format!("· {}", price.notes));
                        }
                    };
                    let mut buttons = |ui: &mut Ui| {
                        if ui.button("New price").clicked() {
                            open_price = Some(key);
                        }
                        if ui.button("Still the same").clicked() {
                            confirm = Some(price.clone());
                        }
                        if ui.button("History").clicked() {
                            history = Some(key);
                        }
                    };

                    // Details beside the buttons when there's room, otherwise above them.
                    const BUTTONS_WIDTH: f32 = 380.0;
                    let details_width = ui.available_width() - BUTTONS_WIDTH;
                    let wrapping = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
                    if details_width >= 300.0 {
                        ui.horizontal(|ui| {
                            ui.allocate_ui_with_layout(egui::vec2(details_width, 0.0), wrapping, details);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.horizontal(|ui| buttons(ui));
                            });
                        });
                    } else {
                        ui.with_layout(wrapping, details);
                        ui.horizontal(|ui| buttons(ui));
                    }
                });
            }

            ui.add_space(4.0);
            ui.weak(format!(
                "Unit prices include store discounts and surcharges. Prices not seen for over {} days (amber) are \
                 adjusted for inflation. Prices over {} days old (red) aren't used to rate the others.",
                self.age_settings.stale_after_days, self.age_settings.ancient_after_days
            ));
        });

        if let Some(key) = open_price {
            self.open_price_form(key.source_id, key.variant_id);
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
                self.notify("Marked as seen today.", NoticeAction::UndoPrice(id));
            }
        }
        if let Some(key) = history {
            self.dialog = Some(Dialog::History(key));
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
            Err(error) => self.report_error(error),
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
            Dialog::Item(form) => self.item_dialog(ctx, form),
            Dialog::Source(form) => self.source_dialog(ctx, form),
            Dialog::Category(form) => self.category_dialog(ctx, form),
            Dialog::Price(form) => self.price_dialog(ctx, form),
            Dialog::History(key) => self.history_dialog(ctx, *key),
            Dialog::Stores => self.stores_dialog(ctx),
            Dialog::Categories => self.categories_dialog(ctx),
            Dialog::Converter(converter) => self.converter_dialog(ctx, converter),
            Dialog::Settings(form) => self.settings_dialog(ctx, form),
            Dialog::Confirm(confirm) => self.confirm_dialog(ctx, confirm),
            Dialog::About => about_dialog(ctx),
        };
        // A dialog can hand over to another one, such as Delete opening a confirmation.
        if keep_open && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }

    fn item_dialog(&mut self, ctx: &egui::Context, form: &mut ItemForm) -> bool {
        let data_set = self.data_set.clone();
        let others = self.items.clone();
        let categories = self.categories.clone();
        match modal(ctx, |ui| form.show(ui, &data_set, &others, &categories)).0 {
            Outcome::Open => true,
            Outcome::Cancel => false,
            Outcome::Save => {
                let draft = form.draft.clone();
                let variants = form.variants.clone();
                self.change(|app| {
                    let item_id = app.db.save_item(&draft)?;
                    app.db.replace_variants(item_id, &variants)?;
                    app.item_id = Some(item_id);
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
                    self.notify("Price saved.", NoticeAction::UndoPrice(id));
                }
                false
            }
            (Outcome::Cancel, _) => false,
            _ => true,
        }
    }

    fn history_dialog(&mut self, ctx: &egui::Context, key: PriceKey) -> bool {
        let Some(item) = self.item().cloned() else { return false };
        let data_set = self.data_set.clone();
        let source_name =
            self.sources.iter().find(|source| source.id == key.source_id).map(|source| source.name.clone()).unwrap_or_default();
        let product_name = match key.variant_id.and_then(|id| self.variants.iter().find(|variant| variant.id == id)) {
            Some(variant) => format!("{} ({})", item.name, variant.name),
            None => item.name.clone(),
        };
        let entries = match self.db.price_history(key.item_id, key.source_id, key.variant_id) {
            Ok(entries) => entries,
            Err(_) => return false,
        };
        let currency = data_set.currency();
        let denominator = self.unit_price_denominator.unwrap_or(data_set.usable_unit(item.default_unit));
        let mut delete = None;
        let mut delete_all = false;
        let mut close = false;

        let (_, should_close) = modal(ctx, |ui| {
            ui.heading(format!("{product_name} at {source_name}"));
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
                        if ui.button("Delete").on_hover_text(hint).clicked() {
                            delete = Some(entry.id);
                        }
                        ui.end_row();
                    }
                });
            });
            ui.weak("Unit prices here are shelf prices, without store discounts or surcharges.");
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
                question: format!("Delete this {product_name} price?"),
                detail: None,
                button: "Delete",
                action: ConfirmAction::DeletePrice { id, key },
            }));
            return false;
        }
        if delete_all {
            self.dialog = Some(Dialog::Confirm(Confirm {
                question: format!("Delete every {product_name} price at {source_name}?"),
                detail: None,
                button: "Delete all",
                action: ConfirmAction::DeletePriceHistory(key),
            }));
            return false;
        }
        !(close || should_close)
    }

    fn stores_dialog(&mut self, ctx: &egui::Context) -> bool {
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
                        if ui.button("Edit").clicked() {
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
            self.dialog = Some(Dialog::Source(SourceForm::new(self.new_source())));
            return false;
        }
        !(close || should_close)
    }

    fn categories_dialog(&mut self, ctx: &egui::Context) -> bool {
        let mut edit = None;
        let mut add = false;
        let mut close = false;
        let (_, should_close) = modal(ctx, |ui| {
            ui.heading("Categories");
            ui.add_space(6.0);
            if self.categories.is_empty() {
                ui.label("No categories yet.");
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                egui::Grid::new("categories").num_columns(3).spacing([16.0, 8.0]).striped(true).show(ui, |ui| {
                    for category in &self.categories {
                        ui.label(&category.name);
                        let count = self.items.iter().filter(|item| item.category_id == Some(category.id)).count();
                        ui.weak(if count == 1 { "1 product".to_string() } else { format!("{count} products") });
                        if ui.button("Edit").clicked() {
                            edit = Some(category.clone());
                        }
                        ui.end_row();
                    }
                });
            });
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                add = ui.button("New category").clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                });
            });
        });
        if let Some(category) = edit {
            self.dialog = Some(Dialog::Category(CategoryForm::new(category)));
            return false;
        }
        if add {
            self.dialog = Some(Dialog::Category(CategoryForm::new(self.new_category())));
            return false;
        }
        !(close || should_close)
    }

    fn category_dialog(&mut self, ctx: &egui::Context, form: &mut CategoryForm) -> bool {
        let others = self.categories.clone();
        match modal(ctx, |ui| form.show(ui, &others)).0 {
            Outcome::Open => true,
            Outcome::Cancel => {
                self.dialog = Some(Dialog::Categories);
                false
            }
            Outcome::Save => {
                let draft = form.draft.clone();
                self.change(|app| app.db.save_category(&draft).map(|_| ()));
                self.dialog = Some(Dialog::Categories);
                false
            }
            Outcome::Delete => {
                let has_products = self.items.iter().any(|item| item.category_id == Some(form.draft.id));
                self.dialog = Some(Dialog::Confirm(Confirm {
                    question: format!("Delete the {} category?", form.draft.name),
                    detail: has_products.then(|| "Its products stay, without a category.".into()),
                    button: "Delete",
                    action: ConfirmAction::DeleteCategory(form.draft.id),
                }));
                false
            }
        }
    }

    fn converter_dialog(&mut self, ctx: &egui::Context, converter: &mut Converter) -> bool {
        let currency = self.data_set.currency();
        let (close, should_close) = modal(ctx, |ui| converter.show(ui, currency));
        !(close || should_close)
    }

    fn settings_dialog(&mut self, ctx: &egui::Context, form: &mut SettingsForm) -> bool {
        match modal(ctx, |ui| form.show(ui)).0 {
            (Outcome::Save, Some(settings)) => {
                let data_set = form.data_set.clone();
                self.change(|app| {
                    app.db.save_data_set(&data_set)?;
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
            ConfirmAction::DeleteItem(id) => self.change(|app| app.db.delete_item(*id)),
            ConfirmAction::DeleteSource(id) => self.change(|app| app.db.delete_source(*id)),
            ConfirmAction::DeleteCategory(id) => {
                self.change(|app| app.db.delete_category(*id));
                self.dialog = Some(Dialog::Categories);
            }
            ConfirmAction::DeletePrice { id, key } => {
                self.change(|app| app.db.delete_price(*id));
                self.dialog = Some(Dialog::History(*key));
            }
            ConfirmAction::DeletePriceHistory(key) => {
                self.change(|app| app.db.delete_price_history(key.item_id, key.source_id, key.variant_id));
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
        if !self.checked_for_update_at_start {
            self.checked_for_update_at_start = true;
            self.check_for_update(ui.ctx(), false);
        }
        self.handle_update_events();
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
        ui.set_min_width(460.0);
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

fn card(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        content(ui);
    });
    ui.add_space(4.0);
}

fn age_label(ui: &mut Ui, compared: &ComparedPrice) {
    let text = format!("seen {}", describe_age(compared.age_days));
    match compared.age_class {
        AgeClass::Fresh => {
            ui.label(text);
        }
        AgeClass::Stale => {
            let color = if ui.visuals().dark_mode { egui::Color32::from_rgb(230, 190, 90) } else { egui::Color32::from_rgb(150, 100, 0) };
            ui.colored_label(color, text);
        }
        AgeClass::Ancient => {
            ui.colored_label(forms::error_color(ui), text);
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
