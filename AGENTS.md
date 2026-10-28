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

- Lint and test: `build-aux/sdk-run cargo clippy --workspace --all-targets -- -D warnings` and `build-aux/sdk-run cargo test --workspace`
- Build and install the Flatpak: `flatpak run --user org.flatpak.Builder --user --install --force-clean --state-dir=.flatpak-builder _build build-aux/io.github.larsw.MSGViewer.json`
- See it running without touching the desktop: `build-aux/headless-screenshot OUT.png [FILE.msg...]` runs the installed Flatpak in a throwaway headless GNOME Shell and saves a screenshot

Sample `.msg` files live in `outlook-msg/tests/samples/`, with their source and license recorded in `SOURCES.md` there.
