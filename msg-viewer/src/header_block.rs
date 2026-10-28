use std::cell::{Cell, RefCell};
use std::time::SystemTime;

use adw::subclass::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use outlook_msg::{Mailbox, Message, MessageKind, RecipientKind};

/// How many recipients a collapsed recipient line shows.
const COLLAPSED_COUNT: usize = 3;

const KINDS: [RecipientKind; 3] = [RecipientKind::To, RecipientKind::Cc, RecipientKind::Bcc];

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/header_block.ui")]
    pub struct HeaderBlock {
        #[template_child]
        pub item_kind: TemplateChild<gtk::Label>,
        #[template_child]
        pub subject: TemplateChild<gtk::Label>,
        #[template_child]
        pub sender: TemplateChild<gtk::Label>,
        #[template_child]
        pub date: TemplateChild<gtk::Label>,
        #[template_child]
        pub recipient_lines: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub to_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub to_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub to_list: TemplateChild<gtk::Label>,
        #[template_child]
        pub to_expand: TemplateChild<gtk::Label>,
        #[template_child]
        pub cc_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub cc_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub cc_list: TemplateChild<gtk::Label>,
        #[template_child]
        pub cc_expand: TemplateChild<gtk::Label>,
        #[template_child]
        pub bcc_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub bcc_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub bcc_list: TemplateChild<gtk::Label>,
        #[template_child]
        pub bcc_expand: TemplateChild<gtk::Label>,
        /// The formatted recipients of each line, in `KINDS` order.
        pub recipients: RefCell<[Vec<String>; 3]>,
        /// Whether each line is expanded, in `KINDS` order.
        pub expanded: Cell<[bool; 3]>,
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

    impl ObjectImpl for HeaderBlock {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            for kind in KINDS {
                obj.line(kind).expand.connect_activate_link(glib::clone!(
                    #[weak]
                    obj,
                    #[upgrade_or]
                    glib::Propagation::Stop,
                    move |_, _| {
                        obj.toggle_expanded(kind);
                        glib::Propagation::Stop
                    }
                ));
            }
        }
    }

    impl WidgetImpl for HeaderBlock {}
    impl BoxImpl for HeaderBlock {}
}

glib::wrapper! {
    /// Subject, sender, date and recipients of a Message.
    pub struct HeaderBlock(ObjectSubclass<imp::HeaderBlock>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

/// The widgets showing one kind of recipient.
struct Line<'a> {
    title: &'a gtk::Label,
    content: &'a gtk::Box,
    list: &'a gtk::Label,
    expand: &'a gtk::Label,
}

impl HeaderBlock {
    pub fn set_message(&self, message: &Message) {
        let imp = self.imp();
        let item_kind = match message.kind() {
            MessageKind::Other(class) => item_kind(&class),
            _ => None,
        };
        imp.item_kind.set_label(item_kind.unwrap_or_default());
        imp.item_kind.set_visible(item_kind.is_some());

        imp.subject
            .set_label(message.subject().unwrap_or("(No Subject)"));
        // Contacts, tasks and notes often have no sender.
        let sender = message.sender().map(|sender| format_mailbox(&sender));
        imp.sender.set_label(sender.as_deref().unwrap_or_default());
        imp.sender.set_visible(sender.is_some());

        let date = message.date().and_then(format_date);
        imp.date.set_label(date.as_deref().unwrap_or_default());
        imp.date.set_visible(date.is_some());

        let mut recipients: [Vec<String>; 3] = Default::default();
        for recipient in message.recipients() {
            recipients[index(recipient.kind)].push(format_mailbox(&recipient.mailbox));
        }
        imp.recipient_lines
            .set_visible(recipients.iter().any(|line| !line.is_empty()));
        imp.recipients.replace(recipients);
        imp.expanded.set([false; 3]);
        for kind in KINDS {
            self.update_line(kind);
        }
    }

    fn line(&self, kind: RecipientKind) -> Line<'_> {
        let imp = self.imp();
        match kind {
            RecipientKind::To => Line {
                title: &imp.to_title,
                content: &imp.to_box,
                list: &imp.to_list,
                expand: &imp.to_expand,
            },
            RecipientKind::Cc => Line {
                title: &imp.cc_title,
                content: &imp.cc_box,
                list: &imp.cc_list,
                expand: &imp.cc_expand,
            },
            RecipientKind::Bcc => Line {
                title: &imp.bcc_title,
                content: &imp.bcc_box,
                list: &imp.bcc_list,
                expand: &imp.bcc_expand,
            },
        }
    }

    fn toggle_expanded(&self, kind: RecipientKind) {
        let mut expanded = self.imp().expanded.get();
        expanded[index(kind)] ^= true;
        self.imp().expanded.set(expanded);
        self.update_line(kind);
    }

    fn update_line(&self, kind: RecipientKind) {
        let imp = self.imp();
        let line = self.line(kind);
        let recipients = &imp.recipients.borrow()[index(kind)];
        let expanded = imp.expanded.get()[index(kind)];

        let visible = !recipients.is_empty();
        line.title.set_visible(visible);
        line.content.set_visible(visible);

        let shown = shown_recipients(recipients, expanded);
        line.list.set_label(&shown.join(", "));
        let hidden = recipients.len() - shown.len();
        line.expand.set_visible(collapsible(recipients));
        let text = if expanded {
            "Show Less".to_owned()
        } else {
            format!("Show {hidden} More")
        };
        line.expand.set_markup(&format!("<a href=\"#\">{text}</a>"));
    }
}

