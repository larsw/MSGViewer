//! Recovering an Encapsulated body from RTF ([MS-OXRTFEX] 2.2.3), and the
//! text of Genuine RTF.
//!
//! RTF is read with a small tokenizer, keeping track of what the
//! specification needs: groups, destinations, the font table's codepages,
//! the current font, `\htmlrtf` suppression and `\uc` skip counts. Anything
//! else, such as formatting, is ignored.
//!
//! Text is collected as bytes in the codepage of the current font, and
//! decoded once the codepage changes or a Unicode character comes along, so
//! that double-byte characters written as two `\'hh` escapes are decoded
//! together.
//!
//! [MS-OXRTFEX]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxrtfex/

use std::collections::HashMap;

use encoding_rs::Encoding;

use crate::props::codepage_encoding;

/// What the RTF was made from, as its first tokens say
/// ([MS-OXRTFEX] 2.2.3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// HTML (`\fromhtml1`): the RTF holds an Encapsulated HTML body.
    Html,
    /// Plain text (`\fromtext`): the RTF holds an Encapsulated plain-text
    /// body.
    Text,
    /// Neither: Genuine RTF.
    Rtf,
}

/// How many tokens at the start of the RTF may hold `\fromhtml1` or
/// `\fromtext` ([MS-OXRTFEX] 2.2.3.1).
const RECOGNITION_TOKENS: usize = 10;

/// Groups nested deeper than this share the state of the deepest one kept,
/// so a hostile file can't make the reader keep a state for each of
/// millions of groups. Real RTF nests a few dozen deep at most.
const MAX_DEPTH: usize = 512;

/// Fonts remembered from the font table; real documents have a few dozen.
const MAX_FONTS: usize = 4096;

/// Whether `rtf` starts as RTF does, with `{\rtf`.
pub(crate) fn is_rtf(rtf: &[u8]) -> bool {
    let mut tokens = Tokenizer::new(rtf);
    matches!(tokens.next(), Some(Token::GroupStart))
        && matches!(tokens.next(), Some(Token::Word { name: b"rtf", .. }))
}

/// What `rtf` was made from: HTML or plain text if a `\fromhtml1` or
/// `\fromtext` comes within its first ten tokens, all of them group starts
/// or control words; otherwise Genuine RTF.
pub(crate) fn origin(rtf: &[u8]) -> Origin {
    for token in Tokenizer::new(rtf).take(RECOGNITION_TOKENS) {
        match token {
            Token::Word {
                name: b"fromhtml",
                param: Some(1),
            } => return Origin::Html,
            Token::Word {
                name: b"fromtext", ..
            } => return Origin::Text,
            Token::GroupStart | Token::Word { .. } | Token::Symbol(_) => {}
            _ => break,
        }
    }
    Origin::Rtf
}

/// The Encapsulated body in `rtf`, or the text of Genuine RTF: HTML if
/// `origin` is [`Origin::Html`], else plain text.
pub(crate) fn extract(rtf: &[u8], origin: Origin) -> String {
    let mut reader = RtfReader::new(origin == Origin::Html);
    for token in Tokenizer::new(rtf) {
        if !reader.token(token) {
            break;
        }
    }
    let mut text = reader.out.finish();
    text.truncate(text.trim_end_matches('\0').len());
    text
}

/// One RTF token. CR and LF between tokens are not tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token<'a> {
    GroupStart,
    GroupEnd,
    /// A control word, such as `\par` or `\fs20`.
    Word {
        name: &'a [u8],
        param: Option<i32>,
    },
    /// A control symbol, such as `\~` or `\*`, other than the escapes of
    /// `\`, `{` and `}`, which come as text.
    Symbol(u8),
    /// A byte written as `\'hh`.
    Hex(u8),
    /// The data following `\binN`.
    Binary(&'a [u8]),
    /// A run of text.
    Text(&'a [u8]),
}

struct Tokenizer<'a> {
    rtf: &'a [u8],
    pos: usize,
}

/// Control words are at most 32 letters ([MSFT-RTF]); longer runs of
/// letters are cut there.
const MAX_WORD_LEN: usize = 32;
/// Parameters are 16- or 32-bit numbers; more digits than this are cut.
const MAX_PARAM_DIGITS: usize = 10;

