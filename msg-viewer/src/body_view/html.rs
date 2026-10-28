//! Prepares an HTML Body for the web view, and finds out whether it
//! references Remote content.
//!
//! What may load is enforced with a Content Security Policy put in front of
//! the Body's own markup, on top of the content filter the web view applies
//! (see `remote_content.rs`). Neither depends on the Body's markup being
//! well-formed or honest.

use std::borrow::Cow;

/// Whether the reader has allowed Remote content for the Message shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RemoteContent {
    #[default]
    Blocked,
    Allowed,
}

/// The policy while Remote content is blocked: nothing loads but what is
/// embedded in the Body itself (`data:` URLs) and Inline attachments
/// (`cid:` links). Inline styles are fine, scripts and plugins never run,
/// and `<base>` can't make relative links point at a server.
const BLOCKED_POLICY: &str = "default-src 'none'; \
     img-src data: cid:; style-src 'unsafe-inline' data: cid:; font-src data: cid:; \
     base-uri 'none'; form-action 'none'";

/// The policy once the reader has allowed Remote content: anything may load
/// but scripts, plugins and frames, which never run in a Body.
const ALLOWED_POLICY: &str =
    "script-src 'none'; object-src 'none'; frame-src 'none'; form-action 'none'";

/// Returns `html` with a Content Security Policy for `remote` and DNS
/// prefetching turned off put first in its `<head>`, before anything in the
/// Body can ask for a load. Neither can be undone by the Body: further
/// policies only restrict more, and DNS prefetching once turned off stays
/// off.
pub fn prepare(html: &str, remote: RemoteContent) -> String {
    let policy = match remote {
        RemoteContent::Blocked => BLOCKED_POLICY,
        RemoteContent::Allowed => ALLOWED_POLICY,
    };
    let head = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{policy}\">\
         <meta http-equiv=\"x-dns-prefetch-control\" content=\"off\">"
    );
    let at = head_insertion_point(html);
    let mut prepared = String::with_capacity(html.len() + head.len());
    prepared.push_str(&html[..at]);
    prepared.push_str(&head);
    prepared.push_str(&html[at..]);
    prepared
}

/// Where elements put into `html` end up first in its `<head>`: right after
/// the doctype, if there is one. Anything else first makes the HTML parser
/// open the `<head>` itself; `<html>` and `<head>` tags that follow are
/// merged into the ones it opened.
fn head_insertion_point(html: &str) -> usize {
    let bytes = html.as_bytes();
    let mut pos = 0;
    loop {
        pos += bytes[pos..]
            .iter()
            .take_while(|b| b.is_ascii_whitespace())
            .count();
        let rest = &bytes[pos..];
        if rest.starts_with(b"<!--") {
            pos += comment_len(rest);
        } else if starts_with_ignore_case(rest, b"<!doctype") {
            return pos + find(rest, b">").map_or(rest.len(), |end| end + 1);
        } else {
            return 0;
        }
    }
}

/// Whether `html` references Remote content: images, style sheets or fonts
/// that the web view would fetch from a server once allowed to.
///
/// Links, frames, forms, scripts, plugins and media never load in a Body,
/// so they don't count. This reads the markup much as the HTML parser does,
/// but errs towards finding Remote content: at worst the reader is offered
/// to load Remote content and nothing more shows.
pub fn references_remote_content(html: &str) -> bool {
    let mut base_is_remote = None;
    let mut urls = Vec::new();
    for_each_resource(html, &mut |resource| match resource {
        Resource::Base(url) => {
            base_is_remote.get_or_insert_with(|| is_network_url(&url));
        }
        resource => urls.push(resource),
    });
    let base_is_remote = base_is_remote.unwrap_or(false);
    urls.iter().any(|resource| match resource {
        Resource::Url(url) => is_remote(url, base_is_remote),
        Resource::AbsoluteUrl(url) => is_network_url(url),
        Resource::Base(_) => false,
    })
}

