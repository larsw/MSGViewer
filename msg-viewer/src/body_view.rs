//! The Body of a Message, shown in a locked-down web view (ADR-0004).
//!
//! JavaScript is off, the view never navigates away from the Body it was
//! given, links open in the browser through the OpenURI portal, and nothing
//! is fetched from the network.

mod plain_text;
mod style;

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib, pango};
use outlook_msg::{Body, Message};
use webkit6::prelude::*;
use webkit6::{
    ContextMenuAction, NavigationPolicyDecision, NavigationType, PolicyDecision,
    PolicyDecisionType, ResponsePolicyDecision, UserContentInjectedFrames, UserStyleLevel,
    UserStyleSheet,
};

use style::Surface;

/// The address of every Body this widget loads.
const BODY_URI: &str = "about:blank";

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/body_view.ui")]
    pub struct BodyView {
        #[template_child]
        pub web_view: TemplateChild<webkit6::WebView>,
        /// What the Body being shown is drawn on.
        pub surface: Cell<Surface>,
        /// The style sheet giving the Body its colours.
        pub style_sheet: RefCell<Option<UserStyleSheet>>,
        /// Set from loading a Body until WebKit asks whether to navigate to it.
        pub loading_body: Cell<bool>,
        pub style_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BodyView {
        const NAME: &'static str = "MsgViewerBodyView";
        type Type = super::BodyView;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            webkit6::WebView::ensure_type();
            webkit6::NetworkSession::ensure_type();
            webkit6::UserContentManager::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for BodyView {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup_web_view();
        }

        fn dispose(&self) {
            let style_manager = adw::StyleManager::default();
            for handler in self.style_handlers.take() {
                style_manager.disconnect(handler);
            }
        }
    }

    impl WidgetImpl for BodyView {}
    impl BoxImpl for BodyView {}
}

