//! The font a Body is shown in, as the reader chose it in Preferences.
//!
//! By default a Body uses the system font, which the web view is given as
//! its default font (see `locked_down_settings`), and nothing here applies.
//! A chosen font is given to the Body's root element by a user style sheet
//! with no specificity, so it is only the default: every rule in an HTML
//! Body wins over it, and text the Body styles itself keeps its own font.

use std::fmt::Write;

use gtk::glib::translate::IntoGlib;
use gtk::prelude::*;
use gtk::{gio, pango};

use super::style::Surface;
use crate::settings;

/// Pango's names for the generic families, and their CSS equivalents.
const GENERIC_FAMILIES: [(&str, &str); 8] = [
    ("sans", "sans-serif"),
    ("sans-serif", "sans-serif"),
    ("serif", "serif"),
    ("monospace", "monospace"),
    ("mono", "monospace"),
    ("cursive", "cursive"),
    ("fantasy", "fantasy"),
    ("system-ui", "system-ui"),
];

/// The reader's choice of font for Bodies.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontPreferences {
    /// The chosen font, or `None` to use the system font.
    pub custom: Option<pango::FontDescription>,
    /// Whether plain-text Bodies use the system monospace font, at the size
    /// of the chosen font if it has one.
    pub monospace_plain_text: bool,
}

/// The system fonts, and how WebKit turns points into CSS pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemFonts {
    /// The font the web view uses by default.
    pub default: Option<pango::FontDescription>,
    pub monospace: Option<pango::FontDescription>,
    /// CSS pixels per point at the system's font resolution.
    pub pixels_per_point: f64,
}

impl FontPreferences {
    /// The preferences as stored: `body_font` is a Pango font description,
    /// used only when `use_system_font` is off, and empty for the system font.
    pub fn new(use_system_font: bool, body_font: &str, monospace_plain_text: bool) -> Self {
        let custom = (!use_system_font && !body_font.trim().is_empty())
            .then(|| pango::FontDescription::from_string(body_font));
        Self {
            custom,
            monospace_plain_text,
        }
    }

    pub fn from_settings(settings: &gio::Settings) -> Self {
        Self::new(
            settings.boolean(settings::USE_SYSTEM_FONT),
            &settings.string(settings::BODY_FONT),
            settings.boolean(settings::MONOSPACE_PLAIN_TEXT),
        )
    }

    /// A user style sheet giving a Body on `surface` the chosen font, or
    /// nothing when the web view's default font is the one to use.
    ///
    /// Plain-text Bodies are on the system surface; HTML Bodies are on paper
    /// and never use the monospace font.
    pub fn stylesheet(&self, surface: Surface, system: &SystemFonts) -> String {
        let mut declarations = String::new();
        if surface == Surface::System && self.monospace_plain_text {
            let monospace = system.monospace.as_ref();
            let mut families = monospace.and_then(font_families).unwrap_or_default();
            if !families.ends_with("monospace") {
                if !families.is_empty() {
                    families.push_str(", ");
                }
                families.push_str("monospace");
            }
            // A size is always given, as the generic monospace family is
            // otherwise shown at WebKit's smaller fixed-width default size.
            let size = [self.custom.as_ref(), monospace, system.default.as_ref()]
                .into_iter()
                .flatten()
                .find_map(|font| font_size(font, system.pixels_per_point));
            write!(declarations, " font-family: {families};").unwrap();
            if let Some(size) = size {
                write!(declarations, " font-size: {size};").unwrap();
            }
        } else if let Some(font) = &self.custom {
            if let Some(families) = font_families(font) {
                write!(declarations, " font-family: {families};").unwrap();
            }
            if let Some(size) = font_size(font, system.pixels_per_point) {
                write!(declarations, " font-size: {size};").unwrap();
            }
            if let Some(weight) = font_weight(font) {
                write!(declarations, " font-weight: {weight};").unwrap();
            }
            if let Some(style) = font_style(font) {
                write!(declarations, " font-style: {style};").unwrap();
            }
        }
        if declarations.is_empty() {
            return String::new();
        }
        format!(":where(:root) {{{declarations} }}\n")
    }
}

