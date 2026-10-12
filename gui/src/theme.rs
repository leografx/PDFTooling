//! macOS look: system font (SF Pro), System Settings colours for light and
//! dark mode, and the small custom widgets egui doesn't have (switch,
//! segmented control, toolbar button, sidebar tile).

use eframe::egui::{
    self, epaint::text::VariationCoords, vec2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    FontTweak, Margin, Response, Sense, Shadow, Stroke, TextStyle, Theme, Ui, Visuals, WidgetText,
};
use std::sync::Arc;

pub const BOLD: &str = "bold";
pub const ICONS: &str = "icons";
pub const ICON_FILL: &str = "icons-fill";

/// Colours for the current appearance.
#[derive(Clone, Copy)]
pub struct Pal {
    pub dark: bool,
    pub content: Color32,
    pub sidebar: Color32,
    pub group: Color32,
    pub border: Color32,
    pub separator: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub tertiary: Color32,
    pub accent: Color32,
    pub red: Color32,
    pub green: Color32,
    pub orange: Color32,
    pub hover: Color32,
}

pub fn pal(ui: &Ui) -> Pal {
    pal_for(ui.visuals().dark_mode)
}

pub fn pal_for(dark: bool) -> Pal {
    if dark {
        Pal {
            dark,
            content: Color32::from_rgb(0x1E, 0x1E, 0x1E),
            sidebar: Color32::from_rgb(0x28, 0x28, 0x2A),
            group: Color32::from_rgb(0x2A, 0x2A, 0x2C),
            border: Color32::from_rgb(0x3A, 0x3A, 0x3C),
            separator: Color32::from_rgb(0x38, 0x38, 0x3A),
            text: Color32::from_rgb(0xF5, 0xF5, 0xF7),
            secondary: Color32::from_rgb(0x98, 0x98, 0x9D),
            tertiary: Color32::from_rgb(0x63, 0x63, 0x66),
            accent: Color32::from_rgb(0x0A, 0x84, 0xFF),
            red: Color32::from_rgb(0xFF, 0x45, 0x3A),
            green: Color32::from_rgb(0x30, 0xD1, 0x58),
            orange: Color32::from_rgb(0xFF, 0x9F, 0x0A),
            hover: Color32::from_rgba_unmultiplied(255, 255, 255, 18),
        }
    } else {
        Pal {
            dark,
            content: Color32::from_rgb(0xF5, 0xF5, 0xF7),
            sidebar: Color32::from_rgb(0xE8, 0xE8, 0xEB),
            group: Color32::WHITE,
            border: Color32::from_rgb(0xDE, 0xDE, 0xE1),
            separator: Color32::from_rgb(0xE6, 0xE6, 0xE8),
            text: Color32::from_rgb(0x1D, 0x1D, 0x1F),
            secondary: Color32::from_rgb(0x6E, 0x6E, 0x73),
            tertiary: Color32::from_rgb(0xAE, 0xAE, 0xB2),
            accent: Color32::from_rgb(0x00, 0x7A, 0xFF),
            red: Color32::from_rgb(0xFF, 0x3B, 0x30),
            green: Color32::from_rgb(0x28, 0xA7, 0x45),
            orange: Color32::from_rgb(0xF5, 0x8A, 0x00),
            hover: Color32::from_rgba_unmultiplied(0, 0, 0, 12),
        }
    }
}

// ------------------------------------------------------------------ fonts

fn first_existing(paths: &[&str]) -> Option<(String, Vec<u8>)> {
    paths.iter().find_map(|p| std::fs::read(p).ok().map(|b| (p.to_string(), b)))
}

