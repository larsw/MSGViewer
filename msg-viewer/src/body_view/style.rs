//! Colours a Body is drawn with.

use gtk::gdk;

/// What a Body is drawn on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surface {
    /// The view background of the system light or dark style, for plain-text
    /// Bodies. The web view is transparent and the widget behind it draws
    /// the background, so only text colours are set here.
    #[default]
    System,
    /// A light "paper" surface whatever the system style, because email HTML
    /// assumes a white background (ADR-0004). For HTML Bodies.
    #[allow(dead_code)]
    Paper,
}

impl Surface {
    /// The colour the web view fills in before the Body draws anything.
    pub fn background(self) -> gdk::RGBA {
        match self {
            Surface::System => gdk::RGBA::TRANSPARENT,
            Surface::Paper => gdk::RGBA::WHITE,
        }
    }

    /// A user style sheet giving the Body its colours. It sits at author
    /// level ahead of the Body's own styles, so HTML Bodies can override it.
    ///
    /// On the system surface, the text and link colours match libadwaita's
    /// `view_fg_color` and `accent_color` (derived from `accent`, the accent
    /// background colour), so the Body looks like the rest of the window.
    pub fn stylesheet(self, system_dark: bool, accent: &gdk::RGBA) -> String {
        let accent = format!(
            "rgb({} {} {})",
            channel(accent.red()),
            channel(accent.green()),
            channel(accent.blue())
        );
        match self {
            Surface::Paper => {
                ":root { color-scheme: light; background: white; color: black; }\n".to_owned()
            }
            Surface::System if system_dark => format!(
                ":root {{ color-scheme: dark; color: white; }}\n\
                 :link, :visited {{ color: oklab(from {accent} max(l, 0.85) a b); }}\n"
            ),
            Surface::System => format!(
                ":root {{ color-scheme: light; color: rgb(0 0 6 / 80%); }}\n\
                 :link, :visited {{ color: oklab(from {accent} min(l, 0.5) a b); }}\n"
            ),
        }
    }
}

/// A colour channel as a CSS integer from 0 to 255.
fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUE: gdk::RGBA = gdk::RGBA::new(0.208, 0.518, 0.894, 1.0);

    #[test]
    fn system_surface_follows_dark_style() {
        let dark = Surface::System.stylesheet(true, &BLUE);
        assert!(dark.contains("color-scheme: dark"));
        assert!(dark.contains("oklab(from rgb(53 132 228) max(l, 0.85) a b)"));
        let light = Surface::System.stylesheet(false, &BLUE);
        assert!(light.contains("color-scheme: light"));
        assert!(light.contains("oklab(from rgb(53 132 228) min(l, 0.5) a b)"));
        assert_eq!(Surface::System.background(), gdk::RGBA::TRANSPARENT);
    }

    #[test]
    fn paper_surface_stays_light() {
        let dark = Surface::Paper.stylesheet(true, &BLUE);
        assert!(dark.contains("color-scheme: light"));
        assert_eq!(dark, Surface::Paper.stylesheet(false, &BLUE));
        assert_eq!(Surface::Paper.background(), gdk::RGBA::WHITE);
    }
}
