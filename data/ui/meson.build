# Every Blueprint file, without extension; each compiles to <name>.ui next to it.
blueprint_names = [
  'add_store',
  'artwork_dialog',
  'cover_tile',
  'fullscreen_bluetooth',
  'fullscreen_network',
  'fullscreen_power',
  'fullscreen_report',
  'fullscreen_sound_output',
  'fullscreen_system_update',
  'game_dialog',
  'game_overlay',
  'game_page',
  'home_page',
  'menu_drawer',
  'preferences',
  'quick_access',
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
