//! Get Games' catalogue (#885, `docs/get-games.md` section 6): Flathub's AppStream
//! catalogue as Flatpak downloads it into an installation, read once per catalogue version
//! into an index in the cache folder, filtered to games and a few extras by the lists file,
//! arranged into shelves and searched. "Store" already means storage in the core, hence
//! the name.

pub mod appstream;
pub mod index;
pub mod lists;

use std::collections::HashSet;
use std::path::PathBuf;

pub use appstream::{App, Block, Kind, Licence, Screenshot};
pub use lists::Lists;

use crate::add_store::{FLATHUB, Installation, StoreApp};
use crate::get_games;
use crate::search::{Query, normalize, sort_name};
use crate::sources::location::HostDirs;

/// The folder Flatpak keeps `installation` in: its apps, their data and its catalogues.
pub fn installation_dir(installation: Installation, dirs: &HostDirs) -> PathBuf {
    match installation {
        Installation::System => dirs.system_flatpak.clone(),
        Installation::User => dirs.data_home.join("flatpak"),
    }
}

/// The folder `installation` keeps Flathub's catalogue in, as Flatpak deploys it:
/// `appstream.xml` and the `icons` folder.
pub fn active_dir(installation: Installation, dirs: &HostDirs) -> PathBuf {
    installation_dir(installation, dirs)
        .join("appstream")
        .join(FLATHUB)
        .join(flatpak_arch())
        .join("active")
}

/// This machine's architecture as Flatpak names it.
fn flatpak_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "i386",
        arch => arch,
    }
}

/// The command that lists Flathub's apps in `installation` with their options, the
/// end-of-life ones among them, for [`end_of_life`].
pub fn end_of_life_command(installation: Installation) -> Vec<String> {
    [
        "flatpak",
        "remote-ls",
        get_games::flag(installation),
        FLATHUB,
        "--app",
        "--all",
        "--columns=application,options",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// The apps [`end_of_life_command`] says are end-of-life: an `eol=` or `eol-rebase=` among
/// their options.
pub fn end_of_life(remote_ls: &str) -> HashSet<String> {
    remote_ls
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(_, options)| {
            options
                .split(',')
                .any(|option| option.starts_with("eol=") || option.starts_with("eol-rebase="))
        })
        .map(|(id, _)| id.trim().to_owned())
        .collect()
}

/// A category of games that gets a shelf of its own: freedesktop's additional categories
/// for games, and emulators, which Flathub files under games.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameCategory {
    /// `ActionGame`.
    Action,
    /// `AdventureGame`.
    Adventure,
    /// `ArcadeGame`.
    Arcade,
    /// `BoardGame`.
    Board,
    /// `BlocksGame`.
    Blocks,
    /// `CardGame`.
    Card,
    /// `KidsGame`.
    Kids,
    /// `LogicGame`.
    Logic,
    /// `RolePlaying`.
    RolePlaying,
    /// `Shooter`.
    Shooter,
    /// `Simulation`.
    Simulation,
    /// `SportsGame`.
    Sports,
    /// `StrategyGame`.
    Strategy,
    /// `Emulator`.
    Emulator,
}

impl GameCategory {
    /// Every category, in the order their shelves come.
    pub const ALL: [Self; 14] = [
        Self::Action,
        Self::Adventure,
        Self::Arcade,
        Self::Board,
        Self::Blocks,
        Self::Card,
        Self::Kids,
        Self::Logic,
        Self::RolePlaying,
        Self::Shooter,
        Self::Simulation,
        Self::Sports,
        Self::Strategy,
        Self::Emulator,
    ];

    /// The category as AppStream names it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Action => "ActionGame",
            Self::Adventure => "AdventureGame",
            Self::Arcade => "ArcadeGame",
            Self::Board => "BoardGame",
            Self::Blocks => "BlocksGame",
            Self::Card => "CardGame",
            Self::Kids => "KidsGame",
            Self::Logic => "LogicGame",
            Self::RolePlaying => "RolePlaying",
            Self::Shooter => "Shooter",
            Self::Simulation => "Simulation",
            Self::Sports => "SportsGame",
            Self::Strategy => "StrategyGame",
            Self::Emulator => "Emulator",
        }
    }
}

