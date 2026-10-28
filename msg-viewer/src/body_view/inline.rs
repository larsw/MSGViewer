//! Inline attachments: the Attachments an HTML Body shows through `cid:`
//! links ([RFC 2392]), served to the web view from the Message itself
//! (ADR-0004).
//!
//! The `cid` scheme serves the Attachments of the Message shown in the web
//! view that asks, and nothing else: no file is read and nothing is fetched.
//! It serves only raster images, with the type sniffed from their content.
//!
//! Which Attachments count as Inline attachments, and so are left out of the
//! attachment strip, is decided by what the HTML Body shows, not by the
//! sender's hints alone; see [`inline_attachments`].
//!
//! [RFC 2392]: https://www.rfc-editor.org/rfc/rfc2392

use std::cell::RefCell;
use std::collections::BTreeSet;

use gtk::prelude::*;
use gtk::{gio, glib};
use outlook_msg::{Attachment, Body, Message};
use webkit6::{URISchemeRequest, URISchemeResponse, WebContext, WebView};

use super::html;

/// The URI scheme of links to Inline attachments.
const CID_SCHEME: &str = "cid";

/// The image formats served, as sniffed from their content. All are raster
/// formats every WebKitGTK build shows. SVG is left out: it can carry
/// scripts and load other documents, and Outlook never sends it inline.
const IMAGE_TYPES: [&str; 5] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
];

/// How much of an Attachment is sniffed: image signatures sit at the start.
const SNIFF_LEN: usize = 4096;

/// `PidTagAttachFlags` bit for an Attachment the sender's client rendered in
/// the HTML Body ([MS-OXCMSG] 2.2.2.18).
const ATT_RENDERED_IN_BODY: u32 = 0x4;

/// The Attachments of a Message that `cid:` links can show: every image
/// Attachment with a Content-ID. Where several share a Content-ID, the first
/// image among them is shown.
#[derive(Debug, Default)]
pub struct InlineImages {
    images: Vec<InlineImage>,
}

#[derive(Debug)]
struct InlineImage {
    /// The Content-ID, as [`content_id_key`] gives it.
    key: String,
    /// The Attachment's position in the Message.
    index: usize,
    /// The image format, sniffed from `data`.
    content_type: &'static str,
    data: glib::Bytes,
}

impl InlineImages {
    pub fn new(attachments: &[Attachment]) -> Self {
        let mut images: Vec<InlineImage> = Vec::new();
        for (index, attachment) in attachments.iter().enumerate() {
            if attachment.is_embedded_message() {
                continue;
            }
            let (Some(id), Some(data)) = (attachment.content_id(), attachment.data()) else {
                continue;
            };
            let key = content_id_key(id);
            if key.is_empty() || images.iter().any(|image| image.key == key) {
                continue;
            }
            if let Some(content_type) = image_type(data) {
                images.push(InlineImage {
                    key,
                    index,
                    content_type,
                    data: glib::Bytes::from(data),
                });
            }
        }
        Self { images }
    }

    /// The image a `cid:` URL names, and its type, if there is one.
    pub fn get(&self, url: &str) -> Option<(glib::Bytes, &'static str)> {
        self.find(url)
            .map(|image| (image.data.clone(), image.content_type))
    }

    fn find(&self, url: &str) -> Option<&InlineImage> {
        let key = cid_url_key(url)?;
        self.images.iter().find(|image| image.key == key)
    }
}

/// The positions of the Inline attachments of `message`, which the
/// attachment strip leaves out.
///
/// An Attachment is an Inline attachment when its HTML Body shows it: a
/// `cid:` link in an image, style sheet or font of the Body names its
/// Content-ID, and it is an image the `cid` scheme serves. The sender's
/// hints alone don't hide anything, since an Attachment hidden from the
/// strip and not shown in the Body would be out of the reader's reach:
///
/// - Hidden (`PidTagAttachmentHidden`) or "rendered in body" (flag 0x4) but
///   not shown in the Body, as with Outlook 2003's RTF-positioned images or a
///   non-image file: listed.
/// - Shown in the Body but explicitly not hidden, and not flagged as
///   rendered in the Body: the sender attached it as a file as well, so it
///   is shown in the Body and listed.
pub fn inline_attachments(message: &Message) -> BTreeSet<usize> {
    match message.body() {
        Some(Body::Html(html)) => find_inline(&html, message.attachments()),
        _ => BTreeSet::new(),
    }
}

