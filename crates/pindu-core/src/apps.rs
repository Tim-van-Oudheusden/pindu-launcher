//! Apps (#830): the applications installed on the system that are not games, such as the
//! game launchers and the extras Get Games installs, each opened with one press. They are
//! read from the application folders the desktop source reads
//! ([`desktop::application_dirs`]), in the same XDG order, so a user's entry overrides or
//! hides a system one. They never enter the game library: no playtime, no artwork lookups,
//! no duplicates, and no place in its views, counts or search.

use std::collections::{BTreeMap, HashMap};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::launch::Command;
use crate::model::{GameId, Source};
use crate::sources::desktop;
use crate::sources::location::HostDirs;

/// The ID a `gamescope --steam` session follows the app with desktop ID `id` under once
/// it is opened, as it follows an opened store (`add_store::opened_id`): its desktop
/// entry, which the library never imports as a game.
pub fn opened_id(id: &str) -> GameId {
    GameId::new(Source::Desktop, id)
}

/// An installed application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Its desktop ID: the entry's path below its application folder, `/` replaced by
    /// `-`, without `.desktop`.
    pub id: String,
    /// Its name, in the user's language where the entry has one.
    pub name: String,
    /// Its icon file, if one is found.
    pub icon: Option<PathBuf>,
    /// The command that opens it, from its `Exec` line.
    pub command: Command,
}

/// What of the session decides which entries show: the desktops it runs
/// (`XDG_CURRENT_DESKTOP`), for `OnlyShowIn` and `NotShowIn`, and the language of its
/// messages, for the localised `Name`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    /// The desktops the session runs, in order, such as `GNOME`.
    pub desktops: Vec<String>,
    /// The locale of the session's messages, such as `de_DE.UTF-8`; `None` for `C`.
    pub locale: Option<String>,
}

impl Session {
    /// The session `vars` describe: `XDG_CURRENT_DESKTOP`, a list split on `:`, and the
    /// first of `LC_ALL`, `LC_MESSAGES` and `LANG` that is set, unless it is the `C` or
    /// `POSIX` locale, which names no language.
    pub fn from_vars(vars: impl Fn(&str) -> Option<String>) -> Self {
        let desktops = vars("XDG_CURRENT_DESKTOP")
            .map(|list| {
                list.split(':')
                    .filter(|desktop| !desktop.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|name| vars(name).filter(|value| !value.is_empty()))
            .filter(|locale| {
                let language = locale.split(['.', '@']).next().unwrap_or_default();
                !matches!(language, "C" | "POSIX")
            });
        Self { desktops, locale }
    }
}

/// The installed applications that `dirs` show to `session`, by name.
pub fn installed(dirs: &HostDirs, session: &Session) -> Vec<App> {
    let mut apps: Vec<App> = desktop::entries(dirs)
        .into_iter()
        .filter_map(|(id, path, entry)| app_from_entry(dirs, session, id, &path, &entry))
        .collect();
    apps.sort_by_cached_key(|app| (app.name.to_lowercase(), app.id.clone()));
    apps
}

/// The app the entry with desktop ID `id` at `path` describes, `None` when it is not one
/// to list.
fn app_from_entry(
    dirs: &HostDirs,
    session: &Session,
    id: String,
    path: &Path,
    entry: &HashMap<String, String>,
) -> Option<App> {
    let get = |key: &str| entry.get(key).map(String::as_str);
    let is_true = |key: &str| get(key) == Some("true");
    // A list of desktops names one of the session's.
    let names_ours = |key: &str| {
        get(key).map(|list| {
            list.split(';')
                .any(|desktop| session.desktops.iter().any(|ours| ours == desktop))
        })
    };
    if get("Type") != Some("Application")
        || is_true("Hidden")
        || is_true("NoDisplay")
        || is_true("Terminal")
        || names_ours("OnlyShowIn") == Some(false)
        || names_ours("NotShowIn") == Some(true)
        || get("TryExec").is_some_and(|program| !installed_program(dirs, program))
    {
        return None;
    }
    let name =
        localised(entry, "Name", session.locale.as_deref()).filter(|name| !name.is_empty())?;
    let argv = desktop::exec_argv(get("Exec")?, name, get("Icon"), path).ok()?;
    // What the library lists already: a desktop game, or a launcher's game shortcut. A
    // launcher's own entry opens it, and ours would open the launcher again.
    let flatpak_id = get("X-Flatpak");
    if desktop::is_own(&id, flatpak_id) {
        return None;
    }
    let game = desktop::game_from_entry(&id, path, entry).is_ok_and(|game| game.is_some());
    if !desktop::is_launcher(&id, flatpak_id) && (game || desktop::is_shortcut(&argv)) {
        return None;
    }
    Some(App {
        id,
        name: name.to_owned(),
        icon: get("Icon").and_then(|icon| desktop::icon_file(dirs, icon)),
        command: Command {
            argv,
            environment: BTreeMap::new(),
            working_dir: get("Path").filter(|dir| !dir.is_empty()).map(PathBuf::from),
        },
    })
}

/// The value of `key` in `entry` for `locale` (`lang_COUNTRY.ENCODING@MODIFIER`), matched as
/// the Desktop Entry Specification says: `lang_COUNTRY@MODIFIER`, `lang_COUNTRY`,
/// `lang@MODIFIER`, `lang`, then the key without a locale.
fn localised<'a>(
    entry: &'a HashMap<String, String>,
    key: &str,
    locale: Option<&str>,
) -> Option<&'a str> {
    let mut keys = Vec::new();
    if let Some(locale) = locale {
        let (rest, modifier) = match locale.split_once('@') {
            Some((rest, modifier)) => (rest, Some(modifier)),
            None => (locale, None),
        };
        let rest = rest.split_once('.').map_or(rest, |(rest, _)| rest);
        let (lang, country) = match rest.split_once('_') {
            Some((lang, country)) => (lang, Some(country)),
            None => (rest, None),
        };
        if let (Some(country), Some(modifier)) = (country, modifier) {
            keys.push(format!("{key}[{lang}_{country}@{modifier}]"));
        }
        if let Some(country) = country {
            keys.push(format!("{key}[{lang}_{country}]"));
        }
        if let Some(modifier) = modifier {
            keys.push(format!("{key}[{lang}@{modifier}]"));
        }
        keys.push(format!("{key}[{lang}]"));
    }
    keys.push(key.to_owned());
    keys.iter()
        .find_map(|key| entry.get(key).map(String::as_str))
}

