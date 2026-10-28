//! Decoding an HTML Body stored as bytes.
//!
//! `PidTagHtml` is usually binary, and sometimes an 8-bit string, in
//! whatever charset the sender's mail client wrote it in. The charset is
//! taken, in order of preference, from a byte order mark, from a
//! `<meta charset>` or `<meta http-equiv="Content-Type">` in the HTML itself,
//! from `PidTagInternetCodepage`, and from `PidTagMessageCodepage`, falling
//! back to Windows-1252.
//!
//! The HTML is searched for a `<meta>` as browsers do it, following the
//! WHATWG "prescan a byte stream to determine its encoding" algorithm
//! (<https://html.spec.whatwg.org/multipage/parsing.html#prescan-a-byte-stream-to-determine-its-encoding>),
//! except that it looks further than the first 1024 bytes: up to the
//! `<body>` tag, as WebKit does, within [`MAX_PRESCAN_LEN`].

use encoding_rs::Encoding;

use crate::props::{DEFAULT_ENCODING, codepage_encoding};

/// How far into the HTML to look for a `<meta>` naming its charset.
const MAX_PRESCAN_LEN: usize = 64 * 1024;

/// Decodes an HTML Body stored as bytes. `codepages` are the codepages the
/// Message names for it, most preferred first; those that are missing or
/// name no usable encoding are skipped.
pub(crate) fn decode(bytes: &[u8], codepages: &[Option<i64>]) -> String {
    let encoding = meta_charset(bytes)
        .or_else(|| {
            codepages
                .iter()
                .filter_map(|&codepage| u16::try_from(codepage?).ok())
                .find_map(codepage_encoding)
        })
        .unwrap_or(DEFAULT_ENCODING);
    // A byte order mark wins over everything else, as in browsers.
    let mut html = encoding.decode(bytes).0.into_owned();
    html.truncate(html.trim_end_matches('\0').len());
    html
}

/// The encoding a `<meta>` element in `html` names, if any.
fn meta_charset(html: &[u8]) -> Option<&'static Encoding> {
    let html = &html[..html.len().min(MAX_PRESCAN_LEN)];
    let mut pos = 0;
    while pos < html.len() {
        let rest = &html[pos..];
        if rest.starts_with(b"<!--") {
            // The comment's "-->" may share its dashes with the "<!--".
            pos += 2 + find(&rest[2..], b"-->").map_or(rest.len(), |end| end + 3);
        } else if starts_with_ignore_case(rest, b"<meta")
            && rest.get(5).is_some_and(is_space_or_slash)
        {
            pos += 5;
            if let Some(encoding) = meta_element(html, &mut pos) {
                return Some(encoding);
            }
        } else if starts_with_ignore_case(rest, b"<body")
            && rest
                .get(5)
                .is_none_or(|&b| is_space_or_slash(&b) || b == b'>')
        {
            return None;
        } else if rest.len() >= 2
            && rest[0] == b'<'
            && (rest[1].is_ascii_alphabetic()
                || (rest[1] == b'/' && rest.get(2).is_some_and(u8::is_ascii_alphabetic)))
        {
            // Any other tag: skip its name and attributes.
            pos += 1;
            while pos < html.len() && !is_space_or_slash(&html[pos]) && html[pos] != b'>' {
                pos += 1;
            }
            while attribute(html, &mut pos).is_some() {}
        } else if rest.starts_with(b"<!") || rest.starts_with(b"</") || rest.starts_with(b"<?") {
            pos += find(rest, b">").map_or(rest.len(), |end| end + 1);
        } else {
            pos += 1;
        }
    }
    None
}