impl<'a> Tokenizer<'a> {
    fn new(rtf: &'a [u8]) -> Self {
        Tokenizer { rtf, pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.rtf.get(self.pos).copied()
    }

    /// Takes bytes while `keep` holds, up to `max` of them.
    fn take_while(&mut self, max: usize, keep: impl Fn(u8) -> bool) -> &'a [u8] {
        let start = self.pos;
        while self.pos - start < max && self.peek().is_some_and(&keep) {
            self.pos += 1;
        }
        &self.rtf[start..self.pos]
    }

    /// Reads what follows a backslash.
    fn control(&mut self) -> Option<Token<'a>> {
        let first = self.peek()?;
        if !first.is_ascii_alphabetic() {
            self.pos += 1;
            return Some(match first {
                b'\\' | b'{' | b'}' => Token::Text(&self.rtf[self.pos - 1..self.pos]),
                b'\'' => {
                    let digits = self.take_while(2, |b| b.is_ascii_hexdigit());
                    let hex = std::str::from_utf8(digits).ok();
                    match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                        Some(byte) if digits.len() == 2 => Token::Hex(byte),
                        _ => Token::Symbol(b'\''),
                    }
                }
                // A backslash before a line break is a paragraph mark.
                b'\r' | b'\n' => Token::Word {
                    name: b"par",
                    param: None,
                },
                other => Token::Symbol(other),
            });
        }
        let name = self.take_while(MAX_WORD_LEN, |b| b.is_ascii_alphabetic());
        // Any letters past the limit are left to come as text.
        let negative =
            self.peek() == Some(b'-') && self.rtf.get(self.pos + 1).is_some_and(u8::is_ascii_digit);
        if negative {
            self.pos += 1;
        }
        let digits = self.take_while(MAX_PARAM_DIGITS, |b| b.is_ascii_digit());
        let param = (!digits.is_empty()).then(|| {
            let value = digits
                .iter()
                .fold(0i64, |n, &d| n * 10 + i64::from(d - b'0'));
            let value = if negative { -value } else { value };
            value.clamp(i32::MIN.into(), i32::MAX.into()) as i32
        });
        // A space ends a control word, and belongs to it.
        if self.peek() == Some(b' ') {
            self.pos += 1;
        }
        if name == b"bin" {
            let len = usize::try_from(param.unwrap_or(0)).unwrap_or(0);
            let len = len.min(self.rtf.len() - self.pos);
            self.pos += len;
            return Some(Token::Binary(&self.rtf[self.pos - len..self.pos]));
        }
        Some(Token::Word { name, param })
    }
}

impl<'a> Iterator for Tokenizer<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        loop {
            let byte = self.peek()?;
            self.pos += 1;
            return match byte {
                b'{' => Some(Token::GroupStart),
                b'}' => Some(Token::GroupEnd),
                b'\\' => match self.control() {
                    Some(token) => Some(token),
                    None => continue,
                },
                b'\r' | b'\n' => continue,
                _ => {
                    self.pos -= 1;
                    let text = self.take_while(usize::MAX, |b| {
                        !matches!(b, b'{' | b'}' | b'\\' | b'\r' | b'\n')
                    });
                    Some(Token::Text(text))
                }
            };
        }
    }
}

/// Where the text in a group goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destination {
    /// The document's text.
    Text,
    /// The font table, which gives each font's codepage.
    FontTable,
    /// An `\*\htmltag` group, holding HTML that RTF can't represent.
    HtmlTag,
    /// Anything that doesn't produce visible text, which is skipped.
    Skip,
}

/// What a group starts out with, and gets back when a group inside it ends.
#[derive(Clone, Copy, Debug)]
struct Group {
    destination: Destination,
    /// Whether `\htmlrtf` suppresses this part of the RTF.
    suppressed: bool,
    /// The current font, or `None` for the default font.
    font: Option<i32>,
    /// How many fallback characters follow a `\uN` (`\ucN`).
    unicode_skip: u32,
    /// Whether text is hidden (`\v`).
    hidden: bool,
    /// Whether no token but `\*` has come in this group yet, so a control
    /// word would name its destination.
    at_start: bool,
    /// Whether the group started with `\*`, making it an ignorable
    /// destination.
    ignorable: bool,
}

