use std::cell::{Cell, RefCell};
use std::time::SystemTime;

use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::glib;
use gtk::prelude::*;
use outlook_msg::{Mailbox, Message, MessageKind, RecipientKind};

use crate::i18n::gettext_noop;

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

    impl WidgetImpl for HeaderBlock {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.obj().fit_recipients(width);
            self.parent_size_allocate(width, height, baseline);
        }
    }
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
        imp.item_kind
            .set_label(&item_kind.map(gettext).unwrap_or_default());
        imp.item_kind.set_visible(item_kind.is_some());

        let no_subject = gettext("(No Subject)");
        imp.subject
            .set_label(message.subject().unwrap_or(&no_subject));
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

    /// Turns the recipients' scrollbar on only while they are taller than
    /// the scrolled window's maximum at `width`. A scrolled window that may
    /// show a vertical scrollbar is never shorter than the scrollbar, which
    /// would leave a gap below one or two recipient lines. `External` still
    /// scrolls and keeps the minimum height small, unlike `Never`.
    fn fit_recipients(&self, width: i32) {
        let window = &self.imp().recipient_lines;
        let Some(content) = window.child() else {
            return;
        };
        let (_, height, _, _) = content.measure(gtk::Orientation::Vertical, width);
        let policy = if fits(height, window.max_content_height()) {
            gtk::PolicyType::External
        } else {
            gtk::PolicyType::Automatic
        };
        if window.vscrollbar_policy() != policy {
            window.set_vscrollbar_policy(policy);
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
            gettext("Show Less")
        } else {
            // Translators: A link that shows all of a recipient line;
            // {count} is the number of recipients it hides, always 2 or more.
            ngettext("Show {count} More", "Show {count} More", hidden as u32)
                .replace("{count}", &hidden.to_string())
        };
        let text = glib::markup_escape_text(&text);
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

/// Whether content `height` fits a scrolled window's `max_height`, where -1
/// means no maximum.
fn fits(height: i32, max_height: i32) -> bool {
    max_height < 0 || height <= max_height
}

/// What kind of item a Message of another Message class is, in words, or
/// `None` for a class with no better name than "message". Classes are
/// matched without regard to case, and cover the classes derived from them.
/// The words are untranslated; pass them to `gettext`.
fn item_kind(class: &str) -> Option<&'static str> {
    const KINDS: &[(&str, &str)] = &[
        ("IPM.Appointment", gettext_noop("Appointment")),
        (
            "IPM.Schedule.Meeting.Request",
            gettext_noop("Meeting Request"),
        ),
        (
            "IPM.Schedule.Meeting.Canceled",
            gettext_noop("Meeting Canceled"),
        ),
        (
            "IPM.Schedule.Meeting.Resp.Pos",
            gettext_noop("Meeting Accepted"),
        ),
        (
            "IPM.Schedule.Meeting.Resp.Tent",
            gettext_noop("Meeting Tentatively Accepted"),
        ),
        (
            "IPM.Schedule.Meeting.Resp.Neg",
            gettext_noop("Meeting Declined"),
        ),
        ("IPM.Schedule.Meeting", gettext_noop("Meeting")),
        ("IPM.Contact", gettext_noop("Contact")),
        ("IPM.DistList", gettext_noop("Contact Group")),
        ("IPM.Task", gettext_noop("Task")),
        ("IPM.TaskRequest", gettext_noop("Task Request")),
        // Translators: An Outlook sticky note.
        ("IPM.StickyNote", gettext_noop("Note")),
        // Translators: A post in an Outlook public folder.
        ("IPM.Post", gettext_noop("Post")),
        ("IPM.Activity", gettext_noop("Journal Entry")),
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
            "IPNRN" => gettext_noop("Read Receipt"),
            "IPNNRN" => gettext_noop("Not Read Notification"),
            "DR" => gettext_noop("Delivery Receipt"),
            "NDR" => gettext_noop("Delivery Failure"),
            // Translators: A report about the delivery of an email.
            _ => gettext_noop("Report"),
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
/// using a 12-hour clock where the locale does. Translators choose the
/// order and words; the locale names the days and months.
fn format_date(time: SystemTime) -> Option<String> {
    let seconds = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).ok()?,
        Err(before) => -i64::try_from(before.duration().as_secs()).ok()?,
    };
    let date = glib::DateTime::from_unix_local(seconds).ok()?;
    let format = if uses_12_hour_clock() {
        // Translators: When a message was sent, for locales with a 12-hour
        // clock, e.g. "Tuesday, 14 November 2023, 11:13 PM". Reorder or
        // change the codes as your language needs; they are listed at
        // https://docs.gtk.org/glib/method.DateTime.format.html
        gettext("%A, %-d %B %Y, %-l:%M %p")
    } else {
        // Translators: When a message was sent, for locales with a 24-hour
        // clock, e.g. "Tuesday, 14 November 2023, 23:13". Reorder or change
        // the codes as your language needs; they are listed at
        // https://docs.gtk.org/glib/method.DateTime.format.html
        gettext("%A, %-d %B %Y, %H:%M")
    };
    date.format(&format).ok().map(Into::into)
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
    fn recipients_scroll_only_when_taller_than_the_maximum() {
        assert!(fits(16, 200));
        assert!(fits(200, 200));
        assert!(!fits(201, 200));
        assert!(fits(5_000, -1));
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
