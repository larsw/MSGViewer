//! Turns a plain-text Body into an HTML document for the web view.

/// URL prefixes turned into links, matched without regard to case.
const LINK_PREFIXES: [&str; 3] = ["http://", "https://", "mailto:"];

/// Characters that end a URL when they come last, as in "see https://x.org."
const TRAILING_PUNCTUATION: &[char] = &['.', ',', ';', ':', '!', '?', '\'', '"', '*'];

/// Wraps `text` in a complete HTML document. The text is escaped, keeps its
/// line breaks, wraps long lines, and has its URLs turned into links.
///
/// The document's Content Security Policy forbids loading anything, so even
/// a bug in escaping cannot fetch from the network.
pub fn to_html(text: &str) -> String {
    let mut html = String::with_capacity(text.len() + 512);
    html.push_str(concat!(
        "<!DOCTYPE html>\n",
        "<html><head><meta charset=\"utf-8\">",
        "<meta http-equiv=\"Content-Security-Policy\" ",
        "content=\"default-src 'none'; style-src 'unsafe-inline'\">",
        "<style>body { margin: 12px 18px 18px; white-space: pre-wrap; ",
        "overflow-wrap: anywhere; }</style>",
        "</head><body>",
    ));
    push_linkified(&mut html, text);
    html.push_str("</body></html>\n");
    html
}

/// Appends `text` to `html`, escaped, with URLs turned into links.
fn push_linkified(html: &mut String, text: &str) {
    let mut rest = text;
    while let Some((start, end)) = find_link(rest) {
        push_escaped(html, &rest[..start]);
        let url = &rest[start..end];
        html.push_str("<a href=\"");
        push_escaped(html, url);
        html.push_str("\">");
        push_escaped(html, url);
        html.push_str("</a>");
        rest = &rest[end..];
    }
    push_escaped(html, rest);
}

/// Finds the first URL in `text`, as a byte range.
fn find_link(text: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    loop {
        let start = from + text[from..].find(['h', 'H', 'm', 'M'])?;
        let tail = &text[start..];
        let at_word_start = !text[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric());
        let prefix = LINK_PREFIXES.iter().find(|prefix| {
            tail.get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        });
        if let (true, Some(prefix)) = (at_word_start, prefix) {
            let len = url_len(tail);
            if len > prefix.len() {
                return Some((start, start + len));
            }
        }
        from = start + 1;
    }
}

/// The length in bytes of the URL at the start of `text`.
fn url_len(text: &str) -> usize {
    let end = text
        .find(|c: char| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"'))
        .unwrap_or(text.len());
    let mut url = &text[..end];
    loop {
        let trimmed = url.trim_end_matches(TRAILING_PUNCTUATION);
        let trimmed = [('(', ')'), ('[', ']'), ('{', '}')]
            .iter()
            .find_map(|&(open, close)| {
                let unbalanced = trimmed.ends_with(close)
                    && trimmed.matches(open).count() < trimmed.matches(close).count();
                unbalanced.then(|| &trimmed[..trimmed.len() - 1])
            })
            .unwrap_or(trimmed);
        if trimmed.len() == url.len() {
            return url.len();
        }
        url = trimmed;
    }
}

/// Appends `text` to `html`, escaped for use in element content or in a
/// double-quoted attribute value.
fn push_escaped(html: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '"' => html.push_str("&quot;"),
            '\'' => html.push_str("&#39;"),
            // NUL has no place in an HTML document.
            '\0' => {}
            c => html.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linkify(text: &str) -> String {
        let mut html = String::new();
        push_linkified(&mut html, text);
        html
    }

    #[test]
    fn escapes_markup() {
        assert_eq!(
            linkify("<b>Tom & \"Jerry\"</b> it's"),
            "&lt;b&gt;Tom &amp; &quot;Jerry&quot;&lt;/b&gt; it&#39;s"
        );
    }

    #[test]
    fn keeps_line_breaks_and_whitespace() {
        assert_eq!(linkify("a\r\n\r\n  b\tc\r\n"), "a\r\n\r\n  b\tc\r\n");
    }

    #[test]
    fn links_urls() {
        assert_eq!(
            linkify("See https://example.org/a?b=1&c=2 now"),
            "See <a href=\"https://example.org/a?b=1&amp;c=2\">\
             https://example.org/a?b=1&amp;c=2</a> now"
        );
    }

    #[test]
    fn links_mailto_and_http_in_any_case() {
        assert_eq!(
            linkify("MAILTO:ada@example.org HTTP://example.org"),
            "<a href=\"MAILTO:ada@example.org\">MAILTO:ada@example.org</a> \
             <a href=\"HTTP://example.org\">HTTP://example.org</a>"
        );
    }

    #[test]
    fn link_in_angle_brackets_stops_at_bracket() {
        assert_eq!(
            linkify("questions\r\n<http://r.example.org/r?clid=1033> . To"),
            "questions\r\n&lt;<a href=\"http://r.example.org/r?clid=1033\">\
             http://r.example.org/r?clid=1033</a>&gt; . To"
        );
    }

    #[test]
    fn trailing_punctuation_is_not_part_of_link() {
        assert_eq!(
            linkify("Go to https://example.org/x."),
            "Go to <a href=\"https://example.org/x\">https://example.org/x</a>."
        );
        assert_eq!(
            linkify("(see https://example.org/x)"),
            "(see <a href=\"https://example.org/x\">https://example.org/x</a>)"
        );
    }

    #[test]
    fn balanced_parentheses_stay_in_link() {
        assert_eq!(
            linkify("https://en.wikipedia.org/wiki/Mail_(disambiguation)"),
            "<a href=\"https://en.wikipedia.org/wiki/Mail_(disambiguation)\">\
             https://en.wikipedia.org/wiki/Mail_(disambiguation)</a>"
        );
    }

    #[test]
    fn ignores_bare_schemes_and_other_schemes() {
        assert_eq!(linkify("http:// alone"), "http:// alone");
        assert_eq!(
            linkify("javascript:alert(1) file:///etc/passwd"),
            "javascript:alert(1) file:///etc/passwd"
        );
    }

    #[test]
    fn scheme_must_start_a_word() {
        assert_eq!(linkify("xhttp://example.org"), "xhttp://example.org");
    }

    #[test]
    fn handles_non_ascii_around_links() {
        assert_eq!(
            linkify("Möte: https://example.org/å — ok"),
            "Möte: <a href=\"https://example.org/å\">https://example.org/å</a> — ok"
        );
    }

    #[test]
    fn document_wraps_escaped_text() {
        let html = to_html("<x>");
        assert!(html.contains("<body>&lt;x&gt;</body>"));
        assert!(html.contains("default-src 'none'"));
        assert!(html.contains("white-space: pre-wrap"));
    }
}