/// Something in an HTML Body that names a URL.
#[derive(Debug, PartialEq, Eq)]
enum Resource {
    /// The URL of a `<base>` element.
    Base(String),
    /// A URL that would be loaded, relative to the base URL.
    Url(String),
    /// A string in CSS that would be loaded only if it is a URL, such as an
    /// argument of `image-set()`.
    AbsoluteUrl(String),
}

/// Elements whose content is text rather than markup.
const RAW_TEXT_ELEMENTS: [&str; 8] = [
    "iframe", "noembed", "noframes", "script", "style", "textarea", "title", "xmp",
];

/// Calls `found` with every URL `html` would load, and its `<base>` URLs.
fn for_each_resource(html: &str, found: &mut dyn FnMut(Resource)) {
    let bytes = html.as_bytes();
    let mut pos = 0;
    while let Some(lt) = bytes[pos..].iter().position(|&b| b == b'<') {
        pos += lt;
        let rest = &bytes[pos..];
        let next = rest.get(1).copied().unwrap_or(0);
        if rest.starts_with(b"<!--") {
            pos += comment_len(rest);
        } else if matches!(next, b'!' | b'/' | b'?') {
            pos += find(rest, b">").map_or(rest.len(), |end| end + 1);
        } else if next.is_ascii_alphabetic() {
            pos += 1;
            let name_len = bytes[pos..]
                .iter()
                .take_while(|&&b| !is_space(b) && b != b'/' && b != b'>')
                .count();
            let name = html[pos..pos + name_len].to_ascii_lowercase();
            pos += name_len;
            let mut attributes = Vec::new();
            while let Some(attribute) = attribute(html, &mut pos) {
                attributes.push(attribute);
            }
            tag_resources(&name, &attributes, found);
            if RAW_TEXT_ELEMENTS.contains(&name.as_str()) {
                let end = find_ignore_case(&bytes[pos..], format!("</{name}").as_bytes())
                    .map_or(bytes.len(), |end| pos + end);
                if name == "style" {
                    css_resources(&html[pos..end], found);
                }
                pos = end;
            }
        } else {
            pos += 1;
        }
    }
}

/// The resources named by the attributes of one start tag.
fn tag_resources(tag: &str, attributes: &[(String, String)], found: &mut dyn FnMut(Resource)) {
    let get = |name: &str| {
        attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, value)| decode_char_refs(value))
    };
    let url = |value: Cow<str>| Resource::Url(value.into_owned());
    if let Some(style) = get("style") {
        css_resources(&style, found);
    }
    if let Some(background) = get("background") {
        found(url(background));
    }
    match tag {
        "base" => {
            if let Some(href) = get("href") {
                found(Resource::Base(href.into_owned()));
            }
        }
        "img" | "image" | "input" => {
            for name in ["src", "lowsrc", "dynsrc"] {
                if let Some(src) = get(name) {
                    found(url(src));
                }
            }
        }
        "link" => {
            let rel = get("rel").unwrap_or_default();
            let loads = rel
                .split_ascii_whitespace()
                .any(|rel| rel == "stylesheet" || rel == "preload");
            if let Some(href) = get("href").filter(|_| loads) {
                found(url(href));
            }
        }
        _ => {}
    }
    // SVG images and references to other SVG documents.
    if matches!(tag, "image" | "use" | "feimage") {
        for name in ["href", "xlink:href"] {
            if let Some(href) = get(name) {
                found(url(href));
            }
        }
    }
    if matches!(tag, "img" | "source") {
        if let Some(srcset) = get("srcset") {
            for candidate in srcset_urls(&srcset) {
                found(Resource::Url(candidate.to_owned()));
            }
        }
    }
}

/// The URLs in a `srcset` attribute, such as `a.png 1x, b.png 2x`.
fn srcset_urls(srcset: &str) -> Vec<&str> {
    let mut urls = Vec::new();
    let mut rest = srcset;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
        if rest.is_empty() {
            return urls;
        }
        let end = rest
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(rest.len());
        let url = &rest[..end];
        urls.push(url.trim_end_matches(','));
        rest = &rest[end..];
        if !url.ends_with(',') {
            // Skip the descriptors, up to the next comma outside parentheses.
            let mut depth = 0usize;
            let end = rest
                .char_indices()
                .find(|&(_, c)| {
                    match c {
                        '(' => depth += 1,
                        ')' => depth = depth.saturating_sub(1),
                        ',' if depth == 0 => return true,
                        _ => {}
                    }
                    false
                })
                .map_or(rest.len(), |(i, _)| i);
            rest = &rest[end..];
        }
    }
}

