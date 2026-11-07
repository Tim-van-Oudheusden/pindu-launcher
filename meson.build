project(
  'pindu-launcher',
  'rust',
  version: '0.3.0',
  license: 'GPL-3.0-or-later',
  meson_version: '>= 1.6.0',
)

i18n = import('i18n')
gnome = import('gnome')

# Development builds get their own ID, so they install beside the stable app. Not
# `.Devel`: Flatpak and AppStream only allow a hyphen in the last segment.
base_id = 'io.github.tim_van_oudheusden.pindu-launcher'
application_id = get_option('profile') == 'development' ? base_id + '-devel' : base_id
gettext_package = meson.project_name()

prefix = get_option('prefix')
bindir = prefix / get_option('bindir')
datadir = prefix / get_option('datadir')
localedir = prefix / get_option('localedir')
pkgdatadir = datadir / meson.project_name()

dependency('gtk4', version: '>= 4.24')
dependency('libadwaita-1', version: '>= 1.10')

cargo = find_program('cargo')
blueprint_compiler = find_program('blueprint-compiler')

subdir('data')
subdir('crates')
subdir('po')
subdir('tests')

test('versions', find_program('build-aux/check-version.sh'), suite: 'packaging')
test('manifests', find_program('build-aux/check-manifests.sh'), suite: 'packaging')

# Release tarballs carry the crates, for builders without network (docs/releasing.md)
meson.add_dist_script('build-aux/dist-vendor.sh', cargo)

gnome.post_install(
  glib_compile_schemas: true,
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
