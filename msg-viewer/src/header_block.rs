use adw::subclass::prelude::*;
use gtk::glib;
use outlook_msg::{Mailbox, Message};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/header_block.ui")]
    pub struct HeaderBlock {
        #[template_child]
        pub subject: TemplateChild<gtk::Label>,
        #[template_child]
        pub sender: TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HeaderBlock {
        const NAME: &'static str = "MsgViewerHeaderBlock";
        type Type = super::HeaderBlock;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for HeaderBlock {}
    impl WidgetImpl for HeaderBlock {}
    impl BoxImpl for HeaderBlock {}
}

glib::wrapper! {
    /// Subject, sender, date and recipients of a Message.
    pub struct HeaderBlock(ObjectSubclass<imp::HeaderBlock>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl HeaderBlock {
    pub fn set_message(&self, message: &Message) {
        let imp = self.imp();
        imp.subject
            .set_label(message.subject().unwrap_or("(No Subject)"));
        imp.sender.set_label(
            &message
                .sender()
                .map(|sender| format_mailbox(&sender))
                .unwrap_or_default(),
        );
    }
}

/// Formats a Mailbox as `Name <address>`, or whichever half is known.
fn format_mailbox(mailbox: &Mailbox) -> String {
    match (&mailbox.name, &mailbox.address) {
        (Some(name), Some(address)) if name != address => format!("{name} <{address}>"),
        (Some(name), _) => name.clone(),
        (None, Some(address)) => address.clone(),
        (None, None) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_name_and_address() {
        let mailbox = Mailbox {
            name: Some("Ada".into()),
            address: Some("ada@example.org".into()),
        };
        assert_eq!(format_mailbox(&mailbox), "Ada <ada@example.org>");
    }

    #[test]
    fn name_equal_to_address_is_shown_once() {
        let mailbox = Mailbox {
            name: Some("ada@example.org".into()),
            address: Some("ada@example.org".into()),
        };
        assert_eq!(format_mailbox(&mailbox), "ada@example.org");
    }
}