/// Use the system UI font when it can be found (SF Pro on macOS, Segoe UI on
/// Windows), falling back to egui's built-in font. `IMPOSE_SETTINGS_FONT=builtin`
/// forces the built-in font.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let builtin = std::env::var("IMPOSE_SETTINGS_FONT").map(|v| v == "builtin").unwrap_or(false);

    let regular = if builtin {
        None
    } else {
        first_existing(&[
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/SFNSText.ttf",
            "/System/Library/Fonts/HelveticaNeue.ttc",
            "C:\\Windows\\Fonts\\SegUIVar.ttf", // Segoe UI Variable (Windows 11)
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "/usr/share/fonts/opentype/inter/Inter-Regular.otf",
            "/usr/share/fonts/truetype/inter/Inter-Regular.ttf",
        ])
    };
    let bold_file = if builtin {
        None
    } else {
        first_existing(&[
            "C:\\Windows\\Fonts\\seguisb.ttf",
            "/usr/share/fonts/opentype/inter/Inter-SemiBold.otf",
            "/usr/share/fonts/truetype/inter/Inter-SemiBold.ttf",
        ])
    };
    let mono = if builtin {
        None
    } else {
        first_existing(&["/System/Library/Fonts/SFNSMono.ttf", "/System/Library/Fonts/Menlo.ttc", "C:\\Windows\\Fonts\\consola.ttf"])
    };

    let mut bold_key = None;
    let mut regular_is_variable = false;
    if let Some((_path, bytes)) = regular {
        let data = FontData::from_owned(bytes);
        let variable = data.variation_axes().iter().any(|a| &a.tag.to_be_bytes() == b"wght");
        fonts.font_data.insert("system".into(), Arc::new(data.clone()));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "system".into());
        regular_is_variable = variable;
        if variable {
            // SF Pro and Segoe UI Variable are variable fonts: semibold is the same file at wght 600.
            let tweak = FontTweak { coords: VariationCoords::new([(b"wght", 600.0)]), ..Default::default() };
            fonts.font_data.insert("system-semibold".into(), Arc::new(data.tweak(tweak)));
            bold_key = Some("system-semibold".to_string());
        }
    }
    if let Some((_p, bytes)) = bold_file.filter(|_| !regular_is_variable) {
        fonts.font_data.insert("system-semibold".into(), Arc::new(FontData::from_owned(bytes)));
        bold_key = Some("system-semibold".to_string());
    }
    if let Some((_p, bytes)) = mono {
        fonts.font_data.insert("system-mono".into(), Arc::new(FontData::from_owned(bytes)));
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "system-mono".into());
    }

    // Icons get their own families: system fonts (SF Pro, Inter …) have their own
    // glyphs in the private-use range Phosphor uses, so icons must never be
    // resolved through the text font.
    fonts.font_data.insert("phosphor".into(), Arc::new(FontData::from_static(egui_phosphor::Variant::Regular.font_bytes())));
    fonts.font_data.insert("phosphor-fill".into(), Arc::new(FontData::from_static(egui_phosphor::Variant::Fill.font_bytes())));
    fonts.families.insert(FontFamily::Name(ICONS.into()), vec!["phosphor".into()]);
    fonts.families.insert(FontFamily::Name(ICON_FILL.into()), vec!["phosphor-fill".into()]);
    fonts.families.entry(FontFamily::Proportional).or_default().push("phosphor".into());

    // "bold" family: semibold face first, then everything proportional (icons included).
    let mut bold_family: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(k) = bold_key {
        bold_family.insert(0, k);
    }
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold_family);

    ctx.set_fonts(fonts);
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

/// An icon on its own (body size unless `.size()` is set).
pub fn ic(icon: &str) -> egui::RichText {
    egui::RichText::new(icon).family(FontFamily::Name(ICONS.into())).size(14.0)
}

/// Icon followed by text, as one piece of widget text.
pub fn itext(icon: &str, text: &str, size: f32, color: Color32) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    job.append(icon, 0.0, TextFormat { font_id: icon_font(size + 1.0), color, valign: egui::Align::Center, ..Default::default() });
    job.append(text, 6.0, TextFormat { font_id: FontId::proportional(size), color, valign: egui::Align::Center, ..Default::default() });
    job
}

// ------------------------------------------------------------------ visuals