/// [`inline_attachments`] of an HTML Body and its Message's Attachments.
fn find_inline(html: &str, attachments: &[Attachment]) -> BTreeSet<usize> {
    let images = InlineImages::new(attachments);
    html::cid_urls(html)
        .iter()
        .filter_map(|url| images.find(url))
        .map(|image| image.index)
        .filter(|&index| !attached_as_file(&attachments[index]))
        .collect()
}

/// Whether the sender marked the Attachment as one to list, even though the
/// Body shows it.
fn attached_as_file(attachment: &Attachment) -> bool {
    attachment.hidden() == Some(false) && attachment.flags() & ATT_RENDERED_IN_BODY == 0
}

/// The image format `data` holds, if it is one that is served.
fn image_type(data: &[u8]) -> Option<&'static str> {
    let sniffed = &data[..data.len().min(SNIFF_LEN)];
    let (content_type, _) = gio::content_type_guess(None::<&str>, Some(sniffed));
    IMAGE_TYPES
        .into_iter()
        .find(|image| gio::content_type_is_a(&content_type, image))
}

/// The form in which Content-IDs are compared: without surrounding spaces
/// and angle brackets, ignoring case.
fn content_id_key(id: &str) -> String {
    id.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_lowercase()
}

/// The Content-ID a `cid:` URL names, as [`content_id_key`] gives it: the
/// rest of the URL, URL-decoded (RFC 2392), without a fragment.
fn cid_url_key(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once(':')?;
    if !scheme.eq_ignore_ascii_case(CID_SCHEME) {
        return None;
    }
    let rest = rest.split('#').next().unwrap_or_default();
    let key = content_id_key(&percent_decode(rest));
    (!key.is_empty()).then_some(key)
}

