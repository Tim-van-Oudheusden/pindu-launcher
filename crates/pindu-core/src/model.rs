//! Library entities shared by the sources, the launch services and both shells.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// Seconds since the Unix epoch.
pub type Timestamp = i64;

/// Error for a stored enum value this version does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownValue {
    /// What the value was meant to be, for example "source" or "game ID".
    pub kind: &'static str,
    /// The value as stored.
    pub value: String,
}

impl fmt::Display for UnknownValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown {} {:?}", self.kind, self.value)
    }
}

impl std::error::Error for UnknownValue {}

/// Declares a fieldless enum stored as a fixed lowercase string.
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident, $kind:literal { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The fixed lowercase string the value is stored as.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl FromStr for $name {
            type Err = UnknownValue;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(UnknownValue { kind: $kind, value: value.to_owned() }),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

string_enum! {
    /// Where a game comes from. The string form is the prefix of every game ID.
    Source, "source" {
        /// Steam.
        Steam => "steam",
        /// An Epic Games Store game in Heroic.
        HeroicEpic => "heroic-epic",
        /// A GOG game in Heroic.
        HeroicGog => "heroic-gog",
        /// An Amazon Games game in Heroic.
        HeroicAmazon => "heroic-amazon",
        /// A game added to Heroic by hand.
        HeroicSideload => "heroic-sideload",
        /// Lutris.
        Lutris => "lutris",
        /// Bottles.
        Bottles => "bottles",
        /// Faugus Launcher.
        Faugus => "faugus",
        /// A desktop entry.
        Desktop => "desktop",
        /// A game the user added in Pindu.
        Manual => "manual",
    }
}

string_enum! {
    /// The operating system a game build targets.
    Platform, "platform" {
        /// Windows.
        Windows => "windows",
        /// Linux.
        Linux => "linux",
    }
}

string_enum! {
    /// How Play starts a game, which also decides the permission tier it needs.
    LaunchKind, "launch kind" {
        /// Handed to the owning launcher through a URI and the OpenURI portal.
        StoreUri => "store-uri",
        /// A command run on the host through host spawn.
        HostCommand => "host-command",
        /// A desktop entry's Exec line (or `flatpak run`) run through host spawn.
        DesktopEntry => "desktop-entry",
        /// A Windows game run through umu and Proton inside the sandbox.
        Proton => "proton",
        /// A native game run in the Steam Linux Runtime through umu.
        RuntimeNative => "runtime-native",
    }
}

string_enum! {
    /// The artwork layouts: SteamGridDB's four, and its square grids.
    ArtLayout, "artwork layout" {
        /// Portrait cover, 2:3 (600×900).
        Cover => "cover",
        /// Wide hero banner (about 1920×620).
        Hero => "hero",
        /// The game's logo.
        Logo => "logo",
        /// The game's icon.
        Icon => "icon",
        /// Square grid (512×512 or 1024×1024), for fullscreen Home's square tiles.
        Square => "square",
    }
}

string_enum! {
    /// Where a piece of artwork came from.
    ArtOrigin, "artwork origin" {
        /// The game's source.
        Source => "source",
        /// SteamGridDB.
        SteamGridDb => "steamgriddb",
        /// Picked by the user.
        User => "user",
        /// A generated placeholder.
        Generated => "generated",
    }
}

/// A stable game ID of the form `<source>:<native id>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GameId(String);

impl GameId {
    /// The ID of the game a source knows as `native_id`.
    pub fn new(source: Source, native_id: &str) -> Self {
        Self(format!("{}:{native_id}", source.as_str()))
    }

    /// The ID as text, `<source>:<native id>`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The source the ID belongs to.
    pub fn source(&self) -> Source {
        self.split().0
    }

    /// The part after the source: the ID the source itself uses.
    pub fn native_id(&self) -> &str {
        self.split().1
    }

    fn split(&self) -> (Source, &str) {
        let (source, native_id) = self.0.split_once(':').expect("validated on construction");
        (
            source.parse().expect("validated on construction"),
            native_id,
        )
    }
}

impl FromStr for GameId {
    type Err = UnknownValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || UnknownValue {
            kind: "game ID",
            value: value.to_owned(),
        };
        let (source, native_id) = value.split_once(':').ok_or_else(invalid)?;
        if native_id.is_empty() {
            return Err(invalid());
        }
        let source: Source = source.parse().map_err(|_| invalid())?;
        Ok(Self::new(source, native_id))
    }
}

impl fmt::Display for GameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One game in the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    /// The game's stable ID.
    pub id: GameId,
    /// The game's title.
    pub title: String,
    /// Title used for sorting, for example without a leading article.
    pub sort_title: String,
    /// The developer, when the source gives one.
    pub developer: Option<String>,
    /// The operating system the build targets, when known.
    pub platform: Option<Platform>,
    /// Whether the game is installed.
    pub installed: bool,
    /// Where the game is installed, when known.
    pub install_path: Option<PathBuf>,
    /// Size on disk in bytes, when known.
    pub size_bytes: Option<i64>,
    /// When the game was added to the library.
    pub added_at: Timestamp,
    /// When the game was last played, if ever.
    pub last_played_at: Option<Timestamp>,
    /// Whether the user marked the game as a favourite.
    pub favourite: bool,
    /// Whether the user hid the game.
    pub hidden: bool,
    /// Position in "Jump back in", 1 to 3, for pinned games.
    pub pin_order: Option<u8>,
    /// Removed by the user; stays removed while the source still lists it.
    pub user_removed: bool,
    /// Set when the source stopped listing the game; cleared when it returns.
    pub missing_since: Option<Timestamp>,
    /// How Play starts the game.
    pub launch_kind: LaunchKind,
    /// The URI its source gives for starting the game, for sources whose URI cannot be
    /// built from the game ID (Bottles needs the bottle and program names).
    pub launch_uri: Option<String>,
    /// The genres the source gives.
    pub genres: Vec<String>,
    /// The source's description of the game.
    pub description: Option<String>,
    /// Start a Steam game through Steam's launch-options dialog
    /// (`steam://launch/<id>/dialog`), which honours the user's launch-option choice.
    pub steam_launch_dialog: bool,
}

