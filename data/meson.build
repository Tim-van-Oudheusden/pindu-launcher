# The data files name the app by `@APP_ID@`, filled in per profile.
id_conf = configuration_data()
id_conf.set('APP_ID', application_id)
id_conf.set('SCHEMA_PATH', '/' + application_id.replace('.', '/') + '/')
id_conf.set('bindir', bindir)

desktop_file = i18n.merge_file(
  input: configure_file(
    input: base_id + '.desktop.in',
    output: application_id + '.desktop.in',
    configuration: id_conf,
  ),
  output: application_id + '.desktop',
  type: 'desktop',
  po_dir: '../po',
  install: true,
  install_dir: datadir / 'applications',
)

metainfo_file = i18n.merge_file(
  input: configure_file(
    input: base_id + '.metainfo.xml.in',
    output: application_id + '.metainfo.xml.in',
    configuration: id_conf,
  ),
  output: application_id + '.metainfo.xml',
  po_dir: '../po',
  install: true,
  install_dir: datadir / 'metainfo',
)

configure_file(
  input: base_id + '.service.in',
  output: application_id + '.service',
  configuration: id_conf,
  install: true,
  install_dir: datadir / 'dbus-1' / 'services',
)

configure_file(
  input: base_id + '.search-provider.ini.in',
  output: application_id + '.search-provider.ini',
  configuration: id_conf,
  install: true,
  install_dir: datadir / 'gnome-shell' / 'search-providers',
)

configure_file(
  input: base_id + '.gschema.xml.in',
  output: application_id + '.gschema.xml',
  configuration: id_conf,
  install: true,
  install_dir: datadir / 'glib-2.0' / 'schemas',
)

# The nightly icon (striped base) marks development builds.
app_icon = get_option('profile') == 'development' ? base_id + '.Devel.svg' : base_id + '.svg'
install_data(
  'icons' / app_icon,
  rename: application_id + '.svg',
  install_dir: datadir / 'icons' / 'hicolor' / 'scalable' / 'apps',
)
install_data(
  'icons' / base_id + '-symbolic.svg',
  rename: application_id + '-symbolic.svg',
  install_dir: datadir / 'icons' / 'hicolor' / 'symbolic' / 'apps',
)

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
  args: ['--strict', '--dry-run', meson.current_build_dir()],
  suite: 'data',
)
