//! The look of the interface: its greys, corners and type, and egui's style made from them.
//! Everything is drawn in these, so that the app adds no colour of its own beside a photo.

use egui::{vec2, Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle};

/// Behind photos, and the wells in other surfaces: tracks, the histogram, text fields.
pub const WELL: Color32 = Color32::from_gray(22);
/// The status bar and the filmstrip.
pub const BAR: Color32 = Color32::from_gray(29);
/// The toolbar, the panel and the rail.
pub const PANEL: Color32 = Color32::from_gray(34);
/// Dialogs, menus and tooltips, and a photo's cell.
pub const RAISED: Color32 = Color32::from_gray(40);
/// A control at rest, a cell under the pointer, and every rule and outline.
pub const FILL_1: Color32 = Color32::from_gray(50);
/// A control under the pointer, a slider's rail and a checkbox.
pub const FILL_2: Color32 = Color32::from_gray(64);
/// A control held down, and the cell of a selected photo.
pub const FILL_3: Color32 = Color32::from_gray(80);
/// The one chosen of several: a segment, a lit switch, a dialog's main button, the focused photo's cell.
pub const FILL_4: Color32 = Color32::from_gray(96);
/// A setting's value left at its default, the quietest text: 4.1:1 on the panel.
pub const QUIET: Color32 = Color32::from_gray(130);
/// Names, status and hints: better than 5:1 on the panel.
pub const LABEL: Color32 = Color32::from_gray(150);
/// Text and marks on controls, and text on its own.
pub const PLAIN: Color32 = Color32::from_gray(205);
/// Titles, changed values, text on a chosen control and the outline of the focused photo.
pub const STRONG: Color32 = Color32::from_gray(220);
/// Under a label laid over a picture, and over what a crop leaves out.
pub const SCRIM: Color32 = Color32::from_black_alpha(160);
/// Shadows, and the dark edge that keeps the split's white line and ring visible on a light picture.
pub const SHADOW: Color32 = Color32::from_black_alpha(100);

/// The corner of a control or a cell.
pub const CORNER: CornerRadius = CornerRadius::same(4);
/// The corner of a box that holds controls or text: a segmented control, a menu, a tooltip.
pub const BOX_CORNER: CornerRadius = CornerRadius::same(6);
/// The corner of a dialog or a window.
pub const WINDOW_CORNER: CornerRadius = CornerRadius::same(10);

/// Toggles in the panel, names under thumbnails and labels over pictures.
pub fn small_font() -> FontId {
    FontId::proportional(11.0)
}

/// Text, buttons and values.
pub fn body_font() -> FontId {
    FontId::proportional(12.5)
}

/// The panel's section titles, a little heavier than the text.
pub fn title_font() -> FontId {
    FontId::new(12.5, FontFamily::Name("Title".into()))
}

/// The title of a dialog or a window, and the buttons of the empty window.
pub fn heading_font() -> FontId {
    FontId::new(15.0, title_font().family)
}

/// The names of the panel's settings, between the text and the titles.
pub fn label_family() -> FontFamily {
    FontFamily::Name("Label".into())
}

pub fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    #[cfg(target_os = "macos")]
    if let Ok(sf) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
        let sf = egui::FontData::from_owned(sf);
        for (name, weight) in [("SF", 400.0), ("SF Label", 430.0), ("SF Title", 500.0)] {
            let mut tweak = egui::FontTweak::default();
            tweak.coords.push(b"wght", weight);
            // the low end of the axis: the design for text of 17 pt and under, where the font's own default is 28
            tweak.coords.push(b"opsz", 17.0);
            fonts.font_data.insert(name.into(), std::sync::Arc::new(sf.clone().tweak(tweak)));
        }
    }
    let rest = fonts.families[&FontFamily::Proportional].clone();
    for (family, face) in [(FontFamily::Proportional, "SF"), (label_family(), "SF Label"), (title_font().family, "SF Title")] {
        let faces = fonts.font_data.contains_key(face).then(|| face.to_string()).into_iter().chain(rest.iter().cloned()).collect();
        fonts.families.insert(family, faces);
    }
    ctx.set_fonts(fonts);
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = RAISED;
    v.extreme_bg_color = WELL;
    v.selection.bg_fill = FILL_4;
    v.selection.stroke = Stroke::new(1.0f32, STRONG);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0f32, FILL_1);
    v.widgets.noninteractive.fg_stroke.color = PLAIN;
    v.widgets.inactive.weak_bg_fill = FILL_1;
    v.widgets.inactive.bg_fill = FILL_2;
    v.widgets.inactive.fg_stroke.color = PLAIN;
    v.widgets.hovered.weak_bg_fill = FILL_2;
    v.widgets.hovered.bg_fill = FILL_3;
    v.widgets.hovered.fg_stroke.color = Color32::WHITE;
    v.widgets.active.weak_bg_fill = FILL_3;
    v.widgets.active.bg_fill = FILL_3;
    v.widgets.open.weak_bg_fill = FILL_2;
    v.widgets.open.fg_stroke.color = PLAIN;
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CORNER;
        w.expansion = 0.0;
    }
    for w in [&mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.bg_stroke = Stroke::NONE;
    }
    v.window_corner_radius = WINDOW_CORNER;
    v.menu_corner_radius = BOX_CORNER;
    v.window_stroke = Stroke::new(1.0f32, FILL_1);
    v.window_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: SHADOW };
    v.popup_shadow = egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: SHADOW };
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = vec2(6.0, 5.0);
        s.spacing.button_padding = vec2(10.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        s.spacing.menu_margin = Margin::same(5);
        s.text_styles.insert(TextStyle::Body, body_font());
        s.text_styles.insert(TextStyle::Button, body_font());
        s.text_styles.insert(TextStyle::Small, small_font());
        s.text_styles.insert(TextStyle::Heading, heading_font());
        s.interaction.tooltip_delay = 0.6;
        s.animation_time = 0.1;
    });
}
