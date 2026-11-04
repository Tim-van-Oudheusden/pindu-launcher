//! Desktop entries: games without a launcher of their own, including Flatpak games.
//!
//! Every `.desktop` file with `Categories=Game` in the host's application folders becomes
//! a game, keeping its parsed Exec line for host spawn. Entries are read in XDG order, so
//! a user's entry overrides (or, with `Hidden`, deletes) a system entry of the same ID.
//! Launchers we import directly, their game shortcuts and our own entry are skipped, so
//! nothing is listed twice.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::location::HostDirs;
use super::{ImportedGame, Snapshot};
use crate::model::{ArtLayout, LaunchConfig, LaunchKind, Platform, Source};

/// Desktop IDs, Flatpak IDs and commands of the launchers we import directly, and ours.
const KNOWN_LAUNCHERS: &[&str] = &[
    "steam",
    "com.valvesoftware.Steam",
    "heroic",
    "heroic-run",
    "com.heroicgameslauncher.hgl",
    "lutris",
    "net.lutris.Lutris",
    "bottles",
    "bottles-cli",
    "com.usebottles.bottles",
    "faugus-launcher",
    "io.github.Faugus.faugus-launcher",
    "pindu-launcher",
    "io.github.tim_van_oudheusden.pindu-launcher",
];

/// URI schemes of launchers we import directly; an Exec line using one is a shortcut.
const LAUNCHER_SCHEMES: &[&str] = &["steam:", "heroic:", "lutris:", "bottles:"];

/// Icon sizes to look for, best first.
const ICON_SIZES: &[&str] = &[
    "512x512", "256x256", "128x128", "scalable", "96x96", "64x64", "48x48", "32x32",
];

/// The application folders, in XDG search order: the user's own, the user's and the
/// system's Flatpak exports, then the host's `/usr`.
pub fn application_dirs(dirs: &HostDirs) -> Vec<PathBuf> {
    share_dirs(dirs)
        .into_iter()
        .map(|share| share.join("applications"))
        .collect()
}

fn share_dirs(dirs: &HostDirs) -> Vec<PathBuf> {
    vec![
        dirs.data_home.clone(),
        dirs.data_home.join("flatpak/exports/share"),
        dirs.system_flatpak.join("exports/share"),
        dirs.host_usr.join("share"),
    ]
}

/// Imports the games among the host's desktop entries. Folders that are missing or
/// unreadable are skipped; the source is then just smaller.
pub fn import(dirs: &HostDirs) -> Snapshot {
    let mut snapshot = Snapshot::new(Source::Desktop);
    let mut seen = HashSet::new();
    for folder in application_dirs(dirs) {
        let mut files = Vec::new();
        collect_desktop_files(&folder, &folder, &mut files);
        files.sort();
        for (id, path) in files {
            // The first entry of an ID wins, even when it hides the others.
            if !seen.insert(id.clone()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            match game_from_entry(&id, &path, &parse_entry(&text)) {
                Ok(Some(mut game)) => {
                    game.artwork = icon(dirs, game.artwork.first().map(|(_, p)| p.as_path()));
                    snapshot.games.push(game);
                }
                Ok(None) => {}
                Err(message) => snapshot
                    .errors
                    .push(format!("{}: {message}", path.display())),
            }
        }
    }
    snapshot
}

/// Every `.desktop` file under `folder`, with its desktop ID (the path below the folder,
/// `/` replaced by `-`, without the extension).
fn collect_desktop_files(base: &Path, folder: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_desktop_files(base, &path, out);
        } else if path.extension().is_some_and(|ext| ext == "desktop")
            && let Ok(relative) = path.with_extension("").strip_prefix(base)
        {
            let id = relative.to_string_lossy().replace('/', "-");
            out.push((id, path));
        }
    }
}