pub fn install_style(ctx: &egui::Context) {
    for theme in [Theme::Light, Theme::Dark] {
        let p = pal_for(theme == Theme::Dark);
        ctx.style_mut_of(theme, |s| {
            s.text_styles = [
                (TextStyle::Small, FontId::proportional(11.0)),
                (TextStyle::Body, FontId::proportional(13.0)),
                (TextStyle::Button, FontId::proportional(13.0)),
                (TextStyle::Heading, FontId::new(20.0, FontFamily::Name(BOLD.into()))),
                (TextStyle::Monospace, FontId::monospace(12.0)),
            ]
            .into();
            s.spacing.item_spacing = vec2(8.0, 6.0);
            s.spacing.button_padding = vec2(10.0, 4.0);
            s.spacing.interact_size = vec2(24.0, 22.0);
            s.spacing.combo_width = 180.0;
            s.spacing.tooltip_width = 340.0;
            s.spacing.menu_margin = Margin::same(6);
            s.interaction.tooltip_delay = 0.45;
            s.interaction.show_tooltips_only_when_still = false;

            let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
            v.panel_fill = p.content;
            v.window_fill = if p.dark { Color32::from_rgb(0x2C, 0x2C, 0x2E) } else { Color32::from_rgb(0xF6, 0xF6, 0xF6) };
            v.window_stroke = Stroke::new(1.0, p.border);
            v.window_corner_radius = CornerRadius::same(12);
            v.menu_corner_radius = CornerRadius::same(8);
            v.window_shadow = Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if p.dark { 110 } else { 50 }) };
            v.popup_shadow = Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if p.dark { 100 } else { 40 }) };
            v.extreme_bg_color = if p.dark { Color32::from_rgb(0x1C, 0x1C, 0x1E) } else { Color32::WHITE };
            v.text_edit_bg_color = Some(v.extreme_bg_color);
            v.faint_bg_color = p.group;
            v.override_text_color = None;
            v.hyperlink_color = p.accent;
            v.selection.bg_fill = p.accent.gamma_multiply(if p.dark { 0.55 } else { 0.3 });
            v.selection.stroke = Stroke::new(1.0, p.accent);
            v.error_fg_color = p.red;
            v.warn_fg_color = p.orange;
            let r = CornerRadius::same(6);
            let w = &mut v.widgets;
            w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
            w.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
            w.noninteractive.corner_radius = r;
            let btn = if p.dark { Color32::from_rgb(0x48, 0x48, 0x4A) } else { Color32::WHITE };
            let btn_hover = if p.dark { Color32::from_rgb(0x55, 0x55, 0x58) } else { Color32::from_rgb(0xF2, 0xF2, 0xF4) };
            let btn_down = if p.dark { Color32::from_rgb(0x60, 0x60, 0x63) } else { Color32::from_rgb(0xE4, 0xE4, 0xE7) };
            let edge = if p.dark { Color32::from_rgb(0x55, 0x55, 0x58) } else { Color32::from_rgb(0xD1, 0xD1, 0xD6) };
            for (st, fill) in [(&mut w.inactive, btn), (&mut w.hovered, btn_hover), (&mut w.active, btn_down), (&mut w.open, btn_down)] {
                st.bg_fill = fill;
                st.weak_bg_fill = fill;
                st.bg_stroke = Stroke::new(1.0, edge);
                st.fg_stroke = Stroke::new(1.0, p.text);
                st.corner_radius = r;
                st.expansion = 0.0;
            }
            w.hovered.bg_stroke = Stroke::new(1.0, edge);
            w.active.bg_stroke = Stroke::new(1.0, p.accent);
            s.visuals = v;
        });
    }
}

// ------------------------------------------------------------------ widgets

