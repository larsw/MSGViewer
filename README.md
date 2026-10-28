# MSG Viewer

A GNOME app for reading Microsoft Outlook `.msg` files on Linux. Double-click a `.msg` attachment in Files and read it like an email instead of an opaque binary file.

**Status:** early development. Nothing is usable yet.

## What v1 does

- Opens `.msg` files from Files, from the Open dialog, or by drag and drop. Each file gets its own window.
- Shows the subject, sender, date and recipients, then the message body: HTML, plain text, or HTML/text recovered from Outlook's RTF.
- Blocks remote content (images, trackers and so on) unless you allow it for the message you're reading.
- Lists attachments, which you can save, save all at once, or open in another app. Executables, scripts and macro-enabled files get a warning first. Messages attached to a message open in a new window.
- Shows as much as it can read from damaged files.

It doesn't edit, reply, send, or connect to mail accounts. See [docs/adr/0006-v1-scope.md](docs/adr/0006-v1-scope.md) for what is deliberately out of scope.

## Technology

- Rust, gtk4-rs and libadwaita, with UI defined in Blueprint and built with Meson driving Cargo.
- A GTK-free parser crate, `outlook-msg`, built on `cfb`.
- Bodies are rendered in WebKitGTK 6 with JavaScript disabled.
- Distributed as a Flatpak on Flathub, app ID `io.github.larsw.MSGViewer`.

Design decisions are recorded in [docs/adr/](docs/adr/), and the project's vocabulary in [GLOSSARY.md](GLOSSARY.md).

## License

[Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0)
