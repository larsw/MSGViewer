//! The Body of a Message, shown in a locked-down web view (ADR-0004).
//!
//! JavaScript is off, the view never navigates away from the Body it was
//! given, links open in the browser through the OpenURI portal, and nothing
//! is fetched from the network unless the reader allows Remote content for
//! the Message shown.
//!
//! Remote content is kept out three ways, so that no single gap lets it
//! through: a content filter blocks every load from a server
//! (`remote_content.rs`), a Content Security Policy is put first in the
//! Body (`html.rs`), and the web view's settings turn off DNS prefetching
//! and connecting to servers ahead of time. Allowing Remote content lifts
//! the filter and loosens the policy for that Message only, and is never
//! remembered.

mod html;
mod plain_text;
mod remote_content;
mod style;
mod zoom;

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib, pango};
use outlook_msg::{Body, Message};
use webkit6::prelude::*;
use webkit6::{
    ContextMenuAction, NavigationPolicyDecision, NavigationType, PolicyDecision,
    PolicyDecisionType, ResponsePolicyDecision, UserContentFilter, UserContentInjectedFrames,
    UserStyleLevel, UserStyleSheet,
};

use html::RemoteContent;
use style::Surface;

/// The address of every Body this widget loads.
const BODY_URI: &str = "about:blank";

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/body_view.ui")]
    pub struct BodyView {
        #[template_child]
        pub remote_content_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub formatting_note: TemplateChild<gtk::Box>,
        #[template_child]
        pub web_view: TemplateChild<webkit6::WebView>,
        /// The HTML Body shown, as the Message has it.
        pub html: RefCell<Option<String>>,
        /// Whether the reader allowed Remote content for the Message shown.
        pub remote_content: Cell<RemoteContent>,
        /// The content filter blocking Remote content, once compiled; `None`
        /// until then, or if it can't be.
        pub filter: RefCell<Option<UserContentFilter>>,
        /// Whether the content filter is in place, or known not to be
        /// available. Nothing is loaded before then.
        pub filter_ready: Cell<bool>,
        /// What to load once the content filter is ready.
        pub pending_load: RefCell<Option<(String, Surface, RemoteContent)>>,
        /// What the Body being shown is drawn on.
        pub surface: Cell<Surface>,
        /// The style sheet giving the Body its colours.
        pub style_sheet: RefCell<Option<UserStyleSheet>>,
        /// Set from loading a Body until the web view commits to showing it.
        /// Only then is navigating to `BODY_URI` allowed.
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
            klass.install_action("body.load-remote-content", None, |view, _, _| {
                view.load_remote_content();
            });
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
    /// Shows the Body of `message`, or nothing if it has none, with a note
    /// when it is Genuine RTF whose formatting isn't shown. Remote content
    /// is blocked again, whatever the reader allowed before.
    pub fn set_message(&self, message: &Message) {
        let imp = self.imp();
        imp.remote_content.set(RemoteContent::Blocked);
        let body = message.body();
        imp.formatting_note
            .set_visible(matches!(body, Some(Body::GenuineRtf(_))));
        let html = match body {
            Some(Body::Html(html)) => Some(html),
            Some(Body::PlainText(text) | Body::GenuineRtf(text)) => {
                let html = plain_text::to_html(&text);
                self.load(html, Surface::System, RemoteContent::Blocked);
                None
            }
            None => {
                self.load(String::new(), Surface::System, RemoteContent::Blocked);
                None
            }
        };
        imp.remote_content_banner
            .set_revealed(html.as_deref().is_some_and(html::references_remote_content));
        imp.html.replace(html);
        self.show_html();
    }

    /// Reloads the HTML Body with Remote content allowed. Only the Message
    /// shown is affected, and nothing is remembered.
    fn load_remote_content(&self) {
        let imp = self.imp();
        if imp.html.borrow().is_none() {
            return;
        }
        imp.remote_content.set(RemoteContent::Allowed);
        imp.remote_content_banner.set_revealed(false);
        self.show_html();
    }

    /// Loads the HTML Body, if the Message has one, on the paper surface.
    fn show_html(&self) {
        let imp = self.imp();
        let remote = imp.remote_content.get();
        let html = imp
            .html
            .borrow()
            .as_deref()
            .map(|body| html::prepare(body, remote));
        if let Some(html) = html {
            self.load(html, Surface::Paper, remote);
        }
    }

    /// Loads `html` as the Body, drawn on `surface`, with Remote content
    /// blocked or not. Waits for the content filter first.
    fn load(&self, html: String, surface: Surface, remote: RemoteContent) {
        let imp = self.imp();
        if !imp.filter_ready.get() {
            imp.pending_load.replace(Some((html, surface, remote)));
            return;
        }
        if let (Some(filter), Some(content_manager)) =
            (&*imp.filter.borrow(), imp.web_view.user_content_manager())
        {
            content_manager.remove_filter(filter);
            if remote == RemoteContent::Blocked {
                content_manager.add_filter(filter);
            }
        }
        imp.surface.set(surface);
        self.update_style();
        imp.loading_body.set(true);
        imp.web_view.load_html(&html, Some(BODY_URI));
    }

    /// Puts the content filter blocking Remote content in place, and loads
    /// whatever was waiting for it.
    fn set_filter(&self, filter: Option<UserContentFilter>) {
        let imp = self.imp();
        if filter.is_none() {
            glib::g_warning!(
                "msg-viewer",
                "No content filter; Remote content is kept out by the Body's \
                 Content Security Policy and the web view's settings only"
            );
        }
        imp.filter.replace(filter);
        imp.filter_ready.set(true);
        if let Some((html, surface, remote)) = imp.pending_load.take() {
            self.load(html, surface, remote);
        }
    }

    fn setup_web_view(&self) {
        let imp = self.imp();
        let web_view = &*imp.web_view;
        web_view.set_settings(&locked_down_settings());
        remote_content::with_blocking_filter(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |filter| view.set_filter(filter)
        ));

        let view = self.downgrade();
        web_view.connect_decide_policy(move |_, decision, kind| {
            match view.upgrade() {
                Some(view) => view.decide_policy(decision, kind),
                None => decision.ignore(),
            }
            true
        });
        web_view.connect_load_changed(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |_, event| {
                if event == webkit6::LoadEvent::Committed {
                    view.imp().loading_body.set(false);
                }
            }
        ));
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
                    && self.imp().loading_body.get()
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

