cargo_options = [
  '--manifest-path', meson.project_source_root() / 'Cargo.toml',
  '--target-dir', meson.project_build_root() / 'cargo-target',
]

# A release tarball (`meson dist`, build-aux/dist-vendor.sh) carries the crates in vendor/
# and a cargo config for them: build from those, without network and with Cargo.lock as is.
vendor_config = meson.project_source_root() / '.cargo' / 'config.toml'
if import('fs').is_dir(meson.project_source_root() / 'vendor')
  cargo_options += ['--offline', '--locked', '--config', vendor_config]
endif

buildtype = get_option('buildtype')
if buildtype == 'release'
  cargo_options += ['--release']
  rust_target = 'release'
elif buildtype == 'debugoptimized'
  cargo_options += ['--profile', 'debugoptimized']
  rust_target = 'debugoptimized'
else
  rust_target = 'debug'
endif

cargo_env = [
  'PINDU_APP_ID=' + application_id,
  'PINDU_LOCALEDIR=' + localedir,
  'PINDU_PKGDATADIR=' + pkgdatadir,
  'PINDU_PKGLIBEXECDIR=' + pkglibexecdir,
]

# The bundled external importers, each in its own folder with its manifest
# (docs/external-importers.md): Bottles' importer runs as one.
bottles_importer_dir = pkglibexecdir / 'importers' / 'bottles'

# One cargo build makes every program in the workspace: the app, and the bundled importers.
cargo_build = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: [meson.project_name(), 'pindu-import-bottles'],
  console: true,
  install: true,
  install_dir: [bindir, bottles_importer_dir],
  depends: resources,
  command: [
    'env', cargo_env, cargo, 'build', cargo_options,
    '&&',
    'cp', meson.project_build_root() / 'cargo-target' / rust_target / meson.project_name(), '@OUTPUT0@',
    '&&',
    'cp', meson.project_build_root() / 'cargo-target' / rust_target / 'pindu-import-bottles', '@OUTPUT1@',
  ],
)
app_binary = cargo_build[0]

configure_file(
  input: 'pindu-import-bottles' / 'importer.json.in',
  output: 'importer.json',
  configuration: {'VERSION': meson.project_version()},
  install: true,
  install_dir: bottles_importer_dir,
)

test(
  'cargo-test',
  find_program('env'),
  args: [cargo_env, cargo, 'test', '--workspace', cargo_options],
  timeout: 600,
  is_parallel: false,
  suite: 'cargo',
)