/// Reads the attributes of a `<meta>` element, starting after its name,
/// and returns the encoding it names, if any.
fn meta_element(html: &[u8], pos: &mut usize) -> Option<&'static Encoding> {
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut got_pragma = false;
    let mut need_pragma = None;
    // `Some(None)` is a label that names no encoding.
    let mut charset: Option<Option<&'static Encoding>> = None;
    while let Some((name, value)) = attribute(html, pos) {
        if seen.contains(&name) {
            continue;
        }
        match name.as_slice() {
            b"http-equiv" => got_pragma |= value == b"content-type",
            b"content" if charset.is_none() => {
                if let Some(label) = charset_from_content(&value) {
                    charset = Some(Encoding::for_label(&label));
                    need_pragma = Some(true);
                }
            }
            b"charset" if charset.is_none() => {
                charset = Some(Encoding::for_label(&value));
                need_pragma = Some(false);
            }
            _ => {}
        }
        seen.push(name);
    }
    if need_pragma? && !got_pragma {
        return None;
    }
    let encoding = charset??;
    // A <meta> naming UTF-16 was written in an ASCII-compatible encoding,
    // so it can't really be UTF-16.
    Some(
        if encoding == encoding_rs::UTF_16LE || encoding == encoding_rs::UTF_16BE {
            encoding_rs::UTF_8
        } else if encoding == encoding_rs::X_USER_DEFINED {
            encoding_rs::WINDOWS_1252
        } else {
            encoding
        },
    )
}