impl Game {
    /// A game with the given identity and every optional field empty.
    pub fn new(
        id: GameId,
        title: impl Into<String>,
        launch_kind: LaunchKind,
        added_at: Timestamp,
    ) -> Self {
        let title = title.into();
        Self {
            id,
            sort_title: title.clone(),
            title,
            developer: None,
            platform: None,
            installed: false,
            install_path: None,
            size_bytes: None,
            added_at,
            last_played_at: None,
            favourite: false,
            hidden: false,
            pin_order: None,
            user_removed: false,
            missing_since: None,
            launch_kind,
            launch_uri: None,
            genres: Vec::new(),
            description: None,
            steam_launch_dialog: false,
        }
    }

    /// The source the game comes from.
    pub fn source(&self) -> Source {
        self.id.source()
    }

    /// Whether the game belongs in the library views: still listed by its source and not
    /// removed by the user. Hidden games count; the views filter them separately.
    pub fn in_library(&self) -> bool {
        self.missing_since.is_none() && !self.user_removed
    }
}

/// How we start a game that has no launcher of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchConfig {
    /// The program to run.
    pub executable: PathBuf,
    /// The arguments passed to the program.
    pub arguments: Vec<String>,
    /// The folder to run in; `None` for the default.
    pub working_dir: Option<PathBuf>,
    /// Environment variables set for the game, on top of ours.
    pub environment: BTreeMap<String, String>,
    /// The Proton build, by its internal name; `None` for the default one.
    pub proton_version: Option<String>,
    /// The Wine prefix; `None` for the game's own folder in the default prefixes folder.
    pub prefix_path: Option<PathBuf>,
    /// Whether the MangoHud Performance Overlay is on.
    pub mangohud: bool,
    /// Whether GameMode is on.
    pub gamemode: bool,
    /// Extra commands wrapped around the game, outermost first.
    pub wrappers: Vec<String>,
}

