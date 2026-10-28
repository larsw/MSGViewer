blueprints = custom_target(
  'blueprints',
  input: files(
    'ui/attachment_strip.blp',
    'ui/body_view.blp',
    'ui/find_bar.blp',
    'ui/header_block.blp',
    'ui/message_view.blp',
    'ui/shortcuts_dialog.blp',
    'ui/window.blp',
  ),
  output: '.',
  command: [
    find_program('blueprint-compiler'),
    'batch-compile',
    '@OUTPUT@',
    '@CURRENT_SOURCE_DIR@',
    '@INPUT@',
  ],
)

resources = gnome.compile_resources(
  'msg-viewer',
  'resources.gresource.xml',
  gresource_bundle: true,
  install: true,
  install_dir: pkgdatadir,
  dependencies: blueprints,
)
