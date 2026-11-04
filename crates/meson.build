cargo_options = [
  '--manifest-path', meson.project_source_root() / 'Cargo.toml',
  '--target-dir', meson.project_build_root() / 'cargo-target',
]

if get_option('buildtype') == 'release'
  cargo_options += ['--release']
  rust_target = 'release'
else
  rust_target = 'debug'
endif

cargo_env = [
  'PINDU_LOCALEDIR=' + localedir,
  'PINDU_PKGDATADIR=' + pkgdatadir,
]

custom_target(
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
