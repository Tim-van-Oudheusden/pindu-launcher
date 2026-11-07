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

if get_option('buildtype') == 'release'
  cargo_options += ['--release']
  rust_target = 'release'
else
  rust_target = 'debug'
endif

cargo_env = [
  'PINDU_APP_ID=' + application_id,
  'PINDU_LOCALEDIR=' + localedir,
  'PINDU_PKGDATADIR=' + pkgdatadir,
]

app_binary = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: meson.project_name(),
  console: true,
  install: true,
  install_dir: bindir,
  depends: resources,
  command: [
    'env', cargo_env, cargo, 'build', cargo_options,
    '&&',
    'cp', meson.project_build_root() / 'cargo-target' / rust_target / meson.project_name(), '@OUTPUT@',
  ],
)

test(
  'cargo-test',
  find_program('env'),
  args: [cargo_env, cargo, 'test', '--workspace', cargo_options],
  timeout: 600,
  is_parallel: false,
  suite: 'cargo',
)
