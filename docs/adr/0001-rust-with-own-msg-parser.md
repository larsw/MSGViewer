# Rust, with our own `.msg` parser crate

The app is written in Rust with gtk4-rs and libadwaita, and `.msg` parsing lives in a separate workspace crate, `outlook-msg`, that has no GTK dependency. It is built on the `cfb` crate and follows [MS-OXMSG], [MS-OXRTFCP] and [MS-OXRTFEX]. We parse untrusted binary files, so a memory-safe language matters. The available `.msg` crates are thin or unmaintained, and the mature Python library `extract-msg` is GPL-3.0, which is incompatible with our Apache-2.0 license (ADR-0002).

## Considered Options

- **Python with PyGObject and `extract-msg`**: fastest to build, but has the license conflict.
- **GJS**: no parser exists, so we'd write one in JavaScript without memory-safety benefits.
- **Vala**: no parser exists, and its tooling is declining.

## Consequences

- The parser forbids `unsafe`, never panics on bad input, caps stream sizes and Embedded message depth, and is fuzzed with `cargo-fuzz`.
- The parser crate can be published to crates.io once its API settles.