/// The URLs CSS would load: `url()` arguments, and quoted strings that are
/// absolute URLs, as in `@import "…"` and `image-set("…" 1x)`.
fn css_resources(css: &str, found: &mut dyn FnMut(Resource)) {
    let css = strip_css_comments(css);
    let bytes = css.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        let b = bytes[pos];
        if b == b'"' || b == b'\'' {
            let (string, len) = css_string(&css[pos..]);
            found(Resource::AbsoluteUrl(string));
            pos += len;
        } else if starts_with_ignore_case(&bytes[pos..], b"url(")
            && !bytes[..pos]
                .last()
                .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            pos += 4;
            pos += bytes[pos..]
                .iter()
                .take_while(|b| b.is_ascii_whitespace())
                .count();
            let rest = &css[pos..];
            if rest.starts_with(['"', '\'']) {
                let (string, len) = css_string(rest);
                found(Resource::Url(string));
                pos += len;
            } else {
                let end = rest.find(')').unwrap_or(rest.len());
                found(Resource::Url(rest[..end].trim().to_owned()));
                pos += end;
            }
        } else {
            pos += 1;
        }
    }
}

/// Reads the CSS string at the start of `css`, which starts with its quote.
/// Returns its value and its length in `css`.
fn css_string(css: &str) -> (String, usize) {
    let mut chars = css.char_indices();
    let Some((_, quote)) = chars.next() else {
        return (String::new(), 0);
    };
    let mut value = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            c if c == quote => return (value, i + 1),
            // An unescaped newline ends a string, which is then invalid.
            '\n' => return (value, i),
            '\\' => {
                if let Some((_, escaped)) = chars.next() {
                    value.push(escaped);
                }
            }
            c => value.push(c),
        }
    }
    (value, css.len())
}

fn strip_css_comments(css: &str) -> Cow<'_, str> {
    if !css.contains("/*") {
        return Cow::Borrowed(css);
    }
    let mut stripped = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        stripped.push_str(&rest[..start]);
        stripped.push(' ');
        rest = rest[start + 2..]
            .find("*/")
            .map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    stripped.push_str(rest);
    Cow::Owned(stripped)
}

/// Reads one attribute of a start tag at `pos`, as the WHATWG "get an
/// attribute" algorithm does, with its name and value lowercased. Returns
/// `None`, after the `>`, at the end of the tag.
fn attribute(html: &str, pos: &mut usize) -> Option<(String, String)> {
    let bytes = html.as_bytes();
    let at = |pos: usize| bytes.get(pos).copied();
    while at(*pos).is_some_and(|b| is_space(b) || b == b'/') {
        *pos += 1;
    }
    match at(*pos) {
        None => return None,
        Some(b'>') => {
            *pos += 1;
            return None;
        }
        Some(_) => {}
    }
    let name_start = *pos;
    // The first character is part of the name even if it is "=".
    *pos += 1;
    while at(*pos).is_some_and(|b| !is_space(b) && !matches!(b, b'/' | b'>' | b'=')) {
        *pos += 1;
    }
    let name = html[name_start..*pos].to_ascii_lowercase();
    while at(*pos).is_some_and(is_space) {
        *pos += 1;
    }
    if at(*pos) != Some(b'=') {
        return Some((name, String::new()));
    }
    *pos += 1;
    while at(*pos).is_some_and(is_space) {
        *pos += 1;
    }
    let value = match at(*pos) {
        Some(quote @ (b'"' | b'\'')) => {
            let start = *pos + 1;
            let end = bytes[start..]
                .iter()
                .position(|&b| b == quote)
                .map_or(bytes.len(), |end| start + end);
            *pos = (end + 1).min(bytes.len());
            &html[start..end]
        }
        _ => {
            let start = *pos;
            while at(*pos).is_some_and(|b| !is_space(b) && b != b'>') {
                *pos += 1;
            }
            &html[start..*pos]
        }
    };
    Some((name, value.to_ascii_lowercase()))
}