/// Destinations that hold no visible text: tables, metadata, pictures,
/// field instructions, headers and footers, and the like.
const SKIPPED_DESTINATIONS: &[&[u8]] = &[
    b"annotation",
    b"atnauthor",
    b"atnid",
    b"author",
    b"buptim",
    b"colortbl",
    b"comment",
    b"creatim",
    b"doccomm",
    b"falt",
    b"field_instruction_placeholder",
    b"file",
    b"filetbl",
    b"fldinst",
    b"footer",
    b"footerf",
    b"footerl",
    b"footerr",
    b"footnote",
    b"ftncn",
    b"ftnsep",
    b"ftnsepc",
    b"header",
    b"headerf",
    b"headerl",
    b"headerr",
    b"info",
    b"keywords",
    b"listoverridetable",
    b"listtable",
    b"nonshppict",
    b"objclass",
    b"objdata",
    b"objname",
    b"operator",
    b"pict",
    b"printim",
    b"private",
    b"revtbl",
    b"revtim",
    b"rsidtbl",
    b"rxe",
    b"shpinst",
    b"stylesheet",
    b"subject",
    b"tc",
    b"title",
    b"txe",
    b"xe",
    b"xmlnstbl",
];

/// Control words that stand for a character.
fn character(name: &[u8]) -> Option<char> {
    Some(match name {
        b"lquote" => '\u{2018}',
        b"rquote" => '\u{2019}',
        b"ldblquote" => '\u{201C}',
        b"rdblquote" => '\u{201D}',
        b"bullet" => '\u{2022}',
        b"endash" => '\u{2013}',
        b"emdash" => '\u{2014}',
        b"enspace" => '\u{2002}',
        b"emspace" => '\u{2003}',
        b"qmspace" => '\u{2005}',
        b"zwj" => '\u{200D}',
        b"zwnj" => '\u{200C}',
        b"ltrmark" => '\u{200E}',
        b"rtlmark" => '\u{200F}',
        _ => return None,
    })
}

/// The codepage of an RTF font charset (`\fcharsetN`), from [MSFT-RTF].
fn charset_codepage(charset: i32) -> Option<u16> {
    Some(match charset {
        0 => 1252,
        // The Symbol charset has no codepage; Windows-1252 at least keeps
        // its letters and digits.
        2 => 1252,
        77 => 10000,
        78 => 10001,
        79 => 10003,
        80 => 10008,
        81 => 10002,
        83 => 10005,
        84 => 10004,
        85 => 10006,
        86 => 10081,
        87 => 10021,
        88 => 10029,
        89 => 10007,
        128 => 932,
        129 => 949,
        130 => 1361,
        134 => 936,
        136 => 950,
        161 => 1253,
        162 => 1254,
        163 => 1258,
        177 => 1255,
        178 => 1256,
        186 => 1257,
        204 => 1251,
        222 => 874,
        238 => 1250,
        254 => 437,
        255 => 850,
        _ => return None,
    })
}

fn encoding_for(codepage: i32) -> Option<&'static Encoding> {
    codepage_encoding(u16::try_from(codepage).ok()?)
}

/// Reads RTF tokens one at a time, collecting the text they make.
struct RtfReader {
    /// Whether to recover HTML, from `\*\htmltag` groups and text not
    /// suppressed by `\htmlrtf`, rather than plain text.
    html: bool,
    group: Group,
    /// The groups around the current one.
    outer: Vec<Group>,
    /// Groups opened past [`MAX_DEPTH`], which share the current state.
    overflow: usize,
    /// The document's codepage (`\ansicpgN`).
    codepage: &'static Encoding,
    /// The default font (`\deffN`).
    default_font: i32,
    /// The codepage of each font in the font table that names one.
    fonts: HashMap<i32, &'static Encoding>,
    /// The font the font table is describing.
    table_font: Option<i32>,
    /// Fallback characters still to skip after a `\uN`.
    skip: u32,
    out: Output,
}

impl RtfReader {
    fn new(html: bool) -> Self {
        RtfReader {
            html,
            group: Group {
                destination: Destination::Text,
                suppressed: false,
                font: None,
                unicode_skip: 1,
                hidden: false,
                at_start: false,
                ignorable: false,
            },
            outer: Vec::new(),
            overflow: 0,
            codepage: encoding_rs::WINDOWS_1252,
            default_font: 0,
            fonts: HashMap::new(),
            table_font: None,
            skip: 0,
            out: Output::new(),
        }
    }