/// Finding text in the Body, and zooming it.
impl BodyView {
    /// Finds `text` in the Body, ignoring case, and highlights the matches.
    /// Empty text ends the search.
    pub fn find(&self, text: &str) {
        let Some(controller) = self.find_controller() else {
            return;
        };
        if text.is_empty() {
            controller.search_finish();
            return;
        }
        let options = webkit6::FindOptions::CASE_INSENSITIVE | webkit6::FindOptions::WRAP_AROUND;
        controller.search(text, options.bits(), u32::MAX);
    }

    /// Moves to the next match, wrapping around at the end.
    pub fn find_next(&self) {
        if let Some(controller) = self.find_controller() {
            controller.search_next();
        }
    }

    /// Moves to the previous match, wrapping around at the start.
    pub fn find_previous(&self) {
        if let Some(controller) = self.find_controller() {
            controller.search_previous();
        }
    }

    /// Ends the search and removes its highlights.
    pub fn find_finish(&self) {
        if let Some(controller) = self.find_controller() {
            controller.search_finish();
        }
    }

    /// Calls `callback` after each search with whether the text was found.
    pub fn connect_find_result<F: Fn(bool) + Clone + 'static>(&self, callback: F) {
        let Some(controller) = self.find_controller() else {
            return;
        };
        let found = callback.clone();
        controller.connect_found_text(move |_, _| found(true));
        controller.connect_failed_to_find_text(move |_| callback(false));
    }

    pub fn zoom_in(&self) {
        let web_view = &self.imp().web_view;
        web_view.set_zoom_level(zoom::larger(web_view.zoom_level()));
    }

    pub fn zoom_out(&self) {
        let web_view = &self.imp().web_view;
        web_view.set_zoom_level(zoom::smaller(web_view.zoom_level()));
    }

    pub fn zoom_reset(&self) {
        self.imp().web_view.set_zoom_level(zoom::DEFAULT);
    }

    fn find_controller(&self) -> Option<webkit6::FindController> {
        self.imp().web_view.find_controller()
    }
}

/// WebKit features that connect to servers without asking the Body's Content
/// Security Policy: `<link rel=preconnect>` opens connections, and the
/// others fetch pages ahead of time.
const NETWORK_FEATURES: [&str; 4] = [
    "LinkPreconnect",
    "LinkPreconnectEarlyHints",
    "LinkPrefetch",
    "SpeculationRulesPrefetch",
];

/// Web view settings for showing a Body: no JavaScript, no plugins, media or
/// storage, nothing fetched or resolved ahead of time, and the same font as
/// the rest of the window.
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
    settings.set_enable_dns_prefetching(false);
    settings.set_enable_hyperlink_auditing(false);
    if let Some(features) = webkit6::Settings::all_features() {
        for feature in (0..features.length()).filter_map(|i| features.get(i)) {
            let identifier = feature.identifier().unwrap_or_default();
            if NETWORK_FEATURES.contains(&identifier.as_str()) {
                settings.set_feature_enabled(&feature, false);
            }
        }
    }

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
