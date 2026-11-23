# End-to-end: the installed app starts. Skipped (exit 77) without a display or before the
# install, so it runs in the Flatpak builds, where tests run after the install under xvfb.
test(
  'ui-smoke',
  find_program('e2e/ui-smoke.sh'),
  args: [app_binary, pkgdatadir / 'resources.gresource'],
  timeout: 60,
  suite: 'ui',
)

# The same, in the desktop window at its narrowest (360 px), which the library page must fit.
test(
  'ui-smoke-narrow',
  find_program('e2e/ui-smoke.sh'),
  args: ['--narrow', application_id, app_binary, pkgdatadir / 'resources.gresource'],
  timeout: 60,
  suite: 'ui',
)

# The repository's tooling in build-aux/, on the tree and on broken copies of it
python = find_program('python3')
foreach tool : ['check_version', 'check_manifests', 'release_notes']
  test(
    tool.replace('_', '-'),
    python,
    args: [files('tooling' / 'test_' + tool + '.py')],
    suite: 'tooling',
  )
endforeach
