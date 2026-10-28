project(
  'msg-viewer',
  version: '1.0.0',
  license: 'Apache-2.0',
  meson_version: '>= 1.0.0',
)

gnome = import('gnome')
i18n = import('i18n')

application_id = 'io.github.larsw.MSGViewer'
pkgdatadir = get_option('prefix') / get_option('datadir') / meson.project_name()
localedir = get_option('prefix') / get_option('localedir')

subdir('data')
subdir('msg-viewer')
subdir('po')

gnome.post_install(
  glib_compile_schemas: true,
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
