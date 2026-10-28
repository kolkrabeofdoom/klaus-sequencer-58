use eframe::egui::{self, Color32, CornerRadius, Stroke, Style, Visuals};

pub const GRAPHITE: Color32 = Color32::from_rgb(16, 20, 23);
pub const PANEL: Color32 = Color32::from_rgb(24, 30, 33);
pub const PANEL_RAISED: Color32 = Color32::from_rgb(31, 38, 42);
pub const CYAN: Color32 = Color32::from_rgb(76, 225, 193);
pub const VIOLET: Color32 = Color32::from_rgb(177, 130, 244);
pub const ORANGE: Color32 = Color32::from_rgb(244, 145, 67);
pub const ACID: Color32 = Color32::from_rgb(209, 244, 79);
pub const MUTED: Color32 = Color32::from_rgb(132, 145, 153);
pub const DANGER: Color32 = Color32::from_rgb(255, 100, 92);

pub fn install(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = GRAPHITE;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(9, 12, 14);
    visuals.faint_bg_color = PANEL_RAISED;
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(55, 67, 73));
    visuals.widgets.inactive.bg_fill = PANEL_RAISED;
    visuals.widgets.inactive.weak_bg_fill = PANEL_RAISED;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(63, 76, 82));
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 56, 60);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(45, 56, 60);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.5, CYAN);
    visuals.widgets.active.bg_fill = CYAN;
    visuals.widgets.active.weak_bg_fill = CYAN;
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, Color32::from_rgb(6, 17, 14));
    visuals.selection.bg_fill = CYAN.gamma_multiply(0.72);
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.window_corner_radius = CornerRadius::same(10);

    let mut style = Style {
        visuals,
        ..Default::default()
    };
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.item_spacing = egui::vec2(6.0, 6.0);
    style.spacing.interact_size = egui::vec2(44.0, 44.0);
    ctx.set_style(style);
}

pub fn lane_color(lane: usize) -> Color32 {
    match lane {
        0 | 1 => CYAN,
        2 | 3 => VIOLET,
        4 | 5 => Color32::from_rgb(91, 202, 157),
        _ => ORANGE,
    }
}