/// A tool fullscreen mode switches on or off for a game we run, from its next launch: its
/// Performance Overlay (MangoHud) or GameMode, as [`LaunchConfig`] keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceTool {
    /// The Performance Overlay (MangoHud).
    Overlay,
    /// GameMode.
    GameMode,
}

impl PerformanceTool {
    /// Whether `config` has the tool on.
    pub fn is_on(self, config: &LaunchConfig) -> bool {
        match self {
            Self::Overlay => config.mangohud,
            Self::GameMode => config.gamemode,
        }
    }
}

/// One play session of a game we started ourselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaySession {
    /// The session's row ID.
    pub id: i64,
    /// The game played.
    pub game_id: GameId,
    /// When the session started.
    pub started_at: Timestamp,
    /// When the session ended.
    pub ended_at: Timestamp,
    /// How long the session lasted, in seconds.
    pub duration_secs: i64,
}

/// Playtime a source tracks itself (Steam, Heroic, Lutris, Faugus).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPlaytime {
    /// The game the playtime belongs to.
    pub game_id: GameId,
    /// The source that tracked it.
    pub source: Source,
    /// Total playtime in seconds.
    pub total_secs: i64,
    /// When the source last saw the game played, if ever.
    pub last_played_at: Option<Timestamp>,
}

/// One stored image of a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artwork {
    /// The game the image belongs to.
    pub game_id: GameId,
    /// The layout the image is for.
    pub layout: ArtLayout,
    /// Where the image came from.
    pub origin: ArtOrigin,
    /// Where the image file is stored.
    pub path: PathBuf,
}

/// A user collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    /// The collection's row ID.
    pub id: i64,
    /// The collection's name.
    pub name: String,
    /// Its place in the user's order of collections, lowest first.
    pub position: i64,
}

/// What we know about one source on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceState {
    /// The source.
    pub source: Source,
    /// Whether the user has the source turned on.
    pub enabled: bool,
    /// The folder read on the last resolve; `None` when none was found.
    pub resolved_location: Option<PathBuf>,
    /// The folder the user picked for the source, if any.
    pub user_override: Option<PathBuf>,
    /// When the source was last synced, if ever.
    pub last_sync: Option<Timestamp>,
    /// Problems the last sync ran into.
    pub errors: Vec<String>,
}

impl SourceState {
    /// The state of a source we know nothing about yet: turned on, never synced.
    pub fn new(source: Source) -> Self {
        Self {
            source,
            enabled: true,
            resolved_location: None,
            user_override: None,
            last_sync: None,
            errors: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_id_is_the_source_and_native_id_joined_by_a_colon() {
        let id = GameId::new(Source::HeroicGog, "1207658924");

        assert_eq!(id.as_str(), "heroic-gog:1207658924");
        assert_eq!(id.source(), Source::HeroicGog);
        assert_eq!(id.native_id(), "1207658924");
    }

    #[test]
    fn a_game_id_parses_back_and_keeps_colons_in_the_native_id() {
        let id: GameId = "desktop:org.example:Game".parse().unwrap();

        assert_eq!(id.source(), Source::Desktop);
        assert_eq!(id.native_id(), "org.example:Game");
    }

    #[test]
    fn malformed_game_ids_are_rejected() {
        for text in ["steam", "steam:", "nintendo:1", ":1", ""] {
            assert!(text.parse::<GameId>().is_err(), "{text:?} parsed");
        }
    }

    #[test]
    fn every_enum_value_parses_back_from_its_text() {
        for source in Source::ALL {
            assert_eq!(source.as_str().parse::<Source>(), Ok(*source));
        }
        for kind in LaunchKind::ALL {
            assert_eq!(kind.as_str().parse::<LaunchKind>(), Ok(*kind));
        }
    }
}
