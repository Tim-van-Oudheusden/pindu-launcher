# Backups written by released versions

Each `<release>.json` here is a backup exported by that release's own `backup::export`, from a small library built
through that release's own pindu-core API. `tests/backup_fixtures.rs` imports each one into a freshly synced library
and checks that everything it holds comes back.

| File | Release | Backup format |
| --- | --- | --- |
| `0.5.1.json` | `v0.5.1`, the last release that wrote format 1 | 1 |
| `0.6.0.json` | `v0.6.0`, the first release that wrote format 2 | 2 |

The libraries hold launchers' games (as a sync adds them), favourites, games pinned to "Jump back in", a hidden game,
two games added by hand (a native one run on the host and a Windows one) with launch settings changed after adding,
collections with and without games, and play sessions. The format 2 library adds a group made by hand with a preferred
copy, an automatic group, a pair remembered as not the same game, three saved filters (over every game, a store and a
collection), metadata a provider fetched and metadata the user edited on a grouped game. The backup formats carry no
pins and no launch settings of launchers' games, so those are in the libraries but not in the files.

All names and paths are made up: `Fixture …` titles, invented store IDs and folders under `/var/fixture/`. The
generators fix the time at 2026-01-01 so the times in the files do not depend on the day they were made. The IDs of
games added by hand are random, so a run makes different ones; the test finds those games by title.

## Making a fixture

`generators/<release>.rs` is the program that made `<release>.json`. It is not built here: it compiles only against
the pindu-core of its release. To make one again, or to make the fixture for a new release:

```sh
git worktree add --detach ~/.cache/pindu-work/old-0.6.0 v0.6.0
mkdir -p ~/.cache/pindu-work/old-0.6.0/crates/pindu-core/examples
cp crates/pindu-core/tests/fixtures/backup/generators/0.6.0.rs \
  ~/.cache/pindu-work/old-0.6.0/crates/pindu-core/examples/backup_fixture.rs
cd ~/.cache/pindu-work/old-0.6.0
cargo run -q -p pindu-core --example backup_fixture -- ~/.cache/pindu-work/fixture-library \
  > <this checkout>/crates/pindu-core/tests/fixtures/backup/0.6.0.json
cd - && git worktree remove --force ~/.cache/pindu-work/old-0.6.0
rm -r ~/.cache/pindu-work/fixture-library
```

The folder given to the generator must not exist yet; it holds the library database while the program runs. Build it
where the release builds (the GNOME SDK with the rust-stable extension, as for the app).

When a pull request raises the backup format's `VERSION`, it adds the fixture of the last release that wrote the old
version (`docs/risk-tiers.md`): copy the newest generator, extend it with whatever user state that release's format
added, run it against that release's tag as above, check that the file names nothing real, and add a test to
`tests/backup_fixtures.rs` that imports it and checks every kind of state it holds.
