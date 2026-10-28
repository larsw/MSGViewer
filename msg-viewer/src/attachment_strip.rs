use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};
use outlook_msg::{Attachment, Message};

use crate::application::Application;
use crate::attachment_open::{self, TempCopies};
use crate::attachment_policy;
use crate::attachment_save;
use crate::body_view::inline;
use crate::window::Window;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/attachment_strip.ui")]
    pub struct AttachmentStrip {
        #[template_child]
        pub count: TemplateChild<gtk::Label>,
        #[template_child]
        pub chips: TemplateChild<gtk::FlowBox>,
        /// The Attachments shown, indexed by the `attachments.open` and
        /// `attachments.save` targets.
        pub attachments: RefCell<Vec<Attachment>>,
        /// Copies of the Attachments opened in other apps, removed with the
        /// window.
        pub copies: TempCopies,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AttachmentStrip {
        const NAME: &'static str = "MsgViewerAttachmentStrip";
        type Type = super::AttachmentStrip;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.install_action_async(
                "attachments.open",
                Some(glib::VariantTy::UINT32),
                |strip, _, index| async move {
                    if let Some(index) = index.and_then(|index| index.get::<u32>()) {
                        strip.open(index as usize, Launch::ChooseApp).await;
                    }
                },
            );
            klass.install_action_async(
                "attachments.save",
                Some(glib::VariantTy::UINT32),
                |strip, _, index| async move {
                    if let Some(index) = index.and_then(|index| index.get::<u32>()) {
                        strip.save(index as usize).await;
                    }
                },
            );
            klass.install_action_async("attachments.save-all", None, |strip, _, _| async move {
                strip.save_all().await;
            });
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AttachmentStrip {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().action_set_enabled("attachments.save-all", false);
        }
    }

    impl WidgetImpl for AttachmentStrip {}
    impl BoxImpl for AttachmentStrip {}
}

/// How an Attachment opened in another app is launched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Launch {
    /// The reader chooses the app, having chosen Open.
    ChooseApp,
    /// In the reader's default app for its type, from a single click.
    DefaultApp,
}

