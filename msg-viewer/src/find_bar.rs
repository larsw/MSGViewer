use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use crate::body_view::BodyView;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/find_bar.ui")]
    pub struct FindBar {
        #[template_child]
        pub search_bar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        pub entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub no_results: TemplateChild<gtk::Label>,
        /// The Body searched.
        pub body_view: glib::WeakRef<BodyView>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FindBar {
        const NAME: &'static str = "MsgViewerFindBar";
        type Type = super::FindBar;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.install_action("find.close", None, |bar, _, _| bar.close());
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for FindBar {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            // Escape in the entry closes the bar.
            self.search_bar.connect_entry(&*self.entry);
            self.search_bar
                .connect_search_mode_enabled_notify(glib::clone!(
                    #[weak]
                    obj,
                    move |search_bar| {
                        if !search_bar.is_search_mode() {
                            obj.finish();
                        }
                    }
                ));
            self.entry.connect_search_changed(glib::clone!(
                #[weak]
                obj,
                move |_| obj.search()
            ));
            self.entry.connect_activate(glib::clone!(
                #[weak]
                obj,
                move |_| obj.find_next()
            ));
            self.entry.connect_next_match(glib::clone!(
                #[weak]
                obj,
                move |_| obj.find_next()
            ));
            self.entry.connect_previous_match(glib::clone!(
                #[weak]
                obj,
                move |_| obj.find_previous()
            ));
        }
    }

    impl WidgetImpl for FindBar {}
    impl BinImpl for FindBar {}
}

glib::wrapper! {
    /// A bar for finding text in the Body, shown below the header bar.
    pub struct FindBar(ObjectSubclass<imp::FindBar>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FindBar {
    /// Makes the bar search `body_view`.
    pub fn set_body_view(&self, body_view: &BodyView) {
        self.imp().body_view.set(Some(body_view));
        body_view.connect_find_result(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |found| bar.show_result(found)
        ));
    }

    /// Shows the bar with its text selected, ready to type.
    pub fn open(&self) {
        let imp = self.imp();
        if !imp.search_bar.is_search_mode() {
            imp.search_bar.set_search_mode(true);
            // Closing ended the last search; find its text again.
            self.search();
        }
        imp.entry.grab_focus();
        imp.entry.select_region(0, -1);
    }

    pub fn close(&self) {
        self.imp().search_bar.set_search_mode(false);
    }

    /// Moves to the next match, or opens the bar if it is closed.
    pub fn find_next(&self) {
        if !self.imp().search_bar.is_search_mode() {
            self.open();
        } else if let Some(body_view) = self.imp().body_view.upgrade() {
            body_view.find_next();
        }
    }

    /// Moves to the previous match, or opens the bar if it is closed.
    pub fn find_previous(&self) {
        if !self.imp().search_bar.is_search_mode() {
            self.open();
        } else if let Some(body_view) = self.imp().body_view.upgrade() {
            body_view.find_previous();
        }
    }

    /// Finds the entry's text in the Body.
    fn search(&self) {
        let text = self.imp().entry.text();
        if let Some(body_view) = self.imp().body_view.upgrade() {
            body_view.find(&text);
        }
        if text.is_empty() {
            self.show_result(true);
        }
    }

    fn finish(&self) {
        if let Some(body_view) = self.imp().body_view.upgrade() {
            body_view.find_finish();
        }
        self.show_result(true);
    }

    /// Shows whether the text was found. Results that arrive after the
    /// search ended are ignored.
    fn show_result(&self, found: bool) {
        let imp = self.imp();
        let searching = imp.search_bar.is_search_mode() && !imp.entry.text().is_empty();
        let no_results = searching && !found;
        imp.no_results.set_visible(no_results);
        if no_results {
            imp.entry.add_css_class("error");
        } else {
            imp.entry.remove_css_class("error");
        }
    }
}