/// The game a desktop entry describes, `None` when it is not one we list. The icon name,
/// if any, is passed back as the only artwork entry for [`import`] to resolve.
fn game_from_entry(
    id: &str,
    path: &Path,
    entry: &HashMap<String, String>,
) -> Result<Option<ImportedGame>, String> {
    let get = |key: &str| entry.get(key).map(String::as_str);
    let is_true = |key: &str| get(key) == Some("true");
    let is_game = get("Categories").is_some_and(|c| c.split(';').any(|c| c == "Game"));
    let flatpak_id = get("X-Flatpak");
    if get("Type") != Some("Application")
        || is_true("Hidden")
        || is_true("NoDisplay")
        || !is_game
        || KNOWN_LAUNCHERS.contains(&id)
        || flatpak_id.is_some_and(|app| KNOWN_LAUNCHERS.contains(&app))
    {
        return Ok(None);
    }
    let (Some(name), Some(exec)) = (get("Name").filter(|n| !n.is_empty()), get("Exec")) else {
        return Ok(None);
    };
    let argv = exec_argv(exec, name, get("Icon"), path)?;
    let is_shortcut = argv.iter().any(|word| {
        let command = word.rsplit('=').next().unwrap_or(word);
        let command = command.rsplit('/').next().unwrap_or(command);
        KNOWN_LAUNCHERS.contains(&command)
            || LAUNCHER_SCHEMES.iter().any(|scheme| word.contains(scheme))
    });
    if is_shortcut {
        return Ok(None);
    }
    let mut argv = argv.into_iter();
    let executable = PathBuf::from(argv.next().expect("exec_argv returns a command"));
    let mut game = ImportedGame::new(id, name, LaunchKind::DesktopEntry);
    game.installed = true;
    game.platform = flatpak_id.map(|_| Platform::Linux);
    game.launch_config = Some(LaunchConfig {
        executable,
        arguments: argv.collect(),
        working_dir: get("Path").filter(|p| !p.is_empty()).map(PathBuf::from),
        ..LaunchConfig::default()
    });
    if let Some(icon) = get("Icon").filter(|icon| !icon.is_empty()) {
        game.artwork = vec![(ArtLayout::Icon, PathBuf::from(icon))];
    }
    Ok(Some(game))
}

/// The icon file for an `Icon` value: an absolute path as it is, or a name looked up in
/// the hicolor theme of each data folder (best size first), then in the host's pixmaps.
fn icon(dirs: &HostDirs, icon: Option<&Path>) -> Vec<(ArtLayout, PathBuf)> {
    let Some(icon) = icon else { return Vec::new() };
    let found = if icon.is_absolute() {
        Some(icon.to_owned()).filter(|path| path.is_file())
    } else {
        let name = icon.to_string_lossy();
        let name = name.as_ref();
        let themed = share_dirs(dirs).into_iter().flat_map(|share| {
            ICON_SIZES.iter().flat_map(move |size| {
                ["png", "svg"]
                    .map(|ext| share.join(format!("icons/hicolor/{size}/apps/{name}.{ext}")))
            })
        });
        let pixmaps = ["png", "svg", "xpm"]
            .map(|ext| dirs.host_usr.join(format!("share/pixmaps/{name}.{ext}")));
        themed.chain(pixmaps).find(|path| path.is_file())
    };
    found
        .map(|path| vec![(ArtLayout::Icon, path)])
        .unwrap_or_default()
}

/// The keys of a desktop file's `[Desktop Entry]` group, with string escapes undone.
fn parse_entry(text: &str) -> HashMap<String, String> {
    let mut entry = HashMap::new();
    let mut in_group = false;
    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_group = line.trim_end() == "[Desktop Entry]";
            continue;
        }
        if in_group && let Some((key, value)) = line.split_once('=') {
            entry.insert(key.trim_end().to_owned(), unescape(value.trim_start()));
        }
    }
    entry
}

/// Undoes the escapes of the desktop entry `string` type: `\s \n \t \r \\`.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => {
                // `\\` is a backslash; any other escape is kept for the Exec quoting rules.
                if other != '\\' {
                    out.push('\\');
                }
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Splits an Exec value into an argument list by the desktop entry quoting rules, and
/// expands its field codes: file and URL codes are dropped (we pass none) along with
/// Flatpak's file-forwarding markers, `%i` becomes `--icon <icon>`, `%c` the name, `%k`
/// the desktop file and `%%` a percent sign.
fn exec_argv(
    exec: &str,
    name: &str,
    icon: Option<&str>,
    path: &Path,
) -> Result<Vec<String>, String> {
    let mut words: Vec<String> = Vec::new();
    let mut word: Option<String> = None;
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted => quoted = false,
            '"' => {
                quoted = true;
                word.get_or_insert_default();
            }
            '\\' if quoted => match chars.next() {
                Some(escaped @ ('"' | '`' | '$' | '\\')) => {
                    word.get_or_insert_default().push(escaped);
                }
                Some(other) => word.get_or_insert_default().extend(['\\', other]),
                None => return Err("the Exec line ends inside an escape".into()),
            },
            ' ' | '\t' if !quoted => {
                words.extend(word.take());
            }
            _ => word.get_or_insert_default().push(c),
        }
    }
    if quoted {
        return Err("the Exec line has an unclosed quote".into());
    }
    words.extend(word);

    let mut argv = Vec::new();
    for word in words {
        match word.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            // Flatpak's markers around the files to forward; we forward none.
            "--file-forwarding" | "@@" | "@@u" => {}
            "%i" => {
                if let Some(icon) = icon.filter(|icon| !icon.is_empty()) {
                    argv.extend(["--icon".to_owned(), icon.to_owned()]);
                }
            }
            _ => argv.push(expand_codes(&word, name, path)),
        }
    }
    if argv.is_empty() {
        return Err("the Exec line is empty".into());
    }
    Ok(argv)
}