glib::wrapper! {
    /// The Body of a Message.
    pub struct BodyView(ObjectSubclass<imp::BodyView>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl BodyView {
    /// Shows the Body of `message`, or nothing if it has none.
    pub fn set_message(&self, message: &Message) {
        match message.body() {
            Some(Body::PlainText(text)) => self.load(&plain_text::to_html(&text), Surface::System),
            None => self.load("", Surface::System),
        }
    }

    /// Loads `html` as the Body, drawn on `surface`.
    fn load(&self, html: &str, surface: Surface) {
        let imp = self.imp();
        imp.surface.set(surface);
        self.update_style();
        imp.loading_body.set(true);
        imp.web_view.load_html(html, Some(BODY_URI));
    }

    fn setup_web_view(&self) {
        let imp = self.imp();
        let web_view = &*imp.web_view;
        web_view.set_settings(&locked_down_settings());

        let view = self.downgrade();
        web_view.connect_decide_policy(move |_, decision, kind| {
            match view.upgrade() {
                Some(view) => view.decide_policy(decision, kind),
                None => decision.ignore(),
            }
            true
        });
        web_view.connect_context_menu(|_, menu, _| {
            trim_context_menu(menu);
            // Returning true shows no menu at all.
            menu.n_items() == 0
        });
        if let Some(session) = web_view.network_session() {
            session.connect_download_started(|_, download| download.cancel());
        }
        web_view.connect_web_process_terminated(|_, reason| {
            glib::g_warning!("msg-viewer", "The Body's web process stopped: {reason:?}");
        });

        // Plain-text Bodies follow the system style as it changes.
        let style_manager = adw::StyleManager::default();
        let update_style = glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |_: &adw::StyleManager| view.update_style()
        );
        imp.style_handlers.replace(vec![
            style_manager.connect_dark_notify(update_style.clone()),
            style_manager.connect_accent_color_rgba_notify(update_style),
        ]);
        self.update_style();
    }

    /// Gives the Body the colours of its surface and the current system style.
    fn update_style(&self) {
        let imp = self.imp();
        let Some(content_manager) = imp.web_view.user_content_manager() else {
            return;
        };
        let surface = imp.surface.get();
        let style_manager = adw::StyleManager::default();
        let style_sheet = UserStyleSheet::new(
            &surface.stylesheet(style_manager.is_dark(), &style_manager.accent_color_rgba()),
            UserContentInjectedFrames::TopFrame,
            UserStyleLevel::Author,
            &[],
            &[],
        );
        if let Some(old) = imp.style_sheet.replace(Some(style_sheet.clone())) {
            content_manager.remove_style_sheet(&old);
        }
        content_manager.add_style_sheet(&style_sheet);
        imp.web_view.set_background_color(&surface.background());
    }

    /// Allows only the Body this widget loaded itself. Everything else is
    /// cancelled, and clicked links open in the browser instead.
    fn decide_policy(&self, decision: &PolicyDecision, kind: PolicyDecisionType) {
        match kind {
            PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction => {
                let Some(action) = decision
                    .downcast_ref::<NavigationPolicyDecision>()
                    .and_then(|decision| decision.navigation_action())
                else {
                    decision.ignore();
                    return;
                };
                let uri = action
                    .request()
                    .and_then(|request| request.uri())
                    .unwrap_or_default();
                let navigation_type = action.navigation_type();
                if kind == PolicyDecisionType::NavigationAction
                    && navigation_type == NavigationType::Other
                    && uri == BODY_URI
                    && self.imp().loading_body.replace(false)
                {
                    decision.use_();
                    return;
                }
                decision.ignore();
                if navigation_type == NavigationType::LinkClicked {
                    self.open_link(&uri);
                }
            }
            PolicyDecisionType::Response => {
                // Show what WebKit can display, and never download anything.
                let displayable = decision
                    .downcast_ref::<ResponsePolicyDecision>()
                    .is_some_and(|decision| decision.is_mime_type_supported());
                if displayable {
                    decision.use_();
                } else {
                    decision.ignore();
                }
            }
            _ => decision.ignore(),
        }
    }

    /// Opens a link from the Body in the browser or mail client.
    fn open_link(&self, uri: &str) {
        if !is_external_link(uri) {
            return;
        }
        let window = self.root().and_downcast::<gtk::Window>();
        gtk::UriLauncher::new(uri).launch(window.as_ref(), gio::Cancellable::NONE, |result| {
            if let Err(error) = result {
                glib::g_warning!("msg-viewer", "Could not open link: {error}");
            }
        });
    }
}

/// Web view settings for showing a Body: no JavaScript, no plugins, media or
/// storage, and the same font as the rest of the window.
fn locked_down_settings() -> webkit6::Settings {
    let settings = webkit6::Settings::new();
    settings.set_enable_javascript(false);
    settings.set_enable_javascript_markup(false);
    settings.set_javascript_can_open_windows_automatically(false);
    settings.set_javascript_can_access_clipboard(false);
    settings.set_allow_modal_dialogs(false);
    settings.set_enable_developer_extras(false);
    settings.set_enable_back_forward_navigation_gestures(false);
    settings.set_enable_page_cache(false);
    settings.set_enable_html5_database(false);
    settings.set_enable_html5_local_storage(false);
    settings.set_enable_media(false);
    settings.set_enable_webaudio(false);
    settings.set_enable_webgl(false);
    settings.set_enable_webrtc(false);
    settings.set_enable_fullscreen(false);
    settings.set_enable_site_specific_quirks(false);

    let font = gtk::Settings::default()
        .and_then(|gtk_settings| gtk_settings.gtk_font_name())
        .map(|name| pango::FontDescription::from_string(&name));
    if let Some(font) = font {
        if let Some(family) = font.family() {
            settings.set_default_font_family(&family);
        }
        let size = font.size() / pango::SCALE;
        if size > 0 {
            let pixels = if font.is_size_absolute() {
                size as u32
            } else {
                webkit6::Settings::font_size_to_pixels(size as u32)
            };
            settings.set_default_font_size(pixels);
        }
    }
    settings
}

/// Removes everything from the web view's context menu except copying.
fn trim_context_menu(menu: &webkit6::ContextMenu) {
    for item in menu.items() {
        let keep = matches!(
            item.stock_action(),
            ContextMenuAction::Copy
                | ContextMenuAction::CopyLinkToClipboard
                | ContextMenuAction::SelectAll
        );
        if !keep {
            menu.remove(&item);
        }
    }
}

/// Whether a link from a Body may be opened outside the app.
fn is_external_link(uri: &str) -> bool {
    glib::Uri::peek_scheme(uri)
        .is_some_and(|scheme| matches!(scheme.as_str(), "http" | "https" | "mailto"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_only_web_and_mail_links() {
        assert!(is_external_link("https://example.org/"));
        assert!(is_external_link("HTTP://example.org/"));
        assert!(is_external_link("mailto:ada@example.org"));
        assert!(!is_external_link("file:///etc/passwd"));
        assert!(!is_external_link("javascript:alert(1)"));
        assert!(!is_external_link("about:blank"));
        assert!(!is_external_link("cid:image001.png"));
        assert!(!is_external_link("not a link"));
    }
}