glib::wrapper! {
    /// The Attachments of a Message as a wrapping strip of chips, each with
    /// a menu to open or save it, plus Save All. Hidden when there are none.
    ///
    /// Clicking a chip opens images, PDFs and plain text in the reader's
    /// default app, and Embedded messages in a new window; any other chip
    /// shows its menu. The menu is also on secondary click, long press, and
    /// the Menu key or Shift+F10.
    pub struct AttachmentStrip(ObjectSubclass<imp::AttachmentStrip>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl AttachmentStrip {
    pub fn set_message(&self, message: &Message) {
        let imp = self.imp();
        // Inline attachments show in the Body instead.
        let inline = inline::inline_attachments(message);
        let attachments: Vec<Attachment> = message
            .attachments()
            .iter()
            .enumerate()
            .filter(|(index, _)| !inline.contains(index))
            .map(|(_, attachment)| attachment.clone())
            .collect();

        imp.chips.remove_all();
        for (index, attachment) in attachments.iter().enumerate() {
            imp.chips.append(&chip(self, attachment, index as u32));
        }
        let count = attachments.len();
        imp.count.set_label(
            &ngettext("{count} Attachment", "{count} Attachments", count as u32)
                .replace("{count}", &count.to_string()),
        );
        self.set_visible(!attachments.is_empty());
        self.action_set_enabled(
            "attachments.save-all",
            attachments.iter().any(|a| a.data().is_some()),
        );
        imp.attachments.replace(attachments);
    }

    /// What a single click on the chip of Attachment `index` does.
    fn click(&self, index: usize, menu: &gtk::PopoverMenu) {
        let opens = self
            .imp()
            .attachments
            .borrow()
            .get(index)
            .is_some_and(opens_on_click);
        if opens {
            let strip = self.clone();
            glib::spawn_future_local(async move {
                strip.open(index, Launch::DefaultApp).await;
            });
        } else {
            menu.popup();
        }
    }

    /// Opens one Attachment: an Embedded message in a new window, a file in
    /// another app, after a warning if it is a Risky attachment.
    async fn open(&self, index: usize, launch: Launch) {
        let embedded = self
            .imp()
            .attachments
            .borrow()
            .get(index)
            .filter(|attachment| attachment.is_embedded_message())
            .cloned();
        if let Some(attachment) = embedded {
            self.open_embedded(attachment);
            return;
        }
        let Some((name, data)) = self
            .imp()
            .attachments
            .borrow()
            .get(index)
            .and_then(|a| Some((a.safe_file_name(), glib::Bytes::from(a.data()?))))
        else {
            return;
        };
        if attachment_policy::is_risky(&name, &data) && !self.confirm_risky(&name).await {
            return;
        }
        let result = match self.imp().copies.write(&name, data).await {
            Ok(file) => {
                let ask = launch == Launch::ChooseApp;
                attachment_open::launch(&file, self.window().as_ref(), ask).await
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(()) => {}
            // FileLauncher reports the reader cancelling the portal's app
            // chooser as cancelled, not dismissed.
            Err(error)
                if error.matches(gtk::DialogError::Dismissed)
                    || error.matches(gtk::DialogError::Cancelled) => {}
            Err(error) => self.show_error(&gettext("Could Not Open Attachment"), &error),
        }
    }

    /// Shows an Embedded message in a new window.
    fn open_embedded(&self, attachment: Attachment) {
        let Some(app) = self
            .window()
            .and_then(|window| window.application())
            .and_downcast::<Application>()
        else {
            return;
        };
        let window = Window::new(&app);
        window.present();
        window.open_embedded(attachment);
    }

    /// Warns that the Attachment named `name` can run code, and returns
    /// whether the reader still wants to open it.
    async fn confirm_risky(&self, name: &str) -> bool {
        let dialog = adw::AlertDialog::new(
            Some(&gettext("Open Risky Attachment?")),
            Some(
                // Translators: {name} is the Attachment's file name.
                &gettext(
                    "“{name}” is a kind of file that can run programs or change your computer \
                     when opened. Only open it if you trust the sender and expected this file.",
                )
                .replace("{name}", name),
            ),
        );
        dialog.add_responses(&[
            ("cancel", &gettext("_Cancel")),
            ("open", &gettext("_Open Anyway")),
        ]);
        dialog.set_response_appearance("open", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        dialog.choose_future(Some(self)).await == "open"
    }

    /// Asks where to save one Attachment and writes it there.
    async fn save(&self, index: usize) {
        let Some((name, data)) = self
            .imp()
            .attachments
            .borrow()
            .get(index)
            .and_then(|a| Some((a.safe_file_name(), glib::Bytes::from(a.data()?))))
        else {
            return;
        };
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Save Attachment"))
            .modal(true)
            .initial_name(name)
            .build();
        let Some(file) = choose(dialog.save_future(self.window().as_ref()).await) else {
            return;
        };
        if let Err(error) = attachment_save::save_to_file(&file, data).await {
            self.show_error(&gettext("Could Not Save Attachment"), &error);
        }
    }

    /// Asks for a folder and saves every Attachment into it. Like the strip,
    /// it leaves out Inline attachments, which are part of the Body.
    async fn save_all(&self) {
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Save All Attachments"))
            .accept_label(gettext("_Save"))
            .modal(true)
            .build();
        let Some(folder) = choose(dialog.select_folder_future(self.window().as_ref()).await) else {
            return;
        };
        // The action is only enabled while some Attachment has data.
        let attachments = self.savable();
        if let Err(error) = attachment_save::save_into_folder(&folder, &attachments).await {
            self.show_error(&gettext("Could Not Save Attachments"), &error);
        }
    }

    /// The safe file name and data of each Attachment that has data.
    fn savable(&self) -> Vec<(String, glib::Bytes)> {
        self.imp()
            .attachments
            .borrow()
            .iter()
            .filter_map(|attachment| {
                let data = attachment.data()?;
                Some((attachment.safe_file_name(), glib::Bytes::from(data)))
            })
            .collect()
    }

    fn window(&self) -> Option<gtk::Window> {
        self.root().and_downcast()
    }

    fn show_error(&self, heading: &str, error: &glib::Error) {
        let dialog = adw::AlertDialog::new(Some(heading), Some(error.message()));
        dialog.add_response("close", &gettext("_Close"));
        dialog.present(Some(self));
    }
}

/// The file the reader chose, or `None` if they cancelled.
fn choose(result: Result<gio::File, glib::Error>) -> Option<gio::File> {
    match result {
        Ok(file) => Some(file),
        Err(error) if error.matches(gtk::DialogError::Dismissed) => None,
        Err(error) => {
            glib::g_warning!("msg-viewer", "Could not choose a file: {error}");
            None
        }
    }
}

/// A chip for one Attachment of `strip`: icon, file name and size. Clicking
/// it opens the Attachment or shows its menu; see [`AttachmentStrip`].
fn chip(strip: &AttachmentStrip, attachment: &Attachment, index: u32) -> gtk::FlowBoxChild {
    let name = attachment.safe_file_name();
    let size = attachment
        .size()
        .map(|size| glib::format_size(size).to_string())
        .unwrap_or_default();

    let icon = gtk::Image::from_gicon(&icon(attachment, &name));
    icon.set_icon_size(gtk::IconSize::Large);

    let name_label = gtk::Label::builder()
        .label(&name)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .max_width_chars(24)
        .build();
    let size_label = gtk::Label::builder().label(&size).xalign(0.0).build();
    size_label.add_css_class("caption");
    size_label.add_css_class("dim-label");

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 0);
    labels.set_valign(gtk::Align::Center);
    labels.append(&name_label);
    labels.append(&size_label);

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    content.append(&icon);
    content.append(&labels);

    let menu = gio::Menu::new();
    let open = gio::MenuItem::new(Some(&gettext("_Open")), None);
    open.set_action_and_target_value(Some("attachments.open"), Some(&index.to_variant()));
    menu.append_item(&open);
    let save = gio::MenuItem::new(Some(&gettext("_Save…")), None);
    save.set_action_and_target_value(Some("attachments.save"), Some(&index.to_variant()));
    menu.append_item(&save);
    let popover = gtk::PopoverMenu::from_model(Some(&menu));

    let button = gtk::Button::builder()
        .child(&content)
        .tooltip_text(&name)
        .build();
    let accessible_label = match size.as_str() {
        "" => name.clone(),
        // Translators: How screen readers name an Attachment, e.g.
        // "report.pdf, 1.2 MB".
        size => gettext("{name}, {size}")
            // The name could contain "{size}", so it goes in last.
            .replace("{size}", size)
            .replace("{name}", &name),
    };
    button.update_property(&[
        gtk::accessible::Property::Label(&accessible_label),
        gtk::accessible::Property::HasPopup(true),
    ]);
    button.connect_clicked(glib::clone!(
        #[weak]
        strip,
        #[weak]
        popover,
        move |_| strip.click(index as usize, &popover)
    ));
    add_menu_triggers(&button, &popover);
    // Links to files elsewhere and OLE objects carry nothing to open or
    // save, and neither do Embedded messages too damaged or nested too deep
    // to read, which the Partial read banner reports.
    if attachment.data().is_none() {
        button.set_sensitive(false);
        let reason = if attachment.is_embedded_message() {
            // Translators: {name} is the name of a message attached to the
            // message shown.
            gettext("{name} couldn’t be read")
        } else {
            // Translators: {name} is the Attachment's file name.
            gettext("{name} can’t be saved")
        }
        .replace("{name}", &name);
        button.set_tooltip_text(Some(&reason));
    }

    // The menu belongs to a box around the button, which removes it along
    // with the chip.
    let menu_anchor = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    menu_anchor.append(&button);
    popover.set_parent(&menu_anchor);

    let child = gtk::FlowBoxChild::builder()
        .child(&menu_anchor)
        .focusable(false)
        .build();
    child.set_halign(gtk::Align::Start);
    child
}

/// Whether a single click opens `attachment` rather than showing its menu.
fn opens_on_click(attachment: &Attachment) -> bool {
    match attachment.data() {
        Some(_) if attachment.is_embedded_message() => true,
        Some(data) => attachment_policy::opens_on_click(&attachment.safe_file_name(), data),
        None => false,
    }
}

/// Shows `menu` on a secondary click or long press on `button`, or when the
/// Menu key or Shift+F10 is pressed while it has focus.
fn add_menu_triggers(button: &gtk::Button, menu: &gtk::PopoverMenu) {
    let secondary_click = gtk::GestureClick::builder()
        .button(gtk::gdk::BUTTON_SECONDARY)
        .build();
    secondary_click.connect_pressed(glib::clone!(
        #[weak]
        menu,
        move |gesture, _, _, _| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            menu.popup();
        }
    ));
    button.add_controller(secondary_click);

    // Claiming the touch keeps the button from also taking it as a click.
    let long_press = gtk::GestureLongPress::builder().touch_only(true).build();
    long_press.connect_pressed(glib::clone!(
        #[weak]
        menu,
        move |gesture, _, _| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            menu.popup();
        }
    ));
    button.add_controller(long_press);

    let popup = gtk::CallbackAction::new(glib::clone!(
        #[weak]
        menu,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, _| {
            menu.popup();
            glib::Propagation::Stop
        }
    ));
    let keys = gtk::ShortcutController::new();
    keys.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Menu|<Shift>F10"),
        Some(popup),
    ));
    button.add_controller(keys);
}

