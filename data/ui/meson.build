# Every Blueprint file, without extension; each compiles to <name>.ui next to it.
blueprint_names = [
  'cover_tile',
  'fullscreen_bluetooth',
  'fullscreen_network',
  'fullscreen_system_update',
  'game_dialog',
  'game_page',
  'home_page',
  'preferences',
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