/// Decodes the character references that matter for telling URLs apart:
/// numeric ones, and the named ones for `:` and `/`.
fn decode_char_refs(value: &str) -> Cow<'_, str> {
    if !value.contains('&') {
        return Cow::Borrowed(value);
    }
    let mut decoded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(amp) = rest.find('&') {
        decoded.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let (c, len) = char_ref(rest).unwrap_or(('&', 1));
        decoded.push(c);
        rest = &rest[len..];
    }
    decoded.push_str(rest);
    Cow::Owned(decoded)
}

/// The character reference at the start of `text`, and its length.
fn char_ref(text: &str) -> Option<(char, usize)> {
    for (name, c) in [
        ("&colon;", ':'),
        ("&sol;", '/'),
        ("&tab;", '\t'),
        ("&newline;", '\n'),
    ] {
        if text.starts_with(name) {
            return Some((c, name.len()));
        }
    }
    let (digits, radix, start) = match text.strip_prefix("&#")? {
        hex if hex.starts_with('x') => (&hex[1..], 16, 3),
        dec => (dec, 10, 2),
    };
    let len = digits
        .find(|c: char| !c.is_digit(radix))
        .unwrap_or(digits.len());
    let number = u32::from_str_radix(digits.get(..len.min(8))?, radix).ok()?;
    let c = char::from_u32(number).unwrap_or(char::REPLACEMENT_CHARACTER);
    let semicolon = usize::from(digits[len..].starts_with(';'));
    Some((c, start + len + semicolon))
}

/// Whether `url`, loaded from a Body whose base URL is remote or not, would
/// be fetched from a server.
fn is_remote(url: &str, base_is_remote: bool) -> bool {
    let url = clean_url(url);
    match scheme(&url) {
        Some(_) => is_network_url(&url),
        // Relative URLs resolve against the Body's own address, which has no
        // server, unless a <base> gives one.
        None => base_is_remote && !url.is_empty() && !url.starts_with('#'),
    }
}

/// Whether `url` is an absolute URL with a scheme fetched over the network.
fn is_network_url(url: &str) -> bool {
    let url = clean_url(url);
    scheme(&url).is_some_and(|scheme| {
        matches!(
            scheme.to_ascii_lowercase().as_str(),
            "http" | "https" | "ftp" | "ws" | "wss"
        )
    })
}

/// `url` as the URL parser sees it: without surrounding spaces and control
/// characters, and without tabs and newlines anywhere.
fn clean_url(url: &str) -> String {
    url.trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect()
}

/// The scheme of an absolute URL.
fn scheme(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next()?.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then_some(scheme)
}

/// The length of the comment at the start of `html`, as the HTML tokenizer
/// reads it: `<!-->` and `<!--->` are empty comments, and an unclosed one
/// runs to the end.
fn comment_len(html: &[u8]) -> usize {
    let body = &html[4..];
    if body.starts_with(b">") {
        return 5;
    }
    if body.starts_with(b"->") {
        return 6;
    }
    [&b"-->"[..], b"--!>"]
        .iter()
        .filter_map(|end| find(body, end).map(|at| 4 + at + end.len()))
        .min()
        .unwrap_or(html.len())
}

fn is_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

