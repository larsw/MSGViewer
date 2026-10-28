# AGENTS.md

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues via the `gh` CLI; external PRs are not a triage surface. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Development

The app crate needs GTK, libadwaita and WebKitGTK from the GNOME SDK, so build and lint it inside the SDK (`flatpak install --user flathub org.gnome.Sdk//51 org.freedesktop.Sdk.Extension.rust-stable//26.08 org.flatpak.Builder`). The `outlook-msg` crate has no GTK dependency and builds with plain `cargo`.

- Lint and test: `build-aux/sdk-run cargo fmt --all --check`, `build-aux/sdk-run cargo clippy --workspace --all-targets -- -D warnings` and `build-aux/sdk-run cargo test --workspace`
- Dependency licenses, advisories and duplicates: `cargo deny check` on the host (`cargo install --locked cargo-deny`). `deny.toml` holds the ADR-0002 license allowlist; a crate under any other license, GPL included, fails it
- Desktop file and metainfo validation: `build-aux/sdk-run meson setup target/meson` once, then `build-aux/sdk-run meson test -C target/meson validate-desktop validate-metainfo`
- CI (`.github/workflows/ci.yml`) runs all of the above, plus a Flatpak build of the Flathub manifest and a check that `build-aux/cargo-sources.json` matches `Cargo.lock`, on every push and pull request
- Run the app: `build-aux/run [FILE.msg...]` rebuilds and reinstalls the Flatpak when sources changed since its last install (`--build` forces it, `--no-build` skips it), then opens the files through the document portal
- There are two Flatpak manifests with the same app ID; keep their `finish-args` the same. `build-aux/io.github.larsw.MSGViewer.dev.json` is a quick debug build that downloads crates while building, and is what `build-aux/run` uses. `build-aux/io.github.larsw.MSGViewer.json` is the Flathub manifest: a release build without network access, as Flathub builds, with every crate listed in `build-aux/cargo-sources.json`. Its copy on Flathub takes the sources from a release tag (a `git` source) instead of the checkout
- Build and install the Flatpak: `flatpak run --user org.flatpak.Builder --user --install --force-clean --state-dir=.flatpak-builder _build build-aux/io.github.larsw.MSGViewer.dev.json`, or the Flathub manifest in its place
- Whenever `Cargo.lock` changes, run `build-aux/update-cargo-sources` (needs [uv](https://docs.astral.sh/uv/)) and commit `build-aux/cargo-sources.json`, or the Flathub manifest can't build. It runs flatpak-builder-tools' `flatpak-cargo-generator.py`, pinned to a commit
- When several builds are in progress at once (for example in separate worktrees), add `--default-branch=<name>` to the Flatpak build so each installs side by side, and set `MSGV_BRANCH=<name>` for the screenshot script
- See it running without touching the desktop: `build-aux/headless-screenshot OUT.png [FILE.msg...]` runs the installed Flatpak in a throwaway headless GNOME Shell and saves a screenshot
- Store screenshots: `build-aux/make-screenshots` retakes `data/screenshots/*.png` from the made-up Messages in `outlook-msg/examples/demo_message.rs`, in English, with the installed Flatpak. Never use real or sample messages with real people in them

Translations use gettext with the `msg-viewer` domain; only English ships in v1, so `po/LINGUAS` is empty:

- Mark user-visible strings with `_("…")` in Blueprints and `gettext`/`ngettext` in Rust, or `gettext_noop` for strings kept in constants, translated with `gettext` where they are shown. Give placeholders names, as in `"{name} can’t be saved"`, and fill them in with `str::replace` after translating. Add a `// Translators:` comment wherever a string is ambiguous
- List every file with marked strings in `po/POTFILES.in`; a test fails if one is missing
- Build the template with `build-aux/sdk-run meson compile -C target/meson msg-viewer-pot`, which writes `po/msg-viewer.pot`

The parser is fuzzed on the host with `cargo-fuzz`, which needs a nightly toolchain (`rustup toolchain install nightly`, `cargo install cargo-fuzz`). The fuzz crate in `outlook-msg/fuzz/` is outside the workspace. From `outlook-msg/`:

- Raw bytes, seeded with the samples: `cargo +nightly fuzz run -O from_bytes fuzz/corpus/from_bytes tests/samples -- -dict=fuzz/msg.dict -max_len=65536 -max_total_time=600`
- Well-formed compound files built from arbitrary Message parts, which reach the property parsing: `cargo +nightly fuzz run -O from_parts fuzz/corpus/from_parts -- -max_total_time=600`
- Bodies made of arbitrary Compressed RTF, compressed with a correct CRC so they reach the decompressor and the RTF reader: `cargo +nightly fuzz run -O rtf_body fuzz/corpus/rtf_body -- -dict=fuzz/rtf.dict -max_len=16384 -max_total_time=600`
- Add a regression test to `outlook-msg/tests/damaged.rs` for every crash found

Sample `.msg` files live in `outlook-msg/tests/samples/`, with their source and license recorded in `SOURCES.md` there.

## Releasing

1. Set the new version in `meson.build` and `msg-viewer/Cargo.toml`, and add a `<release>` with notes at the top of `<releases>` in `data/io.github.larsw.MSGViewer.metainfo.xml.in`. `outlook-msg` has its own version.
2. If the UI changed, retake the screenshots with `build-aux/make-screenshots`. Point the screenshot URLs in the metainfo at the new tag.
3. Check with `build-aux/release-notes X.Y.Z`, which fails unless all three versions agree and prints the notes; then commit, tag `vX.Y.Z` and push the tag.
4. `.github/workflows/release.yml` then builds the Flathub manifest into `MSGViewer-vX.Y.Z.flatpak` and publishes a GitHub release with it, the notes, and the Flathub manifest and `cargo-sources.json` (from `build-aux/flathub-manifest`, building from the tag).
5. Copy those two files into the Flathub repository (`flathub/io.github.larsw.MSGViewer`; for the first release, a pull request to `flathub/flathub`'s `new-pr` branch).