/// The symbolic icon for the Attachment's content type, guessed from its
/// name and data rather than trusting the declared MIME type.
fn icon(attachment: &Attachment, name: &str) -> gio::Icon {
    if attachment.is_embedded_message() {
        return gio::ThemedIcon::new("mail-unread-symbolic").upcast();
    }
    let (mut content_type, uncertain) = gio::content_type_guess(Some(name), attachment.data());
    if uncertain {
        if let Some(declared) = attachment
            .mime_type()
            .and_then(gio::content_type_from_mime_type)
        {
            content_type = declared;
        }
    }
    gio::content_type_get_symbolic_icon(&content_type)
}

#[cfg(test)]
mod tests {
    use outlook_msg::Limits;

    use super::*;

    fn sample(bytes: &[u8]) -> Message {
        Message::from_bytes(bytes).unwrap()
    }

    fn clicks(message: &Message) -> Vec<(String, bool)> {
        message
            .attachments()
            .iter()
            .map(|a| (a.safe_file_name(), opens_on_click(a)))
            .collect()
    }

    #[test]
    fn embedded_messages_and_pdfs_open_on_click() {
        let message = sample(include_bytes!(
            "../../outlook-msg/tests/samples/testMSG_att_msg.msg"
        ));
        let clicks = clicks(&message);
        assert_eq!(clicks.len(), 2);
        assert_eq!(clicks[0], ("Test Attachment.msg".to_owned(), true));
        assert!(clicks[1].0.ends_with(".pdf"));
        assert!(clicks[1].1);
    }

