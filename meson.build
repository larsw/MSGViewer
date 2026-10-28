project(
  'msg-viewer',
  version: '0.1.0',
  license: 'Apache-2.0',
  meson_version: '>= 1.0.0',
)

gnome = import('gnome')

application_id = 'io.github.larsw.MSGViewer'
pkgdatadir = get_option('prefix') / get_option('datadir') / meson.project_name()

subdir('data')
subdir('msg-viewer')

gnome.post_install(
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