fn starts_with_ignore_case(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn find_ignore_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(html: &str) -> bool {
        references_remote_content(html)
    }

    #[test]
    fn policy_goes_first_in_head() {
        let blocked = prepare("<html><head><title>x</title>", RemoteContent::Blocked);
        assert!(blocked.starts_with(
            "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; "
        ));
        assert!(blocked.contains("<meta http-equiv=\"x-dns-prefetch-control\" content=\"off\">"));
        assert!(blocked.ends_with("\"off\"><html><head><title>x</title>"));
        let allowed = prepare("<p>x", RemoteContent::Allowed);
        assert!(allowed.starts_with(
            "<meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'none';"
        ));
        assert!(allowed.ends_with("<p>x"));
    }

    #[test]
    fn policy_goes_after_doctype_to_keep_standards_mode() {
        for (html, doctype_end) in [
            ("<!DOCTYPE html><html>", 15),
            ("\r\n<!doctype HTML>\r\n<HTML>", 17),
            ("<!-- saved --><!DOCTYPE html><html>", 29),
        ] {
            let prepared = prepare(html, RemoteContent::Blocked);
            assert_eq!(prepared[..doctype_end], html[..doctype_end]);
            assert!(prepared[doctype_end..].starts_with("<meta http-equiv=\"Content-Security"));
            assert!(prepared.ends_with(&html[doctype_end..]));
        }
        assert_eq!(head_insertion_point("<!-- x --><html>"), 0);
        assert_eq!(head_insertion_point("<!DOCTYPE html"), 14);
        assert_eq!(head_insertion_point(""), 0);
    }

    #[test]
    fn blocked_policy_allows_only_embedded_content_and_inline_attachments() {
        let directives: Vec<&str> = BLOCKED_POLICY.split("; ").collect();
        assert_eq!(directives[0], "default-src 'none'");
        for directive in &directives[1..] {
            let sources = directive.split_once(' ').unwrap().1;
            for source in sources.split(' ') {
                assert!(
                    matches!(source, "data:" | "cid:" | "'unsafe-inline'" | "'none'"),
                    "{directive}"
                );
            }
        }
        assert!(directives.contains(&"base-uri 'none'"));
    }

    #[test]
    fn finds_images() {
        assert!(remote("<img src=\"https://example.org/a.png\">"));
        assert!(remote("<IMG SRC=HTTP://example.org/a.png>"));
        assert!(remote("<img alt=x src='http://example.org/a.png'/>"));
        assert!(remote("<input type=image src=https://example.org/b.png>"));
        assert!(remote(
            "<svg><image href=\"https://example.org/c.svg\"/></svg>"
        ));
        assert!(remote(
            "<svg><use xlink:href=\"https://example.org/d.svg#x\"/></svg>"
        ));
        assert!(remote(
            "<table><tr><td background=\"https://example.org/bg.gif\">"
        ));
    }

    #[test]
    fn finds_srcset() {
        assert!(remote(
            "<img src=a.png srcset=\"https://example.org/a2.png 2x\">"
        ));
        assert!(remote(
            "<picture><source srcset=\"data:image/png;base64,AA, https://example.org/b.png 2x\">"
        ));
        assert!(!remote(
            "<img srcset=\"data:image/png;base64,AA 1x, cid:b 2x\">"
        ));
        assert_eq!(
            srcset_urls("a.png 1x,b.png 2x , data:x,y 100w, c.png"),
            ["a.png", "b.png", "data:x,y", "c.png"]
        );
        assert_eq!(
            srcset_urls("a.png (max-width: 1px, 2px) 1x, b.png"),
            ["a.png", "b.png"]
        );
    }

    #[test]
    fn finds_style_sheets_and_css_urls() {
        assert!(remote(
            "<link rel=\"stylesheet\" href=\"https://example.org/a.css\">"
        ));
        assert!(remote(
            "<link rel=preload as=image href=//cdn.example.org/a.png><base href=http://example.org/>"
        ));
        assert!(remote(
            "<style>@import url(https://fonts.example.org/css);</style>"
        ));
        assert!(remote(
            "<style>@import 'https://fonts.example.org/css';</style>"
        ));
        assert!(remote(
            "<style>@font-face { font-family: X; src: url( \"https://example.org/x.woff\" ) }</style>"
        ));
        assert!(remote(
            "<p style=\"background:URL(https://example.org/p.gif)\">"
        ));
        assert!(remote(
            "<div style='background-image: image-set(\"https://example.org/i.png\" 1x)'>"
        ));
        assert!(remote(
            "<p style=\"background:url(&#104;ttps&colon;//example.org/p.gif)\">"
        ));
    }

    #[test]
    fn ignores_what_never_loads() {
        assert!(!remote("<a href=\"https://example.org/\">link</a>"));
        assert!(!remote("<area href=\"https://example.org/\">"));
        assert!(!remote("<form action=\"https://example.org/\">"));
        assert!(!remote("<iframe src=\"https://example.org/\"></iframe>"));
        assert!(!remote(
            "<script src=\"https://example.org/a.js\"></script>"
        ));
        assert!(!remote(
            "<object data=\"https://example.org/a.swf\"></object>"
        ));
        assert!(!remote(
            "<video poster=\"https://example.org/p.png\"></video>"
        ));
        assert!(!remote(
            "<link rel=icon href=\"https://example.org/favicon.ico\">"
        ));
        assert!(!remote(
            "<link rel=preconnect href=\"https://example.org/\">"
        ));
        assert!(!remote(
            "<meta http-equiv=refresh content=\"0;url=https://example.org/\">"
        ));
    }

    #[test]
    fn ignores_embedded_and_inline_attachment_urls() {
        assert!(!remote("<img src=\"cid:image001.png@01D0A524.96D40F30\">"));
        assert!(!remote("<img src=\"data:image/png;base64,iVBORw0KGgo=\">"));
        assert!(!remote("<p style=\"behavior:url(#default#VML)\">"));
        assert!(!remote(
            "<img src=\"image001.png\"><img src=//example.org/a.png>"
        ));
        assert!(!remote("<style>p { content: 'see https://example.org' ; }"));
        assert!(!remote("<p>https://example.org/a.png</p>"));
    }

    #[test]
    fn style_content_is_not_markup() {
        assert!(!remote(
            "<style>p::after { content: \"<img src=x>\" }</style>"
        ));
        assert!(remote(
            "<style>/* <!-- */ p { background: url(https://example.org/a) }</style>"
        ));
        assert!(!remote("<style>/* url(https://example.org/a) */</style>"));
        assert!(!remote(
            "<title><img src=https://example.org/a></title><textarea><img src=https://example.org/b>"
        ));
        assert!(remote("<STYLE>x{}</STYLE><img src=https://example.org/a>"));
    }

    #[test]
    fn comments_hide_markup() {
        assert!(!remote("<!-- <img src=https://example.org/a> -->"));
        assert!(!remote(
            "<!--[if gte mso 9]><v:background src=\"https://example.org/a.png\"/><![endif]-->"
        ));
        assert!(remote("<!--><img src=https://example.org/a>"));
        assert!(remote(
            "<!--[if !mso]><!--><img src=https://example.org/a><!--<![endif]-->"
        ));
    }

    #[test]
    fn relative_urls_are_remote_only_with_a_remote_base() {
        assert!(!remote("<img src=\"images/a.png\">"));
        assert!(remote(
            "<base href=\"https://example.org/\"><img src=\"images/a.png\">"
        ));
        assert!(remote(
            "<img src=\"images/a.png\"><base href=\"https://example.org/\">"
        ));
        assert!(!remote(
            "<base href=\"https://example.org/\"><img src=\"#x\">"
        ));
        assert!(!remote("<base href=\"cid:x\"><img src=\"a.png\">"));
    }

    #[test]
    fn decodes_character_references() {
        assert_eq!(decode_char_refs("a&amp;b"), "a&amp;b");
        assert_eq!(decode_char_refs("&#104;ttp&#x3a;//x"), "http://x");
        assert_eq!(decode_char_refs("&#104ttp&colon;&sol;&sol;x"), "http://x");
        assert_eq!(decode_char_refs("&#99999999999;&"), "\u{fffd}&");
        assert!(is_remote(" ht\ntps://x ", false));
        assert!(!is_remote("javascript:x", true));
    }

    #[test]
    fn survives_truncated_markup() {
        let html = "<!DOCTYPE html><!-- c --><html><head><base href='https://x/'>\
                    <style>p{background:url('a.png')}</style></head>\
                    <body background=b.png><img srcset=\"c.png 1x, d.png 2x\" alt='é'>";
        for end in (0..=html.len()).filter(|&end| html.is_char_boundary(end)) {
            references_remote_content(&html[..end]);
            prepare(&html[..end], RemoteContent::Blocked);
        }
        assert!(remote(html));
    }
}