/// Decodes `%XX` escapes. Anything else, malformed escapes included, is
/// kept as it is; bytes that don't form UTF-8 become U+FFFD.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut pos = 0;
    while pos < bytes.len() {
        let escaped = bytes
            .get(pos + 1..pos + 3)
            .filter(|hex| bytes[pos] == b'%' && hex.iter().all(u8::is_ascii_hexdigit))
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                pos += 3;
            }
            None => {
                decoded.push(bytes[pos]);
                pos += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

thread_local! {
    /// The web contexts the `cid` scheme is registered on. WebKit allows
    /// registering a scheme only once per context.
    static REGISTERED: RefCell<Vec<glib::WeakRef<WebContext>>> = const { RefCell::new(Vec::new()) };
}

/// Serves `cid:` links in the web views of `context`, once per context.
///
/// Web views share their context, so each request is answered by `serve`
/// for the web view that made it: it gets that view and the URL, and
/// returns the image and its type. Anything it doesn't find fails as "not
/// found", and the Body shows nothing in its place.
pub fn register_cid_scheme(
    context: &WebContext,
    serve: impl Fn(&WebView, &str) -> Option<(glib::Bytes, &'static str)> + 'static,
) {
    let new = REGISTERED.with_borrow_mut(|contexts| {
        contexts.retain(|known| known.upgrade().is_some());
        let known = contexts
            .iter()
            .any(|known| known.upgrade().as_ref() == Some(context));
        if !known {
            contexts.push(context.downgrade());
        }
        !known
    });
    if new {
        context.register_uri_scheme(CID_SCHEME, move |request| {
            let image = request
                .web_view()
                .zip(request.uri())
                .and_then(|(web_view, uri)| serve(&web_view, &uri));
            respond(request, image);
        });
    }
}

fn respond(request: &URISchemeRequest, image: Option<(glib::Bytes, &'static str)>) {
    match image {
        Some((data, content_type)) => {
            let length = data.len() as i64;
            let stream = gio::MemoryInputStream::from_bytes(&data);
            request.finish(&stream, length, Some(content_type));
        }
        None => {
            let response = URISchemeResponse::new(&gio::MemoryInputStream::new(), 0);
            response.set_status(404, Some("Not Found"));
            response.set_content_type("text/plain");
            request.finish_with_response(&response);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use cfb::CompoundFile;

    use super::*;

    const PID_SUBJECT: u16 = 0x0037;
    const PID_HTML: u16 = 0x1013;
    const PID_ATTACH_DATA: u16 = 0x3701;
    const PID_ATTACH_METHOD: u16 = 0x3705;
    const PID_ATTACH_LONG_FILENAME: u16 = 0x3707;
    const PID_ATTACH_MIME_TAG: u16 = 0x370E;
    const PID_ATTACH_CONTENT_ID: u16 = 0x3712;
    const PID_ATTACH_FLAGS: u16 = 0x3714;
    const PID_ATTACHMENT_HIDDEN: u16 = 0x7FFE;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";
    const JPEG: &[u8] = b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01\x01\0\0\x01\0\x01\0\0";
    const GIF: &[u8] = b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\xff\xff";
    const PDF: &[u8] = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
    const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>x()</script></svg>";

    enum Value<'a> {
        Long(i32),
        Bool(bool),
        Unicode(&'a str),
        Binary(&'a [u8]),
    }

    type Props<'a> = Vec<(u16, Value<'a>)>;

    /// A Message with an HTML Body and Attachments with these properties.
    fn build(html: &str, attachments: &[Props]) -> Message {
        Message::from_bytes(&msg_file("Inline attachments", html, attachments)).unwrap()
    }

    /// A `.msg` file with a subject, an HTML Body and Attachments with these
    /// properties, laid out as in [MS-OXMSG] 2.4.
    fn msg_file(subject: &str, html: &str, attachments: &[Props]) -> Vec<u8> {
        let mut file = CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        let mut header = vec![0; 8];
        for n in [0, attachments.len(), 0, attachments.len()] {
            header.extend((n as u32).to_le_bytes());
        }
        header.extend([0; 8]);
        let message = [
            (PID_SUBJECT, Value::Unicode(subject)),
            (PID_HTML, Value::Binary(html.as_bytes())),
        ];
        write_props(&mut file, "", header, message.iter());
        for (i, props) in attachments.iter().enumerate() {
            let storage = format!("/__attach_version1.0_#{i:08X}");
            file.create_storage(&storage).unwrap();
            let mut props: Vec<_> = props.iter().collect();
            let method = (PID_ATTACH_METHOD, Value::Long(1));
            props.push(&method);
            write_props(&mut file, &storage, vec![0; 8], props);
        }
        file.flush().unwrap();
        file.into_inner().into_inner()
    }

    fn write_props<'a>(
        file: &mut CompoundFile<Cursor<Vec<u8>>>,
        storage: &str,
        mut table: Vec<u8>,
        props: impl IntoIterator<Item = &'a (u16, Value<'a>)>,
    ) {
        for (id, value) in props {
            let (ty, fixed, data) = match value {
                Value::Long(n) => (0x0003, u64::from(*n as u32), None),
                Value::Bool(b) => (0x000B, u64::from(*b), None),
                Value::Unicode(s) => {
                    let mut data: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                    data.extend([0, 0]);
                    (0x001F, data.len() as u64, Some(data))
                }
                Value::Binary(data) => (0x0102, data.len() as u64, Some(data.to_vec())),
            };
            let tag = (u32::from(*id) << 16) | ty;
            table.extend(tag.to_le_bytes());
            table.extend(6u32.to_le_bytes());
            table.extend(fixed.to_le_bytes());
            if let Some(data) = data {
                let path = format!("{storage}/__substg1.0_{tag:08X}");
                file.create_stream(&path).unwrap().write_all(&data).unwrap();
            }
        }
        let path = format!("{storage}/__properties_version1.0");
        file.create_stream(&path)
            .unwrap()
            .write_all(&table)
            .unwrap();
    }

    /// An Attachment named `name` holding `data`, with Content-ID `cid`.
    fn image<'a>(name: &'a str, cid: &'a str, data: &'a [u8]) -> Props<'a> {
        vec![
            (PID_ATTACH_LONG_FILENAME, Value::Unicode(name)),
            (PID_ATTACH_CONTENT_ID, Value::Unicode(cid)),
            (PID_ATTACH_DATA, Value::Binary(data)),
        ]
    }

    fn with<'a>(
        mut props: Props<'a>,
        extra: impl IntoIterator<Item = (u16, Value<'a>)>,
    ) -> Props<'a> {
        props.extend(extra);
        props
    }

    fn inline(message: &Message) -> Vec<usize> {
        inline_attachments(message).into_iter().collect()
    }

    fn served(message: &Message, url: &str) -> Option<(Vec<u8>, &'static str)> {
        InlineImages::new(message.attachments())
            .get(url)
            .map(|(data, content_type)| (data.to_vec(), content_type))
    }

    #[test]
    fn referenced_images_are_inline_and_served() {
        let message = build(
            "<p><img src=\"cid:logo@example.org\"><img src=cid:photo.jpg@01D0>",
            &[
                with(
                    image("logo.png", "<logo@example.org>", PNG),
                    [
                        (PID_ATTACHMENT_HIDDEN, Value::Bool(true)),
                        (PID_ATTACH_FLAGS, Value::Long(4)),
                    ],
                ),
                image("photo.jpg", "photo.jpg@01D0", JPEG),
                image("report.png", "report@example.org", PNG),
            ],
        );
        assert_eq!(inline(&message), [0, 1]);
        assert_eq!(
            served(&message, "cid:logo@example.org"),
            Some((PNG.to_vec(), "image/png"))
        );
        assert_eq!(
            served(&message, "cid:photo.jpg@01D0"),
            Some((JPEG.to_vec(), "image/jpeg"))
        );
        // Not referenced, but still served if the Body asks in a way the
        // scan doesn't see.
        assert!(served(&message, "cid:report@example.org").is_some());
    }

    #[test]
    fn hidden_but_not_referenced_is_listed() {
        let message = build(
            "<p>No pictures here.</p><a href=\"cid:logo@x\">logo</a>",
            &[with(
                image("logo.png", "logo@x", PNG),
                [
                    (PID_ATTACHMENT_HIDDEN, Value::Bool(true)),
                    (PID_ATTACH_FLAGS, Value::Long(4)),
                ],
            )],
        );
        assert!(inline(&message).is_empty());
    }

    #[test]
    fn referenced_but_not_hidden_is_inline() {
        let message = build(
            "<img src=\"cid:logo@x\">",
            &[image("logo.png", "logo@x", GIF)],
        );
        assert_eq!(inline(&message), [0]);
        assert_eq!(served(&message, "cid:logo@x").unwrap().1, "image/gif");
    }

    #[test]
    fn referenced_but_attached_as_a_file_is_listed_and_shown() {
        let message = build(
            "<img src=\"cid:chart@x\"><img src=\"cid:logo@x\">",
            &[
                with(
                    image("chart.png", "chart@x", PNG),
                    [(PID_ATTACHMENT_HIDDEN, Value::Bool(false))],
                ),
                with(
                    image("logo.png", "logo@x", PNG),
                    [
                        (PID_ATTACHMENT_HIDDEN, Value::Bool(false)),
                        (PID_ATTACH_FLAGS, Value::Long(4)),
                    ],
                ),
            ],
        );
        assert_eq!(inline(&message), [1]);
        assert!(served(&message, "cid:chart@x").is_some());
    }

    #[test]
    fn duplicate_content_ids_show_the_first_image() {
        let message = build(
            "<img src=\"cid:dup@x\">",
            &[
                image("notes.pdf", "dup@x", PDF),
                image("first.png", "<dup@x>", PNG),
                image("second.jpg", "DUP@X", JPEG),
            ],
        );
        assert_eq!(inline(&message), [1]);
        assert_eq!(served(&message, "cid:dup@x").unwrap().1, "image/png");
    }

    #[test]
    fn odd_cid_encodings_match() {
        let message = build(
            "<img src=\"CID:%3CImage%20One@X%3E\"><img src=\"cid:caf%C3%A9@x#top\">\
             <img src=\"cid:%3cpercent%2541@x%3e\">",
            &[
                image("one.png", "<image one@x>", PNG),
                image("café.png", "Café@X", PNG),
                image("percent.png", "percent%41@x", PNG),
            ],
        );
        assert_eq!(inline(&message), [0, 1, 2]);
        assert!(served(&message, "cid:image%20one@x").is_some());
        assert!(served(&message, "cid:CAFÉ@x").is_some());
        assert!(served(&message, "cid:percentA@x").is_none());
    }

    #[test]
    fn content_ids_and_urls_compare_like_rfc_2392() {
        assert_eq!(content_id_key(" <Image001.PNG@01D0> "), "image001.png@01d0");
        assert_eq!(
            cid_url_key("cid:Image001.PNG@01D0"),
            Some("image001.png@01d0".into())
        );
        assert_eq!(cid_url_key("CID:%3Ca%20b%3E#frag"), Some("a b".into()));
        assert_eq!(cid_url_key("cid:100%"), Some("100%".into()));
        assert_eq!(cid_url_key("cid:%zz%4"), Some("%zz%4".into()));
        assert_eq!(cid_url_key("cid:%ff"), Some("\u{fffd}".into()));
        assert_eq!(cid_url_key("cid:"), None);
        assert_eq!(cid_url_key("cid:%3C%3E"), None);
        assert_eq!(cid_url_key("mid:a@b"), None);
        assert_eq!(cid_url_key("a@b"), None);
    }

    #[test]
    fn only_images_are_served_whatever_the_declared_type() {
        let message = build(
            "<img src=cid:pdf@x><img src=cid:svg@x><img src=cid:png@x><img src=cid:fake@x>",
            &[
                image("report.pdf", "pdf@x", PDF),
                with(
                    image("drawing.svg", "svg@x", SVG),
                    [(PID_ATTACH_MIME_TAG, Value::Unicode("image/svg+xml"))],
                ),
                with(
                    image("logo", "png@x", PNG),
                    [(
                        PID_ATTACH_MIME_TAG,
                        Value::Unicode("application/octet-stream"),
                    )],
                ),
                with(
                    image("fake.png", "fake@x", PDF),
                    [(PID_ATTACH_MIME_TAG, Value::Unicode("image/png"))],
                ),
            ],
        );
        assert_eq!(inline(&message), [2]);
        assert!(served(&message, "cid:pdf@x").is_none());
        assert!(served(&message, "cid:svg@x").is_none());
        assert!(served(&message, "cid:fake@x").is_none());
        assert_eq!(served(&message, "cid:png@x").unwrap().1, "image/png");
    }

    #[test]
    fn missing_targets_are_not_found() {
        let message = build(
            "<img src=\"cid:gone@x\">",
            &[image("logo.png", "logo@x", PNG)],
        );
        assert!(inline(&message).is_empty());
        assert!(served(&message, "cid:gone@x").is_none());
        assert!(served(&message, "https://example.org/logo@x").is_none());
        assert!(InlineImages::default().get("cid:logo@x").is_none());
    }

    #[test]
    fn sample_with_inline_images() {
        let message = Message::from_bytes(include_bytes!(
            "../../../outlook-msg/tests/samples/attachment_msg_inlineImg.msg"
        ))
        .unwrap();
        assert_eq!(message.attachments().len(), 4);
        assert_eq!(inline(&message), [0, 1, 2, 3]);
        let (_, content_type) = served(&message, "cid:image006.jpg@01D0A526.B649E220").unwrap();
        assert_eq!(content_type, "image/jpeg");
    }

    #[test]
    fn rtf_positioned_images_without_html_are_listed() {
        // Outlook 2003 flags its images as rendered in the Body, but they
        // have no Content-ID and the Message has no HTML Body to show them.
        let message = Message::from_bytes(include_bytes!(
            "../../../outlook-msg/tests/samples/test-outlook2003.msg"
        ))
        .unwrap();
        assert!(message.attachments().iter().all(|a| a.flags() & 4 != 0));
        assert!(inline(&message).is_empty());
    }

    /// Writes `.msg` files with Inline attachments for looking at the app with,
    /// e.g. `MSG_FIXTURE_DIR=/tmp/fixtures cargo test -p msg-viewer -- --ignored`.
    #[test]
    #[ignore]
    fn write_inline_fixtures() {
        let dir = std::env::var("MSG_FIXTURE_DIR").expect("MSG_FIXTURE_DIR is not set");
        // One-pixel GIFs in a single colour, scaled up by the Body.
        let gif = |r, g, b| {
            let mut gif = b"GIF89a\x01\0\x01\0\x80\0\0".to_vec();
            gif.extend([r, g, b, 0, 0, 0]);
            gif.extend(b"!\xf9\x04\0\0\0\0\0,\0\0\0\0\x01\0\x01\0\0\x02\x02D\x01\0;");
            gif
        };
        let (red, blue, green) = (
            gif(0xe0, 0x1b, 0x24),
            gif(0x1c, 0x71, 0xd8),
            gif(0x2e, 0xc2, 0x7e),
        );
        for (name, colour, data) in [("red", "Red", &red), ("blue", "Blue", &blue)] {
            let html = format!(
                "<!DOCTYPE html><html><body><h1>{colour} logo</h1>\
             <p>Inline attachment <code>cid:logo@example.org</code>, {name} in this Message:</p>\
             <p><img alt=\"logo\" src=\"cid:logo@example.org\" width=\"160\" height=\"80\"></p>\
             <p>Inline attachment by odd encoding: \
             <img alt=\"odd\" src=\"CID:%3CSquare%20One@Example.org%3E\" width=\"40\" height=\"40\"></p>\
             <p>Missing target: [<img src=\"cid:missing@example.org\">] \
             and with alt text: [<img alt=\"missing\" src=\"cid:missing@example.org\">]</p>\
             <p>Not an image: [<img src=\"cid:report@example.org\">] \
             SVG: [<img src=\"cid:drawing@example.org\" width=\"40\" height=\"40\">]</p>\
             <p>Remote image: <img alt=\"remote\" src=\"https://example.org/remote.png\" width=\"40\" height=\"40\"></p>\
             </body></html>"
            );
            let hidden = (PID_ATTACHMENT_HIDDEN, Value::Bool(true));
            let attachments = [
                with(image("logo.gif", "<logo@example.org>", data), [hidden]),
                image("square.gif", "<square one@example.org>", &green),
                image("report.pdf", "<report@example.org>", PDF),
                image("drawing.svg", "<drawing@example.org>", SVG),
                with(
                    image("unreferenced.gif", "<unreferenced@example.org>", &green),
                    [(PID_ATTACHMENT_HIDDEN, Value::Bool(true))],
                ),
            ];
            let subject = format!("Inline attachments: {colour}");
            let bytes = msg_file(&subject, &html, &attachments);
            std::fs::write(format!("{dir}/inline-{name}.msg"), bytes).unwrap();
        }
    }
}