/// A row of the catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    /// The lists file's picks, in its order.
    Picks,
    /// Steam, Heroic and Lutris, which install through Add a Store's flow.
    Launchers,
    /// The games of a category.
    Category(GameCategory),
    /// The games in no category of their own.
    More,
    /// The extras that are not games.
    Extras,
}

/// What Get Games offers: the catalogue's applications that count as games or are
/// extras, less the blocked and the end-of-life ones, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Offer {
    apps: Vec<App>,
    lists: Lists,
}

impl Offer {
    /// What `apps`, a catalogue's entries, offer under `lists`, leaving out the apps
    /// `end_of_life` names.
    pub fn new(apps: Vec<App>, lists: Lists, end_of_life: &HashSet<String>) -> Self {
        let mut apps: Vec<App> = apps
            .into_iter()
            .filter(|app| {
                offerable(app, &lists)
                    && !end_of_life.contains(&app.id)
                    && !lists.blocklist.contains(&app.id)
            })
            .collect();
        apps.sort_by_cached_key(|app| (sort_name(&app.name), app.id.clone()));
        Self { apps, lists }
    }

    /// Every app offered, by name.
    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    /// The offered app `id`; `None` for one the catalogue does not offer.
    pub fn get(&self, id: &str) -> Option<&App> {
        self.apps.iter().find(|app| app.id == id)
    }

    /// Whether the offered app `id` is an extra rather than a game: it opens, it does not
    /// play, and the library never shows it.
    pub fn is_extra(&self, id: &str) -> bool {
        self.get(id).is_some_and(|app| !is_game(app, &self.lists))
    }

    /// The shelves that have apps, in order, each with its apps.
    pub fn shelves(&self) -> Vec<(Shelf, Vec<&App>)> {
        let launcher = |app: &App| StoreApp::ALL.iter().any(|store| store.app_id() == app.id);
        let games = || {
            self.apps
                .iter()
                .filter(|app| is_game(app, &self.lists) && !launcher(app))
        };
        let in_category = |app: &App, category: GameCategory| {
            app.categories.iter().any(|name| name == category.name())
        };
        let mut shelves: Vec<(Shelf, Vec<&App>)> = vec![
            (
                Shelf::Picks,
                self.lists
                    .picks
                    .iter()
                    .filter_map(|id| self.get(id))
                    .collect(),
            ),
            (
                Shelf::Launchers,
                StoreApp::ALL
                    .iter()
                    .filter_map(|store| self.get(store.app_id()))
                    .collect(),
            ),
        ];
        shelves.extend(GameCategory::ALL.into_iter().map(|category| {
            let apps = games().filter(|app| in_category(app, category)).collect();
            (Shelf::Category(category), apps)
        }));
        shelves.push((
            Shelf::More,
            games()
                .filter(|app| {
                    !GameCategory::ALL
                        .into_iter()
                        .any(|category| in_category(app, category))
                })
                .collect(),
        ));
        shelves.push((
            Shelf::Extras,
            self.apps
                .iter()
                .filter(|app| !is_game(app, &self.lists))
                .collect(),
        ));
        shelves.retain(|(_, apps)| !apps.is_empty());
        shelves
    }

    /// The offered apps that `text`, as typed, finds by name, summary or keyword: those it
    /// finds by name first, each group by name. Every app for no text.
    pub fn search(&self, text: &str) -> Vec<&App> {
        let query = Query::new(text);
        let (mut by_name, mut by_text) = (Vec::new(), Vec::new());
        for app in &self.apps {
            if query.matches(&normalize(&app.name)) {
                by_name.push(app);
            } else if query.matches(&normalize(&app.summary))
                || app
                    .keywords
                    .iter()
                    .any(|keyword| query.matches(&normalize(keyword)))
            {
                by_text.push(app);
            }
        }
        by_name.append(&mut by_text);
        by_name
    }
}

/// Whether `lists` lets Get Games offer `app`: an application that is a game or an extra.
/// The index ([`index::load`]) keeps these, so [`Offer::new`] finds every one it may offer.
fn offerable(app: &App, lists: &Lists) -> bool {
    app.kind == Kind::Application && (is_game(app, lists) || lists.extras.contains(&app.id))
}