/// The host's folders of programs, as `PATH` names them on a desktop, under `dirs`: the
/// user's, the host's own, and the Flatpak exports'.
fn program_dirs(dirs: &HostDirs) -> Vec<PathBuf> {
    vec![
        dirs.home.join(".local/bin"),
        dirs.host_usr.join("local/bin"),
        dirs.host_usr.join("bin"),
        dirs.host_usr.join("sbin"),
        dirs.host_usr.join("games"),
        dirs.data_home.join("flatpak/exports/bin"),
        dirs.system_flatpak.join("exports/bin"),
    ]
}

/// Whether `program`, a `TryExec` value, is installed: an executable file at its path,
/// a path under `/usr` looked up under the host's, or a name in the host's folders of
/// programs ([`program_dirs`]).
fn installed_program(dirs: &HostDirs, program: &str) -> bool {
    let executable = |path: &Path| {
        path.metadata()
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    };
    let path = Path::new(program);
    if path.is_absolute() {
        return match path.strip_prefix("/usr") {
            Ok(below) => executable(&dirs.host_usr.join(below)),
            Err(_) => executable(path),
        };
    }
    program_dirs(dirs)
        .iter()
        .any(|folder| executable(&folder.join(program)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::add_store::{self, StoreApp};

    #[test]
    fn an_opened_app_is_followed_as_its_desktop_entry_as_an_opened_store_is() {
        let heroic = opened_id("com.heroicgameslauncher.hgl");

        assert_eq!(heroic, add_store::opened_id(StoreApp::Heroic));
        assert_eq!(heroic.source(), Source::Desktop);
        assert_eq!(heroic.native_id(), "com.heroicgameslauncher.hgl");
        assert_ne!(opened_id("org.gnome.TextEditor"), heroic);
    }

    fn vars<'a>(pairs: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<String> + 'a {
        |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn the_session_s_desktops_and_language_come_from_its_environment() {
        let session = Session::from_vars(vars(&[
            ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"),
            ("LANG", "en_GB.UTF-8"),
            ("LC_MESSAGES", "de_DE.UTF-8"),
        ]));

        assert_eq!(session.desktops, ["ubuntu", "GNOME"]);
        assert_eq!(session.locale.as_deref(), Some("de_DE.UTF-8"));
        // LC_ALL wins over both; the C locale and an empty one name no language.
        let all = vars(&[("LC_ALL", "fr_FR.UTF-8"), ("LC_MESSAGES", "de_DE")]);
        assert_eq!(
            Session::from_vars(all).locale.as_deref(),
            Some("fr_FR.UTF-8")
        );
        for locale in ["C", "C.UTF-8", "POSIX", ""] {
            let session = Session::from_vars(vars(&[("LANG", locale)]));
            assert_eq!(session.locale, None, "{locale}");
        }
        assert_eq!(Session::from_vars(vars(&[])), Session::default());
    }
}