/// The families of `font` as a CSS `font-family` value, or `None` if it
/// names none. Pango's generic families become CSS generic families; other
/// names are quoted, so that no name can end the value or the rule.
fn font_families(font: &pango::FontDescription) -> Option<String> {
    let families = font.family()?;
    let families: Vec<String> = families
        .split(',')
        .map(str::trim)
        .filter(|family| !family.is_empty())
        .map(|family| {
            GENERIC_FAMILIES
                .iter()
                .find(|(pango, _)| family.eq_ignore_ascii_case(pango))
                .map_or_else(|| css_string(family), |(_, css)| (*css).to_owned())
        })
        .collect();
    (!families.is_empty()).then(|| families.join(", "))
}

/// `text` as a CSS string. Anything but letters, digits, spaces, hyphens
/// and underscores is escaped.
fn css_string(text: &str) -> String {
    let mut css = String::with_capacity(text.len() + 2);
    css.push('"');
    for c in text.chars() {
        match c {
            // CSS turns NUL into U+FFFD; it has no place in a family name.
            '\0' => {}
            c if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') => css.push(c),
            // A hex escape ends at the space after it.
            c => write!(css, "\\{:x} ", u32::from(c)).unwrap(),
        }
    }
    css.push('"');
    css
}

/// The size of `font` in whole CSS pixels, rounded as WebKit rounds the
/// system font's size, or `None` if it has none.
fn font_size(font: &pango::FontDescription, pixels_per_point: f64) -> Option<String> {
    if !font.set_fields().contains(pango::FontMask::SIZE) || font.size() <= 0 {
        return None;
    }
    let size = f64::from(font.size()) / f64::from(pango::SCALE);
    let pixels = if font.is_size_absolute() {
        size
    } else {
        size * pixels_per_point
    };
    Some(format!("{}px", pixels.round().max(1.0)))
}

/// The weight of `font` as a CSS `font-weight`, unless it is normal.
fn font_weight(font: &pango::FontDescription) -> Option<i32> {
    let weight = font.weight();
    let set = font.set_fields().contains(pango::FontMask::WEIGHT);
    (set && weight != pango::Weight::Normal).then(|| weight.into_glib().clamp(1, 1000))
}