    #[test]
    fn plain_text_opens_on_click_but_word_documents_show_the_menu() {
        let message = sample(include_bytes!(
            "../../outlook-msg/tests/samples/testMSG_att_doc.msg"
        ));
        let clicks = clicks(&message);
        let by_extension = |extension: &str| {
            clicks
                .iter()
                .find(|(name, _)| name.ends_with(extension))
                .unwrap()
                .1
        };
        assert!(!by_extension(".doc"));
        assert!(by_extension(".txt"));
    }

    #[test]
    fn images_open_on_click() {
        let message = sample(include_bytes!(
            "../../outlook-msg/tests/samples/test-outlook2003.msg"
        ));
        let images: Vec<_> = clicks(&message)
            .into_iter()
            .filter(|(name, _)| {
                let name = name.to_lowercase();
                [".png", ".jpg", ".jpeg", ".gif"]
                    .iter()
                    .any(|extension| name.ends_with(extension))
            })
            .collect();
        assert!(!images.is_empty());
        assert!(images.iter().all(|(_, opens)| *opens), "{images:?}");
    }

    #[test]
    fn embedded_message_past_the_depth_limit_does_not_open() {
        let bytes = include_bytes!("../../outlook-msg/tests/samples/testMSG_att_msg.msg");
        let limits = Limits {
            max_embedded_depth: 0,
            ..Limits::DEFAULT
        };
        let message = Message::from_bytes_with_limits(bytes, limits).unwrap();
        // The window's Partial read banner reports it.
        assert!(message.is_partial());
        let embedded = &message.attachments()[0];
        assert!(embedded.is_embedded_message());
        assert!(!opens_on_click(embedded));
    }
}