/// Expands the field codes inside one argument.
fn expand_codes(word: &str, name: &str, path: &Path) -> String {
    let mut out = String::with_capacity(word.len());
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('c') => out.push_str(name),
            Some('k') => out.push_str(&path.to_string_lossy()),
            // Other codes expand to nothing here.
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_desktop_entry_group_is_read() {
        let entry = parse_entry(
            "# A comment\n\
             [Desktop Entry]\n\
             Type=Application\n\
             Name=Celeste\n\
             Name[de]=Celeste DE\n\
             Comment=Line one\\nLine two\\sand\\ttab\\\\\n\
             \n\
             [Desktop Action new-game]\n\
             Name=New Game\n\
             Exec=celeste --new\n",
        );

        assert_eq!(entry.get("Name").map(String::as_str), Some("Celeste"));
        assert_eq!(
            entry.get("Name[de]").map(String::as_str),
            Some("Celeste DE")
        );
        assert_eq!(
            entry.get("Comment").map(String::as_str),
            Some("Line one\nLine two and\ttab\\")
        );
        assert_eq!(entry.get("Exec"), None);
    }

    #[test]
    fn exec_splits_on_spaces_and_drops_field_codes() {
        let argv = exec_argv(
            "\"/opt/My Game/game\" --fullscreen %U",
            "My Game",
            None,
            Path::new("/apps/game.desktop"),
        );

        assert_eq!(
            argv,
            Ok(vec!["/opt/My Game/game".into(), "--fullscreen".into()])
        );
    }

    #[test]
    fn exec_keeps_environment_wrappers_and_drops_every_file_code() {
        let argv = exec_argv(
            "env FOO=1 game %f %F %u %d %D %n %N %v %m",
            "Game",
            None,
            Path::new("/a"),
        );

        assert_eq!(argv, Ok(vec!["env".into(), "FOO=1".into(), "game".into()]));
    }

    #[test]
    fn exec_unescapes_quoted_arguments() {
        // `sh -c "echo \"hi\" \$HOME \`id\` \\"` once the string escapes are undone.
        let argv = exec_argv(
            r#"sh -c "echo \"hi\" \$HOME \`id\` \\""#,
            "Game",
            None,
            Path::new("/a"),
        );

        assert_eq!(
            argv,
            Ok(vec![
                "sh".into(),
                "-c".into(),
                "echo \"hi\" $HOME `id` \\".into()
            ])
        );
    }

    #[test]
    fn exec_expands_icon_name_and_location_codes() {
        let argv = exec_argv(
            "game %i --title=%c %k 100%%",
            "My Game",
            Some("game-icon"),
            Path::new("/apps/game.desktop"),
        );

        assert_eq!(
            argv,
            Ok(vec![
                "game".into(),
                "--icon".into(),
                "game-icon".into(),
                "--title=My Game".into(),
                "/apps/game.desktop".into(),
                "100%".into(),
            ])
        );
        // Without an Icon key, %i expands to nothing.
        assert_eq!(
            exec_argv("game %i", "Game", None, Path::new("/a")),
            Ok(vec!["game".into()])
        );
    }

    #[test]
    fn flatpak_file_forwarding_markers_go_with_the_dropped_files() {
        let argv = exec_argv(
            "/usr/bin/flatpak run --branch=stable --command=retroarch --file-forwarding org.libretro.RetroArch @@ %f @@",
            "RetroArch",
            None,
            Path::new("/a"),
        );

        assert_eq!(
            argv,
            Ok(vec![
                "/usr/bin/flatpak".into(),
                "run".into(),
                "--branch=stable".into(),
                "--command=retroarch".into(),
                "org.libretro.RetroArch".into(),
            ])
        );
    }

    #[test]
    fn a_broken_exec_line_is_an_error() {
        assert!(exec_argv("game \"unclosed", "Game", None, Path::new("/a")).is_err());
        assert!(exec_argv("  ", "Game", None, Path::new("/a")).is_err());
    }
}
