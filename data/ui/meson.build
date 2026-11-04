# Every Blueprint file, without extension; each compiles to <name>.ui next to it.
blueprint_names = [
  'cover_tile',
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
