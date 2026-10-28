# Flathub is the only supported distribution; files only through portals

The supported build is a Flatpak on the latest stable GNOME runtime, published on Flathub. It has no filesystem permissions: files are opened and saved only through the document and file-chooser portals, and Attachments are opened through the OpenURI portal. Network access (`--share=network`) is kept only for opt-in Remote content (ADR-0004). Meson builds for distribution packagers work but are not a supported target, and the Rust minimum version tracks the runtime's Rust SDK extension.
