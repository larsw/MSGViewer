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
- CI (`.github/workflows/ci.yml`) runs all of the above plus a Flatpak build on every push and pull request
- Build and install the Flatpak: `flatpak run --user org.flatpak.Builder --user --install --force-clean --state-dir=.flatpak-builder _build build-aux/io.github.larsw.MSGViewer.json`
- When several builds are in progress at once (for example in separate worktrees), add `--default-branch=<name>` to the Flatpak build so each installs side by side, and set `MSGV_BRANCH=<name>` for the screenshot script
- See it running without touching the desktop: `build-aux/headless-screenshot OUT.png [FILE.msg...]` runs the installed Flatpak in a throwaway headless GNOME Shell and saves a screenshot

The parser is fuzzed on the host with `cargo-fuzz`, which needs a nightly toolchain (`rustup toolchain install nightly`, `cargo install cargo-fuzz`). The fuzz crate in `outlook-msg/fuzz/` is outside the workspace. From `outlook-msg/`:

- Raw bytes, seeded with the samples: `cargo +nightly fuzz run -O from_bytes fuzz/corpus/from_bytes tests/samples -- -dict=fuzz/msg.dict -max_len=65536 -max_total_time=600`
- Well-formed compound files built from arbitrary Message parts, which reach the property parsing: `cargo +nightly fuzz run -O from_parts fuzz/corpus/from_parts -- -max_total_time=600`
- Add a regression test to `outlook-msg/tests/damaged.rs` for every crash found

Sample `.msg` files live in `outlook-msg/tests/samples/`, with their source and license recorded in `SOURCES.md` there.