    /// The encoding of text in the current font.
    fn font_encoding(&self) -> &'static Encoding {
        let font = self.group.font.unwrap_or(self.default_font);
        self.fonts.get(&font).copied().unwrap_or(self.codepage)
    }

    /// Handles one token; returns false at the end of the document.
    fn token(&mut self, token: Token<'_>) -> bool {
        match token {
            Token::GroupStart => {
                self.skip = 0;
                if self.outer.len() < MAX_DEPTH {
                    self.outer.push(self.group);
                    self.group.at_start = true;
                    self.group.ignorable = false;
                } else {
                    self.overflow += 1;
                }
                return true;
            }
            Token::GroupEnd => {
                self.skip = 0;
                if self.overflow > 0 {
                    self.overflow -= 1;
                    return true;
                }
                return match self.outer.pop() {
                    Some(group) => {
                        self.group = group;
                        // The group that ends the document.
                        !self.outer.is_empty()
                    }
                    None => false,
                };
            }
            _ => {}
        }

        if self.skip > 0 {
            match token {
                Token::Text(text) => {
                    let skipped = text.len().min(self.skip as usize);
                    self.skip -= skipped as u32;
                    if skipped < text.len() {
                        self.text_token(Token::Text(&text[skipped..]));
                    }
                }
                _ => self.skip -= 1,
            }
            return true;
        }

        let at_start = std::mem::replace(&mut self.group.at_start, false);
        match token {
            Token::Symbol(b'*') if at_start => {
                self.group.ignorable = true;
                self.group.at_start = true;
            }
            Token::Word { name, .. } if at_start => {
                if let Some(destination) = self.destination(name) {
                    self.group.destination = destination;
                } else {
                    self.text_token(token);
                }
            }
            _ => self.text_token(token),
        }
        true
    }

    /// The destination a group starting with control word `name` becomes,
    /// or `None` if it stays as it is.
    fn destination(&self, name: &[u8]) -> Option<Destination> {
        let current = self.group.destination;
        if current == Destination::Skip {
            return None;
        }
        if name == b"htmltag" && self.html && current != Destination::FontTable {
            return Some(Destination::HtmlTag);
        }
        if self.group.ignorable || SKIPPED_DESTINATIONS.contains(&name) {
            return Some(Destination::Skip);
        }
        (name == b"fonttbl").then_some(Destination::FontTable)
    }

    /// Handles a token other than a group start or end.
    fn text_token(&mut self, token: Token<'_>) {
        match self.group.destination {
            Destination::Skip => {}
            Destination::FontTable => self.font_table_token(token),
            Destination::HtmlTag => self.html_tag_token(token),
            Destination::Text => self.body_token(token),
        }
    }

    fn font_table_token(&mut self, token: Token<'_>) {
        let Token::Word {
            name,
            param: Some(param),
        } = token
        else {
            return;
        };
        let codepage = match name {
            b"f" => {
                self.table_font = Some(param);
                return;
            }
            b"fcharset" => charset_codepage(param).and_then(|cp| encoding_for(cp.into())),
            b"cpg" => encoding_for(param),
            _ => return,
        };
        if let (Some(font), Some(codepage)) = (self.table_font, codepage) {
            if self.fonts.len() < MAX_FONTS || self.fonts.contains_key(&font) {
                self.fonts.insert(font, codepage);
            }
        }
    }

    /// Handles a token in an `\*\htmltag` group, whose text is in the
    /// document's codepage ([MS-OXRTFEX] 2.2.3.2).
    fn html_tag_token(&mut self, token: Token<'_>) {
        match token {
            Token::Text(text) => self.out.bytes(text, self.codepage),
            Token::Hex(byte) => self.out.bytes(&[byte], self.codepage),
            Token::Symbol(b'_') => self.out.char('\u{00AD}'),
            _ => self.common_token(token),
        }
    }

    /// Handles a token in the document's text.
    fn body_token(&mut self, token: Token<'_>) {
        if let Token::Word { name, param } = token {
            match name {
                b"htmlrtf" if self.html => {
                    self.group.suppressed = param != Some(0);
                    return;
                }
                // The current font is tracked even where \htmlrtf
                // suppresses it.
                b"f" => {
                    self.group.font = param;
                    return;
                }
                _ => {}
            }
        }
        if self.group.suppressed {
            return;
        }
        if let Token::Word { name, param } = token {
            match name {
                b"ansi" => self.codepage = encoding_rs::WINDOWS_1252,
                b"mac" => self.codepage = encoding_for(10000).unwrap_or(self.codepage),
                b"pc" => self.codepage = encoding_for(437).unwrap_or(self.codepage),
                b"pca" => self.codepage = encoding_for(850).unwrap_or(self.codepage),
                b"ansicpg" => {
                    self.codepage = param.and_then(encoding_for).unwrap_or(self.codepage);
                }
                b"deff" => self.default_font = param.unwrap_or(0),
                b"plain" => {
                    self.group.font = None;
                    self.group.hidden = false;
                }
                b"uc" => self.group.unicode_skip = param.unwrap_or(1).max(0) as u32,
                b"v" if !self.html => self.group.hidden = param != Some(0),
                _ => {}
            }
        }
        if self.group.hidden {
            return;
        }
        match token {
            Token::Word {
                name: b"cell" | b"nestcell",
                ..
            } if !self.html => self.out.str("\t"),
            Token::Word {
                name: b"row" | b"nestrow" | b"sect" | b"page",
                ..
            } if !self.html => self.out.str("\r\n"),
            Token::Text(text) => self.out.bytes(text, self.font_encoding()),
            Token::Hex(byte) => self.out.bytes(&[byte], self.font_encoding()),
            Token::Symbol(b'_') => self.out.char('\u{2011}'),
            _ => self.common_token(token),
        }
    }

    /// Handles the tokens that mean the same in and out of `\*\htmltag`
    /// groups: line breaks, tabs, Unicode and other characters.
    fn common_token(&mut self, token: Token<'_>) {
        match token {
            Token::Word { name, param } => match name {
                b"par" | b"line" => self.out.str("\r\n"),
                b"tab" => self.out.str("\t"),
                b"uc" => self.group.unicode_skip = param.unwrap_or(1).max(0) as u32,
                b"u" => {
                    if let Some(code) = param {
                        // Code points above U+7FFF are written negative.
                        let code = if code < 0 { code + 0x10000 } else { code };
                        self.out.unicode(code as u32);
                        self.skip = self.group.unicode_skip;
                    }
                }
                _ => {
                    if let Some(c) = character(name) {
                        self.out.char(c);
                    }
                }
            },
            Token::Symbol(b'~') => self.out.char('\u{00A0}'),
            Token::Symbol(b'-') => self.out.char('\u{00AD}'),
            _ => {}
        }
    }
}

