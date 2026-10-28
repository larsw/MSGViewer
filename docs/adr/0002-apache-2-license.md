# Apache-2.0 license with an enforced dependency allowlist

The project is licensed Apache-2.0, not the GPL-3.0-or-later that most GNOME apps use. CI runs `cargo-deny` with an allowlist: MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0, and LGPL only for dynamically linked system GNOME libraries. Any GPL crate fails the build. Because of this, GPL-only `.msg` libraries cannot be used (ADR-0001).
