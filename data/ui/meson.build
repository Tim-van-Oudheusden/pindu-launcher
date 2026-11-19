# Every Blueprint file, without extension; each compiles to <name>.ui next to it.
blueprint_names = [
  'add_store',
  'artwork_dialog',
  'cover_tile',
  'fullscreen_add_game',
  'fullscreen_bluetooth',
  'fullscreen_health',
  'fullscreen_network',
  'fullscreen_power',
  'fullscreen_proton_downloads',
  'fullscreen_report',
  'fullscreen_sound_output',
  'fullscreen_system_update',
  'game_dialog',
  'game_overlay',
  'game_page',
  'health_page',
  'home_page',
  'menu_drawer',
  'merge_dialog',
  'preferences',
  'quick_access',
  'save_snapshots_dialog',
  'secret_dialog',
  'shortcuts-dialog',
  'window',
]

blueprint_inputs = []
blueprint_outputs = []
foreach name : blueprint_names
  blueprint_inputs += name + '.blp'
  blueprint_outputs += name + '.ui'
endforeach

blueprints = custom_target(
  'blueprints',
  input: blueprint_inputs,
  output: blueprint_outputs,
  command: [
    blueprint_compiler,
    'batch-compile',
    '@OUTDIR@',
    '@CURRENT_SOURCE_DIR@',
    '@INPUT@',
  ],
)
