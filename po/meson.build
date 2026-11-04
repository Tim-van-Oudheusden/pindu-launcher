i18n.gettext(gettext_package, preset: 'glib')

test(
  'check-potfiles',
  find_program('check-potfiles.py'),
  args: [meson.project_source_root()],
  suite: 'po',
)