/// The WHATWG "get an attribute" algorithm: reads one attribute starting at
/// `pos`, with its name lowercased, or returns `None` at the end of the tag.
fn attribute(html: &[u8], pos: &mut usize) -> Option<(Vec<u8>, Vec<u8>)> {
    let at = |pos: usize| html.get(pos).copied();
    while at(*pos).is_some_and(|b| is_space_or_slash(&b)) {
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
    let mut name = Vec::new();
    let mut value = Vec::new();
    loop {
        match at(*pos) {
            None => return Some((name, value)),
            Some(b'=') if !name.is_empty() => {
                *pos += 1;
                break;
            }
            Some(b) if is_space(b) => {
                while at(*pos).is_some_and(is_space) {
                    *pos += 1;
                }
                if at(*pos) != Some(b'=') {
                    return Some((name, value));
                }
                *pos += 1;
                break;
            }
            Some(b'/' | b'>') => return Some((name, value)),
            Some(b) => {
                name.push(b.to_ascii_lowercase());
                *pos += 1;
            }
        }
    }
    while at(*pos).is_some_and(is_space) {
        *pos += 1;
    }
    match at(*pos) {
        None => {}
        Some(quote @ (b'"' | b'\'')) => {
            *pos += 1;
            while let Some(b) = at(*pos) {
                *pos += 1;
                if b == quote {
                    break;
                }
                value.push(b.to_ascii_lowercase());
            }
        }
        Some(b'>') => {}
        Some(_) => {
            while let Some(b) = at(*pos).filter(|&b| !is_space(b) && b != b'>') {
                value.push(b.to_ascii_lowercase());
                *pos += 1;
            }
        }
    }
    Some((name, value))
}

/// The WHATWG "extract a character encoding from a meta element" algorithm,
/// for the value of a `content` attribute such as
/// `text/html; charset=iso-8859-1`.
fn charset_from_content(content: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 0;
    loop {
        pos += find(&content[pos..], b"charset")? + b"charset".len();
        let mut rest = trim_start(&content[pos..]);
        if let Some(after) = rest.strip_prefix(b"=") {
            rest = trim_start(after);
            return match rest.first()? {
                &quote @ (b'"' | b'\'') => {
                    let value = &rest[1..];
                    let end = value.iter().position(|&b| b == quote)?;
                    Some(value[..end].to_vec())
                }
                _ => {
                    let end = rest
                        .iter()
                        .position(|&b| is_space(b) || b == b';')
                        .unwrap_or(rest.len());
                    Some(rest[..end].to_vec()).filter(|label| !label.is_empty())
                }
            };
        }
    }
}

fn is_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

fn is_space_or_slash(b: &u8) -> bool {
    is_space(*b) || *b == b'/'
}

fn trim_start(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|&b| !is_space(b))
        .unwrap_or(bytes.len());
    &bytes[start..]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn charset(html: &str) -> Option<&'static str> {
        meta_charset(html.as_bytes()).map(Encoding::name)
    }

    #[test]
    fn reads_meta_charset() {
        assert_eq!(charset("<meta charset=\"big5\">"), Some("Big5"));
        assert_eq!(charset("<META CHARSET=koi8-r>"), Some("KOI8-R"));
        assert_eq!(charset("<meta charset='utf-8'/>"), Some("UTF-8"));
    }

    #[test]
    fn reads_http_equiv_content_type() {
        assert_eq!(
            charset(
                "<html xmlns:v=\"urn:schemas-microsoft-com:vml\"><head>\
                 <meta http-equiv=Content-Type content=\"text/html; charset=iso-8859-1\">"
            ),
            Some("windows-1252")
        );
        assert_eq!(
            charset("<META CONTENT=\"text/html; charset=big5\" HTTP-EQUIV=\"Content-Type\">"),
            Some("Big5")
        );
        assert_eq!(
            charset("<meta http-equiv='content-type' content='text/html;charset = \"shift_jis\"'>"),
            Some("Shift_JIS")
        );
    }

    #[test]
    fn content_charset_needs_http_equiv() {
        assert_eq!(charset("<meta content=\"text/html; charset=big5\">"), None);
        assert_eq!(
            charset("<meta name=x content=\"charset=big5\"><meta charset=koi8-r>"),
            Some("KOI8-R")
        );
    }

    #[test]
    fn ignores_unknown_and_malformed_labels() {
        // As in Apache POI's HTMLBodyBinary samples.
        assert_eq!(charset("<html><meta charset=\\\"utf-8\\\"><body>"), None);
        assert_eq!(charset("<meta charset=\"no-such-charset\">"), None);
        assert_eq!(
            charset("<meta http-equiv=Content-Type content=\"text/html\">"),
            None
        );
    }

    #[test]
    fn utf_16_label_means_utf_8() {
        assert_eq!(charset("<meta charset=utf-16le>"), Some("UTF-8"));
        assert_eq!(
            charset("<meta charset=x-user-defined>"),
            Some("windows-1252")
        );
    }

    #[test]
    fn skips_comments_and_other_tags() {
        assert_eq!(
            charset(
                "<!-- <meta charset=big5> --><title x=\"<meta charset=koi8-r>\">\
                     <meta charset=gbk>"
            ),
            Some("GBK")
        );
        assert_eq!(charset("<!--> <meta charset=big5>"), Some("Big5"));
        assert_eq!(
            charset("<metaphor charset=big5><meta charset=gbk>"),
            Some("GBK")
        );
    }

    #[test]
    fn stops_at_body() {
        assert_eq!(charset("<head></head><body><meta charset=big5>"), None);
        assert_eq!(charset("<bodyx><meta charset=big5>"), Some("Big5"));
    }

    #[test]
    fn looks_past_long_styles() {
        let html = format!("<style>{}</style><meta charset=big5>", "p{}".repeat(1000));
        assert_eq!(charset(&html), Some("Big5"));
    }

    #[test]
    fn survives_truncated_markup() {
        for html in [
            "<meta",
            "<meta charset",
            "<meta charset=",
            "<meta charset=\"big",
            "<!--",
            "<a b='",
        ] {
            assert_eq!(charset(html), None);
        }
        assert_eq!(charset_from_content(b"text/html; charset=\"big5"), None);

        let html = b"<!DOCTYPE html><!-- x --><html a=1 b='2' c=\"3\" / d><head>\
                     <meta http-equiv=\"Content-Type\" content='text/html; charset=\"gbk\"'>";
        for end in 0..html.len() {
            let charset = meta_charset(&html[..end]);
            assert!(charset.is_none() || charset == Some(encoding_rs::GBK));
        }
        assert_eq!(meta_charset(html), Some(encoding_rs::GBK));
    }

    #[test]
    fn decodes_with_meta_then_codepages() {
        let big5 = b"<meta charset=big5>\xae\xe6\xa6\xa1";
        assert!(decode(big5, &[Some(1251), Some(1252)]).ends_with("格式"));
        let cp1251 = b"<p>\xcf\xf0\xe8\xe2\xe5\xf2";
        assert_eq!(decode(cp1251, &[None, Some(1251)]), "<p>Привет");
        assert_eq!(decode(cp1251, &[Some(1200), Some(1251)]), "<p>Привет");
        assert_eq!(decode(b"caf\xe9\0\0", &[]), "café");
    }

    #[test]
    fn byte_order_mark_wins() {
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(
                "<meta charset=big5>é"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        assert_eq!(decode(&utf16, &[Some(1252)]), "<meta charset=big5>é");
        assert_eq!(decode(b"\xef\xbb\xbfcaf\xc3\xa9", &[Some(1252)]), "café");
    }
}
