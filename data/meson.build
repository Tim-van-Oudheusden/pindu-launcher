desktop_file = i18n.merge_file(
  input: application_id + '.desktop.in',
  output: application_id + '.desktop',
  type: 'desktop',
  po_dir: '../po',
  install: true,
  install_dir: datadir / 'applications',
)

metainfo_file = i18n.merge_file(
  input: application_id + '.metainfo.xml.in',
  output: application_id + '.metainfo.xml',
  po_dir: '../po',
  install: true,
  install_dir: datadir / 'metainfo',
)

service_conf = configuration_data()
service_conf.set('bindir', bindir)
configure_file(
  input: application_id + '.service.in',
  output: application_id + '.service',
  configuration: service_conf,
  install: true,
  install_dir: datadir / 'dbus-1' / 'services',
)

install_data(
  application_id + '.gschema.xml',
  install_dir: datadir / 'glib-2.0' / 'schemas',
)

install_subdir('icons', install_dir: datadir, strip_directory: false)

subdir('ui')

resources = gnome.compile_resources(
  'resources',
  'resources.gresource.xml',
  gresource_bundle: true,
  source_dir: [meson.current_build_dir(), meson.current_source_dir()],
  dependencies: blueprints,
  install: true,
  install_dir: pkgdatadir,
)

test(
  'validate-desktop',
  find_program('desktop-file-validate'),
  args: [desktop_file],
  suite: 'data',
)

test(
  'validate-metainfo',
  find_program('appstreamcli'),
  args: ['validate', '--no-net', '--explain', metainfo_file],
  suite: 'data',
)

test(
  'validate-schema',
  find_program('glib-compile-schemas'),
  args: ['--strict', '--dry-run', meson.current_source_dir()],
  suite: 'data',
)
