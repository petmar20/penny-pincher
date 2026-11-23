//! Sizes everything for fingers: about 44 px tall tap targets and larger text.

use egui::{FontFamily, FontId, TextStyle, vec2};

pub const TAP_HEIGHT: f32 = 44.0;

pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(18.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(18.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(26.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(16.0, FontFamily::Monospace)),
        ]
        .into();

        let spacing = &mut style.spacing;
        spacing.interact_size = vec2(TAP_HEIGHT, TAP_HEIGHT);
        spacing.button_padding = vec2(16.0, 10.0);
        spacing.item_spacing = vec2(10.0, 10.0);
        spacing.icon_width = 26.0;
        spacing.icon_width_inner = 14.0;
        spacing.icon_spacing = 10.0;
        spacing.combo_height = 400.0;
        spacing.menu_margin = egui::Margin::same(10);
        spacing.window_margin = egui::Margin::same(16);

        // Always-visible, wide scroll bars are easier to grab with a finger.
        let mut scroll = egui::style::ScrollStyle::solid();
        scroll.bar_width = 14.0;
        spacing.scroll = scroll;
    });
}
