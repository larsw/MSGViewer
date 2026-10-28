cargo = find_program('cargo')

cargo_options = [
  '--manifest-path', meson.project_source_root() / 'Cargo.toml',
  '--target-dir', meson.project_build_root() / 'target',
  '--package', meson.project_name(),
]
cargo_env = ['MSG_VIEWER_PKGDATADIR=' + pkgdatadir]

if get_option('buildtype') == 'release'
  cargo_options += ['--release']
  rust_target = 'release'
else
  rust_target = 'debug'
endif

custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: meson.project_name(),
  console: true,
  install: true,
  install_dir: get_option('bindir'),
  depends: resources,
  command: [
    'env', cargo_env,
    cargo, 'build', cargo_options,
    '&&', 'cp', meson.project_build_root() / 'target' / rust_target / meson.project_name(), '@OUTPUT@',
  ],
)

test(
  'cargo-test',
  cargo,
  args: ['test', '--workspace', '--manifest-path', meson.project_source_root() / 'Cargo.toml',
         '--target-dir', meson.project_build_root() / 'target'],
  env: cargo_env,
  timeout: 600,
)
