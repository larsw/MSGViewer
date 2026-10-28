use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use outlook_msg::{Attachment, Message};

use crate::attachment_save;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/attachment_strip.ui")]
    pub struct AttachmentStrip {
        #[template_child]
        pub count: TemplateChild<gtk::Label>,
        #[template_child]
        pub chips: TemplateChild<gtk::FlowBox>,
        /// The Attachments shown, indexed by the `attachments.save` target.
        pub attachments: RefCell<Vec<Attachment>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AttachmentStrip {
        const NAME: &'static str = "MsgViewerAttachmentStrip";
        type Type = super::AttachmentStrip;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
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

glib::wrapper! {
    /// The Attachments of a Message as a wrapping strip of chips, each with
    /// a menu to save it, plus Save All. Hidden when there are none.
    pub struct AttachmentStrip(ObjectSubclass<imp::AttachmentStrip>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl AttachmentStrip {
    pub fn set_message(&self, message: &Message) {
        let imp = self.imp();
        let attachments = message.attachments().to_vec();

        imp.chips.remove_all();
        for (index, attachment) in attachments.iter().enumerate() {
            imp.chips.append(&chip(attachment, index as u32));
        }
        imp.count.set_label(&match attachments.len() {
            1 => "1 Attachment".to_owned(),
            n => format!("{n} Attachments"),
        });
        self.set_visible(!attachments.is_empty());
        self.action_set_enabled(
            "attachments.save-all",
            attachments.iter().any(|a| a.data().is_some()),
        );
        imp.attachments.replace(attachments);
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
            .title("Save Attachment")
            .modal(true)
            .initial_name(name)
            .build();
        let Some(file) = choose(dialog.save_future(self.window().as_ref()).await) else {
            return;
        };
        if let Err(error) = attachment_save::save_to_file(&file, data).await {
            self.show_error("Could Not Save Attachment", &error);
        }
    }

    /// Asks for a folder and saves every Attachment into it.
    async fn save_all(&self) {
        let dialog = gtk::FileDialog::builder()
            .title("Save All Attachments")
            .accept_label("_Save")
            .modal(true)
            .build();
        let Some(folder) = choose(dialog.select_folder_future(self.window().as_ref()).await) else {
            return;
        };
        // The action is only enabled while some Attachment has data.
        let attachments = self.savable();
        if let Err(error) = attachment_save::save_into_folder(&folder, &attachments).await {
            self.show_error("Could Not Save Attachments", &error);
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
        dialog.add_response("close", "_Close");
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

/// A chip for one Attachment: icon, file name and size, opening a menu.
fn chip(attachment: &Attachment, index: u32) -> gtk::FlowBoxChild {
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
    let save = gio::MenuItem::new(Some("_Save…"), None);
    save.set_action_and_target_value(Some("attachments.save"), Some(&index.to_variant()));
    menu.append_item(&save);

    let button = gtk::MenuButton::builder()
        .child(&content)
        .menu_model(&menu)
        .tooltip_text(&name)
        .build();
    let accessible_label = match size.as_str() {
        "" => name.clone(),
        size => format!("{name}, {size}"),
    };
    button.update_property(&[gtk::accessible::Property::Label(&accessible_label)]);
    // Links to files elsewhere and OLE objects carry nothing to save.
    if attachment.data().is_none() {
        button.set_sensitive(false);
        button.set_tooltip_text(Some(&format!("{name} can’t be saved")));
    }

    let child = gtk::FlowBoxChild::builder()
        .child(&button)
        .focusable(false)
        .build();
    child.set_halign(gtk::Align::Start);
    child
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
