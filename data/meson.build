desktop_file = i18n.merge_file(
  input: '@0@.desktop.in'.format(application_id),
  output: '@0@.desktop'.format(application_id),
  type: 'desktop',
  po_dir: '../po',
  install: true,
  install_dir: get_option('datadir') / 'applications',
)

desktop_file_validate = find_program('desktop-file-validate', required: false)
if desktop_file_validate.found()
  test('validate-desktop', desktop_file_validate, args: [desktop_file])
endif

metainfo_file = i18n.merge_file(
  input: '@0@.metainfo.xml.in'.format(application_id),
  output: '@0@.metainfo.xml'.format(application_id),
  po_dir: '../po',
  install: true,
  install_dir: get_option('datadir') / 'metainfo',
)

appstreamcli = find_program('appstreamcli', required: false)
if appstreamcli.found()
  test(
    'validate-metainfo',
    appstreamcli,
    args: ['validate', '--no-net', '--explain', metainfo_file],
  )
endif

# The desktop file is DBusActivatable, so the session bus starts the app
# through this when it is launched or asked to open files.
configure_file(
  input: '@0@.service.in'.format(application_id),
  output: '@0@.service'.format(application_id),
  configuration: {
    'application_id': application_id,
    'bindir': get_option('prefix') / get_option('bindir'),
  },
  install_dir: get_option('datadir') / 'dbus-1/services',
)

# The reader's preferences (msg-viewer/src/settings.rs). Compiled here too,
# so GSETTINGS_SCHEMA_DIR can point at the build directory.
gschema_file = '@0@.gschema.xml'.format(application_id)
install_data(
  gschema_file,
  install_dir: get_option('datadir') / 'glib-2.0/schemas',
)
gnome.compile_schemas(depend_files: files(gschema_file))

glib_compile_schemas = find_program('glib-compile-schemas', required: false)
if glib_compile_schemas.found()
  test(
    'validate-schema',
    glib_compile_schemas,
    args: ['--strict', '--dry-run', meson.current_source_dir()],
  )
endif

install_data(
  'icons/hicolor/scalable/apps/@0@.svg'.format(application_id),
  install_dir: get_option('datadir') / 'icons/hicolor/scalable/apps',
)
install_data(
  'icons/hicolor/symbolic/apps/@0@-symbolic.svg'.format(application_id),
  install_dir: get_option('datadir') / 'icons/hicolor/symbolic/apps',
)

subdir('resources')