/// macOS switch. Returns the response (changed when toggled).
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(34.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, ""));
    if ui.is_rect_visible(rect) {
        let p = pal(ui);
        let t = ui.ctx().animate_bool_responsive(resp.id, *on);
        let off = if p.dark { Color32::from_rgb(0x48, 0x48, 0x4A) } else { Color32::from_rgb(0xE0, 0xE0, 0xE3) };
        let mut track = lerp_color(off, p.accent, t);
        if !ui.is_enabled() {
            track = track.gamma_multiply(0.45);
        }
        let painter = ui.painter();
        painter.rect(rect, CornerRadius::same(10), track, Stroke::NONE, egui::StrokeKind::Inside);
        let r = rect.height() / 2.0 - 2.0;
        let cx = egui::lerp((rect.left() + r + 2.0)..=(rect.right() - r - 2.0), t);
        let c = egui::pos2(cx, rect.center().y);
        painter.circle_filled(c + vec2(0.0, 0.6), r + 0.4, Color32::from_black_alpha(40));
        painter.circle_filled(c, r, if ui.is_enabled() { Color32::WHITE } else { Color32::from_gray(235) });
    }
    resp
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Segmented control. `items` = (icon, label, hover help). Returns the clicked index.
pub fn segmented(ui: &mut Ui, id: egui::Id, selected: usize, items: &[(&str, &str, &str)]) -> Option<usize> {
    let p = pal(ui);
    let mut clicked = None;
    let font = FontId::proportional(12.0);
    let pad = 10.0;
    let h = 22.0;
    let galleys: Vec<_> = items
        .iter()
        .map(|(icon, label, _)| {
            let text = if icon.is_empty() { label.to_string() } else { format!("{icon} {label}") };
            ui.painter().layout_no_wrap(text, font.clone(), p.text)
        })
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + pad * 2.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (rect, _) = ui.allocate_exact_size(vec2(total, h), Sense::hover());
    let track = if p.dark { Color32::from_rgb(0x32, 0x32, 0x34) } else { Color32::from_rgb(0xE6, 0xE6, 0xE9) };
    ui.painter().rect_filled(rect, CornerRadius::same(7), track);
    let mut x = rect.left() + 2.0;
    for (i, (g, w)) in galleys.into_iter().zip(widths).enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 2.0), vec2(w, h - 4.0));
        let resp = ui.interact(r, id.with(i), Sense::click());
        let resp = resp.on_hover_text(items[i].2);
        let sel = i == selected;
        if sel {
            let fill = if p.dark { Color32::from_rgb(0x63, 0x63, 0x66) } else { Color32::WHITE };
            ui.painter().rect_filled(r.translate(vec2(0.0, 0.5)), CornerRadius::same(5), Color32::from_black_alpha(22));
            ui.painter().rect_filled(r, CornerRadius::same(5), fill);
        } else if resp.hovered() && ui.is_enabled() {
            ui.painter().rect_filled(r, CornerRadius::same(5), p.hover);
        } else if i > 0 && i != selected + 1 {
            let sx = r.left();
            ui.painter().line_segment([egui::pos2(sx, r.top() + 4.0), egui::pos2(sx, r.bottom() - 4.0)], Stroke::new(1.0, p.border));
        }
        let mut color = if sel { p.text } else { p.secondary };
        if !ui.is_enabled() {
            color = color.gamma_multiply(0.5);
        }
        ui.painter().galley_with_override_text_color(r.center() - g.size() / 2.0, g, color);
        if resp.clicked() && ui.is_enabled() {
            clicked = Some(i);
        }
        x += w;
    }
    clicked
}

/// Borderless toolbar icon button with hover background.
pub fn tool_button(ui: &mut Ui, icon: &str, enabled: bool) -> Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, CornerRadius::same(6), p.hover);
    }
    let color = if enabled { p.secondary } else { p.tertiary };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icon, icon_font(18.0), color);
    resp
}

/// Coloured rounded tile with a white filled icon, like System Settings.
pub fn tile(ui: &mut Ui, icon: &str, rgb: [u8; 3], size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    let top = lerp_color(c, Color32::WHITE, 0.12);
    ui.painter().rect_filled(rect, CornerRadius::same((size * 0.25) as u8), c);
    ui.painter().rect_filled(
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.center().y)),
        CornerRadius { nw: (size * 0.25) as u8, ne: (size * 0.25) as u8, sw: 0, se: 0 },
        top.gamma_multiply(0.35),
    );
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icon, FontId::new(size * 0.62, FontFamily::Name(ICON_FILL.into())), Color32::WHITE);
}

/// Rounded white box holding a group of rows.
pub fn group_frame(p: &Pal) -> egui::Frame {
    egui::Frame::new()
        .fill(p.group)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin { left: 12, right: 10, top: 2, bottom: 2 })
}

/// Blue “primary” push button.
pub fn primary_button(ui: &mut Ui, text: impl Into<WidgetText>) -> Response {
    let p = pal(ui);
    let text: WidgetText = text.into();
    let text = match text {
        WidgetText::LayoutJob(_) => text,
        t => t.color(Color32::WHITE),
    };
    ui.add(egui::Button::new(text).fill(p.accent).stroke(Stroke::NONE).min_size(vec2(84.0, 24.0)))
}

pub fn button(ui: &mut Ui, text: impl Into<WidgetText>) -> Response {
    ui.add(egui::Button::new(text).min_size(vec2(84.0, 24.0)))
}
