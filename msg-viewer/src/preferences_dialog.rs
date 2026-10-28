//! Preferences: the font Bodies are shown in. Every row is bound to the
//! app's settings, so changes are saved and reach every window at once.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib, pango};

use crate::body_view;
use crate::settings;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/preferences_dialog.ui")]
    pub struct PreferencesDialog {
        #[template_child]
        pub use_system_font_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub font_button: TemplateChild<gtk::FontDialogButton>,
        #[template_child]
        pub monospace_row: TemplateChild<adw::SwitchRow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferencesDialog {
        const NAME: &'static str = "MsgViewerPreferencesDialog";
        type Type = super::PreferencesDialog;
        type ParentType = adw::PreferencesDialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PreferencesDialog {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().bind_settings(&settings::new());
        }
    }

    impl WidgetImpl for PreferencesDialog {}
    impl AdwDialogImpl for PreferencesDialog {}
    impl PreferencesDialogImpl for PreferencesDialog {}
}

glib::wrapper! {
    pub struct PreferencesDialog(ObjectSubclass<imp::PreferencesDialog>)
        @extends adw::PreferencesDialog, adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl PreferencesDialog {
    pub fn new() -> Self {
        glib::Object::new()
    }

    fn bind_settings(&self, settings: &gio::Settings) {
        let imp = self.imp();
        settings
            .bind(
                settings::USE_SYSTEM_FONT,
                &*imp.use_system_font_row,
                "active",
            )
            .build();
        settings
            .bind(
                settings::MONOSPACE_PLAIN_TEXT,
                &*imp.monospace_row,
                "active",
            )
            .build();
        // Until the reader picks a font, the button shows the system font.
        settings
            .bind(settings::BODY_FONT, &*imp.font_button, "font-desc")
            .mapping(|variant, _| {
                let font = variant.str().filter(|font| !font.trim().is_empty());
                let font = match font {
                    Some(font) => pango::FontDescription::from_string(font),
                    None => body_view::system_font()?,
                };
                Some(font.to_value())
            })
            .set_mapping(|value, _| {
                let font = value.get::<Option<pango::FontDescription>>().ok()??;
                Some(font.to_str().to_variant())
            })
            .build();
    }
}

impl Default for PreferencesDialog {
    fn default() -> Self {
        Self::new()
    }
}
