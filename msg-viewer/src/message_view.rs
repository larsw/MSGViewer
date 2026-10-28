use adw::subclass::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use outlook_msg::Message;

use crate::body_view::BodyView;
use crate::header_block::HeaderBlock;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/message_view.ui")]
    pub struct MessageView {
        #[template_child]
        pub header_block: TemplateChild<HeaderBlock>,
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
        self.imp().body_view.set_message(message);
    }
}