/// Text being collected, with bytes and UTF-16 code units waiting to be
/// decoded.
struct Output {
    text: String,
    bytes: Vec<u8>,
    encoding: &'static Encoding,
    units: Vec<u16>,
}

impl Output {
    fn new() -> Self {
        Output {
            text: String::new(),
            bytes: Vec::new(),
            encoding: encoding_rs::WINDOWS_1252,
            units: Vec::new(),
        }
    }

    fn bytes(&mut self, bytes: &[u8], encoding: &'static Encoding) {
        self.decode_units();
        if encoding != self.encoding {
            self.decode_bytes();
            self.encoding = encoding;
        }
        self.bytes.extend_from_slice(bytes);
    }

    /// Adds a character from `\uN`. Characters past U+FFFF come as two
    /// surrogates, which are decoded together.
    fn unicode(&mut self, code: u32) {
        self.decode_bytes();
        match u16::try_from(code) {
            Ok(unit) => self.units.push(unit),
            Err(_) => self.char(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER)),
        }
    }

    fn char(&mut self, c: char) {
        self.decode_bytes();
        self.decode_units();
        self.text.push(c);
    }

    fn str(&mut self, s: &str) {
        self.decode_bytes();
        self.decode_units();
        self.text.push_str(s);
    }

    fn decode_bytes(&mut self) {
        if !self.bytes.is_empty() {
            let (text, _) = self.encoding.decode_without_bom_handling(&self.bytes);
            self.text.push_str(&text);
            self.bytes.clear();
        }
    }

    fn decode_units(&mut self) {
        if !self.units.is_empty() {
            self.text.push_str(&String::from_utf16_lossy(&self.units));
            self.units.clear();
        }
    }

    fn finish(mut self) -> String {
        self.decode_bytes();
        self.decode_units();
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(rtf: &str) -> String {
        extract(rtf.as_bytes(), Origin::Html)
    }

    fn text(rtf: &[u8]) -> String {
        extract(rtf, Origin::Text)
    }

    fn tokens(rtf: &str) -> Vec<Token<'_>> {
        Tokenizer::new(rtf.as_bytes()).collect()
    }

    #[test]
    fn tokenizes_control_words_symbols_and_text() {
        assert_eq!(
            tokens("{\\rtf1\\ansi \\fs-20 a b\r\n\\'e9\\~\\{x}"),
            [
                Token::GroupStart,
                Token::Word {
                    name: b"rtf",
                    param: Some(1)
                },
                Token::Word {
                    name: b"ansi",
                    param: None
                },
                Token::Word {
                    name: b"fs",
                    param: Some(-20)
                },
                Token::Text(b"a b"),
                Token::Hex(0xE9),
                Token::Symbol(b'~'),
                Token::Text(b"{"),
                Token::Text(b"x"),
                Token::GroupEnd,
            ]
        );
    }

    #[test]
    fn tokenizer_survives_truncated_and_odd_input() {
        for rtf in ["\\", "\\'", "\\'4", "\\'zz", "\\f-", "\\bin99 ab", "\\u-"] {
            let _ = tokens(rtf);
        }
        assert_eq!(
            tokens("\\bin3 {}\\x"),
            [Token::Binary(b"{}\\"), Token::Text(b"x")]
        );
        assert_eq!(
            tokens("\\f99999999999"),
            [
                Token::Word {
                    name: b"f",
                    param: Some(i32::MAX)
                },
                Token::Text(b"9")
            ]
        );
        assert_eq!(tokens("\\'4g"), [Token::Symbol(b'\''), Token::Text(b"g")]);
    }

    #[test]
    fn recognizes_origin_in_first_ten_tokens() {
        assert_eq!(
            origin(b"{\\rtf1\\ansi\\ansicpg1252\\fromhtml1 \\deff0"),
            Origin::Html
        );
        assert_eq!(
            origin(b"{\\rtf1\\ansi\\ansicpg1251\\fromtext \\deff0"),
            Origin::Text
        );
        assert_eq!(
            origin(b"{\\rtf1\\ansi\\fbidis\\ansicpg1252\\deff0\\fromhtml1{\\fonttbl"),
            Origin::Html
        );
        // Only \fromhtml1 counts.
        assert_eq!(origin(b"{\\rtf1\\fromhtml \\fromhtml0"), Origin::Rtf);
        // Text before it, or too many tokens.
        assert_eq!(origin(b"{\\rtf1 x\\fromhtml1"), Origin::Rtf);
        assert_eq!(
            origin(b"{\\rtf1\\a\\b\\c\\d\\e\\f\\g\\h\\i\\fromhtml1"),
            Origin::Rtf
        );
        assert_eq!(
            origin(b"{\\rtf1\\a\\b\\c\\d\\e\\f\\g\\fromhtml1"),
            Origin::Html
        );
        assert_eq!(origin(b""), Origin::Rtf);
    }

    #[test]
    fn checks_rtf_heading() {
        assert!(is_rtf(b"{\\rtf1\\ansi}"));
        assert!(is_rtf(b"\r\n{\\rtf"));
        assert!(!is_rtf(b"{\\rtx1}"));
        assert!(!is_rtf(b"<html>"));
        assert!(!is_rtf(b""));
    }

    /// [MS-OXRTFEX] 3.1, with its typos ("a bold text", "which have")
    /// as they are.
    const SPEC_EXAMPLE: &str = r#"{\rtf1\ANSI\ansicpg1251\fromhtml1 \deff0
{\fonttbl {\f0\fmodern Courier New;}{\f1\fswiss Arial;}{\f2\fswiss\fcharset0 Arial;}}
{\colortbl\red0\green0\blue0;\red0\green0\blue255;}
{\*\htmltag64}
\uc1\pard\plain\deftab360 \f0\fs24
{\*\htmltag <HTML><head>\par
<style>\par
<!--\par
/* Style Definitions */\par
p.MsoNormal, li.MsoNormal \{font-family:Arial;\}\par
-->\par
</style>\par
\tab <!-- This is a HTML comment.\par
There is a horizontal tab (%x09) character before the comment, \par
and some new lines inside the comment. -->\par
</head>\par
<body>\par
<p\par
class="MsoNormal">}
{\htmlrtf \f1 \htmlrtf0 Note the line break inside a P tag. {\*\htmltag <b>}{\htmlrtf \b \htmlrtf0 This is a bold text{\*\htmltag </b>}} \htmlrtf\par\htmlrtf0}
\htmlrtf \par \htmlrtf0
{\*\htmltag </p>\par
<p class="MsoNormal">\par}
{\htmlrtf \f1 \htmlrtf0 This is a normal text with a character references:
{\*\htmltag &nbsp;}\htmlrtf \'a0\htmlrtf0  {\*\htmltag &lt;}\htmlrtf <\htmlrtf0  {\*\htmltag &uml;}\htmlrtf {\f2\'a8}\htmlrtf0{\*\htmltag <br>\par}\htmlrtf\line\htmlrtf0
characters which have special meaning in RTF: \{\}\\{\*\htmltag <br>\par}\htmlrtf\line\htmlrtf0\htmlrtf\par\htmlrtf0}
{\*\htmltag </p>\par
<ol>\par
<li class="MsoNormal">}{\htmlrtf {{\*\pn\pnlvlbody\pndec\pnstart1\pnindent360{\pntxta.}}\li360\fi-360{\pntext 1.\tab} \f1 \htmlrtf0 This is a list item}\htmlrtf\par\htmlrtf0}
{\*\htmltag \par
</ol>\par
</body>\par
</HTML>\par }}"#;

    #[test]
    fn recovers_html_of_specification_example() {
        assert_eq!(origin(SPEC_EXAMPLE.as_bytes()), Origin::Html);
        // As the specification's source HTML, except where the RTF differs
        // from it: a CRLF for each line break, the two typos, and no space
        // before "&nbsp;", where the RTF has a line break instead.
        let expected = "<HTML><head>\r\n\
<style>\r\n\
<!--\r\n\
/* Style Definitions */\r\n\
p.MsoNormal, li.MsoNormal {font-family:Arial;}\r\n\
-->\r\n\
</style>\r\n\
\t<!-- This is a HTML comment.\r\n\
There is a horizontal tab (%x09) character before the comment, \r\n\
and some new lines inside the comment. -->\r\n\
</head>\r\n\
<body>\r\n\
<p\r\n\
class=\"MsoNormal\">Note the line break inside a P tag. <b>This is a bold text</b> </p>\r\n\
<p class=\"MsoNormal\">\r\n\
This is a normal text with a character references:&nbsp; &lt; &uml;<br>\r\n\
characters which have special meaning in RTF: {}\\<br>\r\n\
</p>\r\n\
<ol>\r\n\
<li class=\"MsoNormal\">This is a list item\r\n\
</ol>\r\n\
</body>\r\n\
</HTML>\r\n";
        assert_eq!(html(SPEC_EXAMPLE), expected);
    }

    #[test]
    fn htmlrtf_state_follows_groups() {
        let rtf = "{\\rtf1\\fromhtml1 a\\htmlrtf b{c\\htmlrtf0 d}e\\htmlrtf0 f{\\htmlrtf g}h}";
        assert_eq!(html(rtf), "adfh");
        assert_eq!(html("{\\rtf1\\fromhtml1 a\\htmlrtf1 b\\htmlrtf0 c}"), "ac");
    }

    #[test]
    fn htmltag_parameter_is_not_content() {
        let rtf = "{\\rtf1\\fromhtml1 {\\*\\htmltag84 <b>}{\\*\\htmltag148 7}{\\*\\htmltag64}\
                   {\\*\\htmltag 12}{\\*\\mhtmltag84 <img src=\"cid:x\">}}";
        assert_eq!(html(rtf), "<b>712");
    }

    #[test]
    fn skips_ignorable_and_invisible_destinations() {
        let rtf = "{\\rtf1\\fromhtml1 {\\colortbl\\red0;}{\\*\\generator Word;}\
                   {\\info{\\title T}}{\\stylesheet{\\s0 Normal;}}{\\pict 0102}\
                   {\\*\\unknown x{y}}{\\field{\\*\\fldinst HYPERLINK \"x\"}{\\fldrslt link}}ok}";
        assert_eq!(html(rtf), "linkok");
    }

    #[test]
    fn htmltag_text_uses_document_codepage_and_body_text_its_font() {
        // A Cyrillic document codepage, with font 1 in Greek.
        let rtf = b"{\\rtf1\\ansi\\ansicpg1251\\fromhtml1 \\deff0{\\fonttbl{\\f0\\fswiss Arial;}\
                    {\\f1\\fcharset161 Arial Greek;}}{\\*\\htmltag <p title=\"\\'cf\xcf\">}\
                    \\'cf{\\f1 \\'e1\xe1}{\\htmlrtf\\f1\\htmlrtf0 \\'e1}}";
        assert_eq!(extract(rtf, Origin::Html), "<p title=\"ПП\">Пααα");
    }

    #[test]
    fn decodes_double_byte_characters_split_over_escapes() {
        let rtf = b"{\\rtf1\\ansi\\ansicpg950\\deff0{\\fonttbl{\\f0\\fcharset136 \\'b7\\'73;}}\
                    \\'ae\\'e6\\'a6\\'a1}";
        assert_eq!(text(rtf), "格式");
    }

    #[test]
    fn decodes_utf_8_codepage() {
        let rtf = "{\\rtf1\\ansi\\ansicpg65001\\fromhtml1 {\\*\\htmltag <p>}Grüße\\'c3\\'a9}";
        assert_eq!(html(rtf), "<p>Grüßeé");
    }

    #[test]
    fn decodes_unicode_with_skip_counts() {
        // \uc1 by default; \uc0 and \uc2 change it for the group.
        let rtf =
            b"{\\rtf1\\fromtext \\u8364?a{\\uc2\\u8364\\'80\\'80b}{\\uc0\\u8364 c}\\u-3913 ?d}";
        assert_eq!(text(rtf), "€a€b€c\u{F0B7}d");
        // Fallback text cut short by the end of a group.
        assert_eq!(text(b"{\\rtf1 {\\uc3\\u8364 x}y}"), "€y");
        // A control word counts as one fallback character.
        assert_eq!(text(b"{\\rtf1 \\u8364\\'80 \\u8364\\par x}"), "€ €x");
        // A surrogate pair.
        assert_eq!(text(b"{\\rtf1 \\u-10179?\\u-8704?}"), "😀");
        assert_eq!(text(b"{\\rtf1 \\u-10179?x}"), "\u{FFFD}x");
    }

    #[test]
    fn maps_control_words_to_text() {
        let rtf =
            b"{\\rtf1\\fromtext a\\par b\\line c\\tab d\\lquote\\rquote\\ldblquote\\rdblquote\
                    \\bullet\\endash\\emdash\\~\\_\\-\\{\\}\\\\\\b\\i0\\fs20 e\\\r\nf}";
        assert_eq!(
            text(rtf),
            "a\r\nb\r\nc\td‘’“”•–—\u{A0}\u{2011}\u{AD}{}\\e\r\nf"
        );
    }

    #[test]
    fn genuine_rtf_text_leaves_out_tables_and_hidden_text() {
        let rtf = b"{\\rtf1\\ansi\\ansicpg1252\\deff0\\nouicompat\\deflang1033{\\fonttbl{\\f0\\fswiss\\fcharset0 Calibri;}}\r\n\
                    {\\colortbl ;\\red0\\green0\\blue0;}\r\n\
                    {\\*\\generator Riched20 16.0.6925}\\viewkind4\\uc1 \r\n\
                    \\pard\\cf1\\f0\\fs22 This is a note {\\v hidden}about the fox\\par\r\n\
                    \\trowd a\\cell b\\cell\\row\r\n}\r\n\0";
        assert_eq!(
            extract(rtf, Origin::Rtf),
            "This is a note about the fox\r\na\tb\t\r\n"
        );
    }

    #[test]
    fn plain_resets_to_default_font() {
        let rtf = b"{\\rtf1\\ansi\\ansicpg1252\\deff1{\\fonttbl{\\f0\\fcharset204 A;}{\\f1\\fcharset0 B;}}\
                    \\f0\\'cf\\plain\\'cf}";
        assert_eq!(text(rtf), "ПÏ");
    }

    #[test]
    fn ignores_htmlrtf_outside_html() {
        assert_eq!(text(b"{\\rtf1\\fromtext a\\htmlrtf b}"), "ab");
    }

    #[test]
    fn stops_at_the_end_of_the_document() {
        assert_eq!(text(b"{\\rtf1 a}b{c}"), "a");
        assert_eq!(text(b"{\\rtf1 a"), "a");
        assert_eq!(text(b"}{\\rtf1 a}"), "");
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let mut rtf = b"{\\rtf1 ".to_vec();
        rtf.extend(std::iter::repeat_n(b'{', 100_000));
        rtf.extend_from_slice(b"x");
        rtf.extend(std::iter::repeat_n(b'}', 100_000));
        rtf.extend_from_slice(b"y}z");
        assert_eq!(text(&rtf), "xy");
    }

    #[test]
    fn never_panics_on_arbitrary_rtf() {
        let pieces: [&[u8]; 16] = [
            b"{",
            b"}",
            b"\\",
            b"\\'",
            b"a",
            b"\\u-1",
            b"\\uc5",
            b"\\htmlrtf",
            b"\\*",
            b"\\htmltag",
            b"\\fonttbl",
            b"\\f1",
            b"\\fcharset136",
            b"\\'b7",
            b"\\bin5 ",
            b"\xff",
        ];
        let mut state = 0x9E37_79B9_u32;
        for _ in 0..5000 {
            let mut rtf = b"{\\rtf1\\fromhtml1 ".to_vec();
            for _ in 0..(state % 40) {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                rtf.extend_from_slice(pieces[(state % 16) as usize]);
            }
            for origin in [Origin::Html, Origin::Text, Origin::Rtf] {
                let _ = extract(&rtf, origin);
            }
        }
    }
}