/// The style of `font` as a CSS `font-style`, unless it is upright.
fn font_style(font: &pango::FontDescription) -> Option<&'static str> {
    match font.style() {
        pango::Style::Italic => Some("italic"),
        pango::Style::Oblique => Some("oblique"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(description: &str) -> Option<pango::FontDescription> {
        Some(pango::FontDescription::from_string(description))
    }

    /// The system fonts of a default GNOME session at 96 dpi.
    fn system() -> SystemFonts {
        SystemFonts {
            default: font("Adwaita Sans 11"),
            monospace: font("Adwaita Mono 11"),
            pixels_per_point: 96.0 / 72.0,
        }
    }

    fn sheet(preferences: &FontPreferences, surface: Surface) -> String {
        preferences.stylesheet(surface, &system())
    }

    #[test]
    fn system_font_adds_nothing() {
        let preferences = FontPreferences::new(true, "Serif 14", false);
        assert_eq!(preferences, FontPreferences::default());
        assert_eq!(sheet(&preferences, Surface::System), "");
        assert_eq!(sheet(&preferences, Surface::Paper), "");
    }

    #[test]
    fn empty_font_is_the_system_font() {
        assert_eq!(FontPreferences::new(false, "", false).custom, None);
        assert_eq!(FontPreferences::new(false, "  ", true).custom, None);
    }

    #[test]
    fn chosen_font_is_the_default_for_both_surfaces() {
        let preferences = FontPreferences::new(false, "Source Serif 4 14", false);
        let expected = ":where(:root) { font-family: \"Source Serif 4\"; font-size: 19px; }\n";
        assert_eq!(sheet(&preferences, Surface::System), expected);
        assert_eq!(sheet(&preferences, Surface::Paper), expected);
    }

    #[test]
    fn chosen_face_keeps_its_weight_and_style() {
        let preferences = FontPreferences::new(false, "Cantarell Bold Italic 12", false);
        assert_eq!(
            sheet(&preferences, Surface::Paper),
            ":where(:root) { font-family: \"Cantarell\"; font-size: 16px; \
             font-weight: 700; font-style: italic; }\n"
        );
    }

    #[test]
    fn monospace_uses_the_system_monospace_font() {
        let preferences = FontPreferences::new(true, "", true);
        assert_eq!(
            sheet(&preferences, Surface::System),
            ":where(:root) { font-family: \"Adwaita Mono\", monospace; font-size: 15px; }\n"
        );
        // HTML Bodies are left alone.
        assert_eq!(sheet(&preferences, Surface::Paper), "");
    }

    #[test]
    fn monospace_takes_the_chosen_size() {
        let preferences = FontPreferences::new(false, "Serif Italic 14", true);
        assert_eq!(
            sheet(&preferences, Surface::System),
            ":where(:root) { font-family: \"Adwaita Mono\", monospace; font-size: 19px; }\n"
        );
        assert_eq!(
            sheet(&preferences, Surface::Paper),
            ":where(:root) { font-family: serif; font-size: 19px; font-style: italic; }\n"
        );
    }

    #[test]
    fn monospace_without_a_chosen_size_or_system_monospace_font() {
        let preferences = FontPreferences::new(false, "Serif", true);
        let system = SystemFonts {
            monospace: None,
            ..system()
        };
        assert_eq!(
            preferences.stylesheet(Surface::System, &system),
            ":where(:root) { font-family: monospace; font-size: 15px; }\n"
        );
        assert_eq!(
            preferences.stylesheet(Surface::Paper, &system),
            ":where(:root) { font-family: serif; }\n"
        );
        let system = SystemFonts {
            default: None,
            monospace: font("Monospace"),
            ..system
        };
        assert_eq!(
            preferences.stylesheet(Surface::System, &system),
            ":where(:root) { font-family: monospace; }\n"
        );
    }

    #[test]
    fn generic_families_become_css_generic_families() {
        let families = |description: &str| font_families(&font(description).unwrap());
        assert_eq!(families("Sans 11").as_deref(), Some("sans-serif"));
        assert_eq!(families("serif 11").as_deref(), Some("serif"));
        assert_eq!(families("Monospace 11").as_deref(), Some("monospace"));
        assert_eq!(
            families("Cantarell,Sans 11").as_deref(),
            Some("\"Cantarell\", sans-serif")
        );
        assert_eq!(families("11"), None);
    }

    #[test]
    fn family_names_cannot_break_out_of_the_style_sheet() {
        assert_eq!(css_string("Noto Sans CJK JP"), "\"Noto Sans CJK JP\"");
        assert_eq!(css_string("Fira_Code-Retina"), "\"Fira_Code-Retina\"");
        assert_eq!(css_string("Ålesund Sans"), "\"Ålesund Sans\"");
        assert_eq!(
            css_string("x\"; } body { color: red; } \\"),
            "\"x\\22 \\3b  \\7d  body \\7b  color\\3a  red\\3b  \\7d  \\5c \""
        );
        assert_eq!(
            css_string("a\nb</style>\0"),
            "\"a\\a b\\3c \\2f style\\3e \""
        );
        // Through a whole font description as well.
        let preferences = FontPreferences::new(false, "Evil\"} * {color:red} 12", false);
        let css = sheet(&preferences, Surface::Paper);
        assert_eq!(css.matches(['{', '}']).count(), 2, "{css}");
        assert_eq!(css.matches('"').count(), 2, "{css}");
    }

    #[test]
    fn sizes_follow_the_font_resolution() {
        let size = |description: &str, pixels_per_point| {
            font_size(&font(description).unwrap(), pixels_per_point)
        };
        assert_eq!(size("Sans 11", 96.0 / 72.0).as_deref(), Some("15px"));
        assert_eq!(size("Sans 10.5", 96.0 / 72.0).as_deref(), Some("14px"));
        // Large text in GNOME's accessibility settings scales by 1.25.
        assert_eq!(size("Sans 11", 120.0 / 72.0).as_deref(), Some("18px"));
        assert_eq!(size("Sans 20px", 2.0).as_deref(), Some("20px"));
        assert_eq!(size("Sans", 96.0 / 72.0), None);
    }
}
