use adw::subclass::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use outlook_msg::Message;

use crate::attachment_strip::AttachmentStrip;
use crate::body_view::BodyView;
use crate::header_block::HeaderBlock;
use crate::protection_notice::{ProtectionNotice, protection_notice};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/message_view.ui")]
    pub struct MessageView {
        #[template_child]
        pub protection_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub protection_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub header_block: TemplateChild<HeaderBlock>,
        #[template_child]
        pub attachment_strip: TemplateChild<AttachmentStrip>,
        #[template_child]
        pub body_view: TemplateChild<BodyView>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageView {
        const NAME: &'static str = "MsgViewerMessageView";
        type Type = super::MessageView;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            HeaderBlock::ensure_type();
            AttachmentStrip::ensure_type();
            BodyView::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for MessageView {}
    impl WidgetImpl for MessageView {}
    impl BoxImpl for MessageView {}
}

glib::wrapper! {
    /// Everything shown for one Message: Header block, Attachments and Body.
    pub struct MessageView(ObjectSubclass<imp::MessageView>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl MessageView {
    pub fn set_message(&self, message: &Message) {
        self.imp().header_block.set_message(message);
        self.imp().attachment_strip.set_message(message);
        self.imp().body_view.set_message(message);
        self.show_protection_notice(protection_notice(message));
    }

    /// Shows what a signed or encrypted Message needs, or hides the notices.
    /// Call it after setting the Attachments and Body, whose visibility it
    /// may change.
    fn show_protection_notice(&self, notice: Option<ProtectionNotice>) {
        let imp = self.imp();
        let banner = match notice {
            Some(ProtectionNotice::Banner(title)) => Some(title),
            _ => None,
        };
        imp.protection_banner.set_title(banner.unwrap_or_default());
        imp.protection_banner.set_revealed(banner.is_some());

        let page = match notice {
            Some(ProtectionNotice::Page {
                icon_name,
                title,
                description,
            }) => {
                imp.protection_page.set_icon_name(Some(icon_name));
                imp.protection_page.set_title(title);
                imp.protection_page.set_description(Some(description));
                true
            }
            _ => false,
        };
        imp.protection_page.set_visible(page);
        imp.body_view.set_visible(!page);
        if page {
            imp.attachment_strip.set_visible(false);
        }
    }
}