fn index(kind: RecipientKind) -> usize {
    match kind {
        RecipientKind::To => 0,
        RecipientKind::Cc => 1,
        RecipientKind::Bcc => 2,
    }
}

/// The recipients a line shows. A collapsed line hides all but the first
/// few, unless only one would be hidden.
fn shown_recipients(recipients: &[String], expanded: bool) -> &[String] {
    if expanded || !collapsible(recipients) {
        recipients
    } else {
        &recipients[..COLLAPSED_COUNT]
    }
}

fn collapsible(recipients: &[String]) -> bool {
    recipients.len() > COLLAPSED_COUNT + 1
}

/// What kind of item a Message of another Message class is, in words, or
/// `None` for a class with no better name than "message". Classes are
/// matched without regard to case, and cover the classes derived from them.
fn item_kind(class: &str) -> Option<&'static str> {
    const KINDS: &[(&str, &str)] = &[
        ("IPM.Appointment", "Appointment"),
        ("IPM.Schedule.Meeting.Request", "Meeting Request"),
        ("IPM.Schedule.Meeting.Canceled", "Meeting Canceled"),
        ("IPM.Schedule.Meeting.Resp.Pos", "Meeting Accepted"),
        (
            "IPM.Schedule.Meeting.Resp.Tent",
            "Meeting Tentatively Accepted",
        ),
        ("IPM.Schedule.Meeting.Resp.Neg", "Meeting Declined"),
        ("IPM.Schedule.Meeting", "Meeting"),
        ("IPM.Contact", "Contact"),
        ("IPM.DistList", "Contact Group"),
        ("IPM.Task", "Task"),
        ("IPM.TaskRequest", "Task Request"),
        ("IPM.StickyNote", "Note"),
        ("IPM.Post", "Post"),
        ("IPM.Activity", "Journal Entry"),
    ];
    let is = |base: &str| {
        class.get(..base.len()).is_some_and(|start| {
            start.eq_ignore_ascii_case(base)
                && matches!(class.as_bytes().get(base.len()), None | Some(b'.'))
        })
    };
    if let Some(&(_, kind)) = KINDS.iter().find(|(base, _)| is(base)) {
        return Some(kind);
    }
    // Reports name the class of the Message they are about, and end in
    // what they report ([MS-OXOMSG] 2.2.2.1).
    if is("REPORT") {
        let what = class.rsplit('.').next().unwrap_or_default();
        return Some(match what.to_ascii_uppercase().as_str() {
            "IPNRN" => "Read Receipt",
            "IPNNRN" => "Not Read Notification",
            "DR" => "Delivery Receipt",
            "NDR" => "Delivery Failure",
            _ => "Report",
        });
    }
    None
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

/// Formats a date in local time, e.g. "Tuesday, 14 November 2023, 23:13",
/// using a 12-hour clock where the locale does.
fn format_date(time: SystemTime) -> Option<String> {
    let seconds = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).ok()?,
        Err(before) => -i64::try_from(before.duration().as_secs()).ok()?,
    };
    let date = glib::DateTime::from_unix_local(seconds).ok()?;
    let format = if uses_12_hour_clock() {
        "%A, %-d %B %Y, %-l:%M %p"
    } else {
        "%A, %-d %B %Y, %H:%M"
    };
    date.format(format).ok().map(Into::into)
}

/// Whether the locale's time format uses a 12-hour clock. GLib's `%p` can't
/// tell, since it falls back to "AM" and "PM" in 24-hour locales.
fn uses_12_hour_clock() -> bool {
    glib::DateTime::from_utc(2000, 1, 1, 13, 0, 0.0)
        .and_then(|afternoon| afternoon.format("%X"))
        .is_ok_and(|time| !time.contains("13"))
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

    fn names(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("R{i}")).collect()
    }

    #[test]
    fn short_lines_are_not_collapsed() {
        let recipients = names(COLLAPSED_COUNT + 1);
        assert!(!collapsible(&recipients));
        assert_eq!(shown_recipients(&recipients, false), recipients);
    }

    #[test]
    fn long_lines_collapse_until_expanded() {
        let recipients = names(20);
        assert_eq!(
            shown_recipients(&recipients, false),
            &recipients[..COLLAPSED_COUNT]
        );
        assert_eq!(shown_recipients(&recipients, true), recipients);
    }

    #[test]
    fn names_other_message_classes() {
        assert_eq!(item_kind("IPM.Contact"), Some("Contact"));
        assert_eq!(item_kind("ipm.contact.custom"), Some("Contact"));
        assert_eq!(item_kind("IPM.Contacts"), None);
        assert_eq!(
            item_kind("IPM.Schedule.Meeting.Resp.Pos"),
            Some("Meeting Accepted")
        );
        assert_eq!(
            item_kind("IPM.Schedule.Meeting.Notification"),
            Some("Meeting")
        );
        assert_eq!(item_kind("REPORT.IPM.Note.IPNRN"), Some("Read Receipt"));
        assert_eq!(item_kind("REPORT.IPM.Note.NDR"), Some("Delivery Failure"));
        assert_eq!(item_kind("REPORT"), Some("Report"));
        assert_eq!(item_kind("IPM.Sharing"), None);
        assert_eq!(item_kind("Élan"), None);
    }

    #[test]
    fn formats_dates_before_and_after_1970() {
        let year = |time| format_date(time).unwrap();
        let day = std::time::Duration::from_secs(86_400);
        assert!(year(SystemTime::UNIX_EPOCH + 20_000 * day).contains("2024"));
        assert!(year(SystemTime::UNIX_EPOCH - 3_650 * day).contains("1960"));
    }
}
