desktop_file = files('@0@.desktop'.format(application_id))
install_data(desktop_file, install_dir: get_option('datadir') / 'applications')

desktop_file_validate = find_program('desktop-file-validate', required: false)
if desktop_file_validate.found()
  test('validate-desktop', desktop_file_validate, args: [desktop_file])
endif

metainfo_file = files('@0@.metainfo.xml'.format(application_id))
install_data(metainfo_file, install_dir: get_option('datadir') / 'metainfo')

appstreamcli = find_program('appstreamcli', required: false)
if appstreamcli.found()
  test(
    'validate-metainfo',
    appstreamcli,
    args: ['validate', '--no-net', '--explain', metainfo_file],
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