/// Whether `app` counts as a game under `lists`: it has one of its categories.
fn is_game(app: &App, lists: &Lists) -> bool {
    app.categories
        .iter()
        .any(|category| lists.categories.contains(category))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn catalogue() -> Vec<App> {
        let xml = std::fs::read_to_string(
            fixture_dir().join("catalogue/appstream/flathub/x86_64/active/appstream.xml"),
        )
        .unwrap();
        appstream::parse(&xml, &[]).unwrap()
    }

    fn default_lists() -> Lists {
        lists::parse(lists::DEFAULT).unwrap()
    }

    fn offer() -> Offer {
        Offer::new(catalogue(), default_lists(), &HashSet::new())
    }

    fn ids<'a>(apps: impl IntoIterator<Item = &'a App>) -> Vec<&'a str> {
        apps.into_iter().map(|app| app.id.as_str()).collect()
    }

    fn dirs() -> HostDirs {
        HostDirs {
            home: "/home/pindu".into(),
            data_home: "/home/pindu/.local/share".into(),
            config_home: "/home/pindu/.config".into(),
            host_usr: "/usr".into(),
            system_flatpak: "/var/lib/flatpak".into(),
        }
    }

    #[test]
    fn the_catalogue_is_in_the_installations_appstream_folder_for_this_architecture() {
        let arch = match std::env::consts::ARCH {
            "x86" => "i386",
            other => other,
        };
        assert_eq!(
            active_dir(Installation::System, &dirs()),
            PathBuf::from(format!("/var/lib/flatpak/appstream/flathub/{arch}/active"))
        );
        assert_eq!(
            active_dir(Installation::User, &dirs()),
            PathBuf::from(format!(
                "/home/pindu/.local/share/flatpak/appstream/flathub/{arch}/active"
            ))
        );
    }

    #[test]
    fn games_and_extras_are_offered_by_name() {
        let offer = offer();
        let offered = ids(offer.apps());
        for id in [
            "net.supertuxkart.SuperTuxKart",
            "org.gnome.Chess",
            "com.valvesoftware.Steam",
            "com.moonlight_stream.Moonlight",
            "org.phoenicis.playonlinux",
            "tv.kodi.Kodi",
            "org.mozilla.firefox",
        ] {
            assert!(offered.contains(&id), "{id} is not offered: {offered:?}");
        }
        let names: Vec<String> = offer
            .apps()
            .iter()
            .map(|app| sort_name(&app.name))
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn blocked_apps_and_apps_neither_games_nor_extras_are_not_offered() {
        let offer = offer();
        for id in [
            "com.steamgriddb.SGDBoop",
            "io.github.benjamimgois.goverlay",
            "net.davidotek.pupgui2",
            "org.gnome.Calculator",
        ] {
            assert_eq!(offer.get(id), None, "{id} is offered");
        }
    }

    #[test]
    fn only_applications_are_offered_whatever_their_categories() {
        let offer = offer();
        for id in [
            "page.codeberg.Qwel.egui_picross",
            "com.valvesoftware.Steam.Utility.InhibitScreensaver",
            "io.github.theforceengine.tfe.Debug",
            "org.freedesktop.Platform",
        ] {
            assert_eq!(offer.get(id), None, "{id} is offered");
        }
    }

    #[test]
    fn end_of_life_apps_are_not_offered() {
        let end_of_life = HashSet::from(["org.gnome.Chess".to_owned()]);
        let offer = Offer::new(catalogue(), default_lists(), &end_of_life);
        assert_eq!(offer.get("org.gnome.Chess"), None);
        assert!(offer.get("org.kde.kiriki").is_some());
    }

    #[test]
    fn the_end_of_life_apps_are_those_whose_options_say_so() {
        let output =
            std::fs::read_to_string(fixture_dir().join("flatpak/remote-ls-eol.stdout")).unwrap();
        let end_of_life = end_of_life(&output);
        assert_eq!(end_of_life.len(), 3);
        assert!(end_of_life.contains("app.drey.Doggo"));
        assert!(end_of_life.contains("app.devsuite.Manuals"));
        assert!(!end_of_life.contains("net.supertuxkart.SuperTuxKart"));
    }

    #[test]
    fn the_end_of_life_command_lists_every_app_of_flathub_in_the_installation() {
        assert_eq!(
            end_of_life_command(Installation::System),
            [
                "flatpak",
                "remote-ls",
                "--system",
                "flathub",
                "--app",
                "--all",
                "--columns=application,options"
            ]
        );
        assert_eq!(end_of_life_command(Installation::User)[2], "--user");
    }

    #[test]
    fn the_shelves_are_picks_launchers_categories_more_games_then_extras() {
        let offer = offer();
        let shelves: Vec<Shelf> = offer
            .shelves()
            .into_iter()
            .map(|(shelf, _)| shelf)
            .collect();
        assert_eq!(
            shelves,
            [
                Shelf::Picks,
                Shelf::Launchers,
                Shelf::Category(GameCategory::Action),
                Shelf::Category(GameCategory::Arcade),
                Shelf::Category(GameCategory::Board),
                Shelf::Category(GameCategory::Card),
                Shelf::Category(GameCategory::Kids),
                Shelf::Category(GameCategory::Shooter),
                Shelf::Category(GameCategory::Simulation),
                Shelf::Category(GameCategory::Strategy),
                Shelf::Category(GameCategory::Emulator),
                Shelf::More,
                Shelf::Extras,
            ]
        );
    }

    fn shelf(offer: &Offer, wanted: Shelf) -> Vec<&str> {
        offer
            .shelves()
            .into_iter()
            .find(|(shelf, _)| *shelf == wanted)
            .map(|(_, apps)| ids(apps))
            .unwrap_or_default()
    }

    #[test]
    fn picks_keep_the_lists_order_and_skip_ids_the_catalogue_lacks() {
        let lists = Lists {
            picks: vec![
                "org.taisei_project.Taisei".to_owned(),
                "com.example.Gone".to_owned(),
                "org.srb2.SRB2".to_owned(),
                "com.steamgriddb.SGDBoop".to_owned(),
            ],
            ..default_lists()
        };
        let offer = Offer::new(catalogue(), lists, &HashSet::new());
        assert_eq!(
            shelf(&offer, Shelf::Picks),
            ["org.taisei_project.Taisei", "org.srb2.SRB2"]
        );
    }

    #[test]
    fn the_stores_are_launchers_only_and_other_shelves_are_by_name() {
        let offer = offer();
        assert_eq!(
            shelf(&offer, Shelf::Launchers),
            [
                "com.valvesoftware.Steam",
                "com.heroicgameslauncher.hgl",
                "net.lutris.Lutris"
            ]
        );
        assert_eq!(
            shelf(&offer, Shelf::Category(GameCategory::Arcade)),
            [
                "org.supertuxproject.SuperTux",
                "net.supertuxkart.SuperTuxKart",
                "org.taisei_project.Taisei",
                "com.tuxpusher.TuxPusherAF"
            ]
        );
        let more = shelf(&offer, Shelf::More);
        assert!(more.contains(&"org.srb2.SRB2"));
        assert!(more.contains(&"com.moonlight_stream.Moonlight"));
        assert!(!more.contains(&"com.valvesoftware.Steam"));
        assert!(!more.contains(&"tv.kodi.Kodi"));
        assert_eq!(
            shelf(&offer, Shelf::Extras),
            ["org.mozilla.firefox", "tv.kodi.Kodi"]
        );
    }

    #[test]
    fn an_extra_is_an_offered_app_outside_the_game_categories() {
        let offer = offer();
        assert!(offer.is_extra("tv.kodi.Kodi"));
        assert!(!offer.is_extra("net.supertuxkart.SuperTuxKart"));
        assert!(!offer.is_extra("org.gnome.Calculator"));
    }

    #[test]
    fn search_finds_by_name_first_then_by_summary_or_keyword() {
        let offer = offer();
        assert_eq!(
            ids(offer.search("tux")),
            [
                "org.supertuxproject.SuperTux",
                "net.supertuxkart.SuperTuxKart",
                "com.tuxpusher.TuxPusherAF"
            ]
        );
        assert_eq!(
            ids(offer.search("Penguin")),
            ["org.supertuxproject.SuperTux", "com.tuxpusher.TuxPusherAF"]
        );
        assert_eq!(ids(offer.search("rts")), ["net.wz2100.wz2100"]);
        assert_eq!(ids(offer.search("solit")), ["org.gnome.Aisleriot"]);
        assert_eq!(ids(offer.search("browser")), ["org.mozilla.firefox"]);
    }

    #[test]
    fn search_never_finds_what_is_not_offered_and_no_text_finds_everything() {
        let offer = offer();
        assert!(offer.search("steamgriddb").is_empty());
        assert!(offer.search("calculator").is_empty());
        assert_eq!(offer.search("").len(), offer.apps().len());
    }
}
