//! Fullscreen designs: theme folders, each `themes/<id>/` with a `theme.json` that names
//! it and a `theme.css` with its rules. Pindu ships some; users add their own beside
//! `fullscreen.css`, and theirs win over a shipped one with the same id.

use std::fs;
use std::ops::RangeInclusive;
use std::path::{Component, Path, PathBuf};

use serde_json::Value as Json;

/// The version of the style contract (the documented classes and `theme.json` fields)
/// this Pindu launcher implements; themes say which one they were written for.
pub const CONTRACT: u64 = 1;

/// A design found in a theme folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// The folder's name, which the `fullscreen-design` setting holds.
    pub id: String,
    /// The theme's name, from `theme.json`.
    pub name: String,
    /// The theme's description, from `theme.json`; empty when it gives none.
    pub description: String,
    /// The theme folder.
    pub dir: PathBuf,
    /// The style contract the theme was written for.
    pub contract: u64,
    /// What the theme changes in fullscreen mode's layout, which CSS cannot.
    pub layout: ThemeLayout,
    /// The looks the theme offers within itself, in its order; empty for a theme with one
    /// look.
    pub variants: Vec<Variant>,
    /// A picture of the design (`theme.json` `preview`), SVG or PNG, inside the theme
    /// folder; `None` for a theme without one or with one Pindu refused.
    pub preview: Option<PathBuf>,
}

/// A look a theme offers within itself (`theme.json` `variants`): its stylesheet loads
/// after the theme's own `theme.css`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// What the `fullscreen-design-variant` setting holds for it: a–z, 0–9 and dashes.
    pub id: String,
    /// Shown in Quick Access.
    pub name: String,
    /// The variant's stylesheet, inside the theme folder; `None` for a variant that is the
    /// theme's own look.
    pub stylesheet: Option<PathBuf>,
}

/// The layout choices a theme makes in its `theme.json` `layout` object; a missing or
/// unknown value keeps the default, today's layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ThemeLayout {
    /// `"home-tile"`: the shape of Home's cards.
    pub home_tile: TileShape,
    /// `"home"`: where Home's carousel sits.
    pub home: HomeLayout,
    /// `"top-bar"`: whether fullscreen mode shows a bar with page tabs and a clock.
    pub top_bar: bool,
    /// `"top-bar-tabs"`: where the top bar's Home and Library tabs sit.
    pub top_bar_tabs: TopBarTabs,
    /// `"home-details"`: show the focused game's details on Home.
    pub home_details: bool,
    /// `"home-tile-size"`: the shorter side of Home's cards in pixels, within
    /// `HOME_TILE_SIZES`; `None` keeps each shape's own size.
    pub home_tile_size: Option<i32>,
    /// `"home-start"`: what the first screen's row starts with.
    pub home_start: HomeStart,
    /// `"home-section-tile"`: what a section's tile shows.
    pub home_section_tile: SectionTile,
    /// `"home-section-preview"`: a strip of the focused section's first covers under the
    /// details row, on the first screen.
    pub home_section_preview: bool,
    /// `"home-background-choice"`: whether the design offers Quick Access's choice of
    /// Home's animated background; without it Home shows the default one.
    pub home_background_choice: bool,
    /// `"panel-sections"`: where Quick Access's section strip sits.
    pub panel_sections: PanelSections,
    /// `"quick-tiles"`: how Quick's tiles are laid out.
    pub quick_tiles: QuickTiles,
    /// `"menu-entries"`: how the menu drawer's places, collections and ways out are laid
    /// out.
    pub menu_entries: MenuEntries,
    /// `"playing-card"`: how the Playing Now card is drawn.
    pub playing_card: PlayingCard,
}

/// The sizes, in pixels, a theme can give the shorter side of Home's cards.
pub const HOME_TILE_SIZES: RangeInclusive<i32> = 64..=400;

impl ThemeLayout {
    /// Adwaita's layout, in force while fullscreen mode shows no design: Home as rows
    /// that start with a carousel of the Jump Back In games (`HomeLayout::Explore`), 116
    /// pixel wide covers, labelled section tiles, the games' details, the top bar with its
    /// tabs in the middle, and the menus as GNOME arranges its own: Quick Access's sections
    /// across the top, Quick's tiles as two columns of pills, the menu's entries as rows and
    /// a compact Playing Now card. `ThemeLayout::default()`, what a `theme.json` leaves
    /// out, is not this.
    pub fn adwaita() -> Self {
        Self {
            home: HomeLayout::Explore,
            home_tile: TileShape::Portrait,
            home_tile_size: Some(116),
            top_bar: true,
            top_bar_tabs: TopBarTabs::Center,
            home_details: true,
            home_start: HomeStart::Sections,
            home_section_tile: SectionTile::Label,
            home_section_preview: false,
            home_background_choice: false,
            panel_sections: PanelSections::Top,
            quick_tiles: QuickTiles::Pills,
            menu_entries: MenuEntries::List,
            playing_card: PlayingCard::Compact,
        }
    }

    fn from_json(layout: Option<&Json>) -> Self {
        let value = |key: &str| layout.and_then(|layout| layout.get(key));
        let text = |key: &str| value(key).and_then(Json::as_str);
        let flag = |key: &str| value(key).and_then(Json::as_bool).unwrap_or_default();
        Self {
            home_tile: text("home-tile")
                .and_then(TileShape::from_name)
                .unwrap_or_default(),
            home: text("home")
                .and_then(HomeLayout::from_name)
                .unwrap_or_default(),
            top_bar: flag("top-bar"),
            top_bar_tabs: text("top-bar-tabs")
                .and_then(TopBarTabs::from_name)
                .unwrap_or_default(),
            home_details: flag("home-details"),
            home_tile_size: value("home-tile-size").and_then(Json::as_f64).map(|size| {
                (size.round() as i32).clamp(*HOME_TILE_SIZES.start(), *HOME_TILE_SIZES.end())
            }),
            home_start: text("home-start")
                .and_then(HomeStart::from_name)
                .unwrap_or_default(),
            home_section_tile: text("home-section-tile")
                .and_then(SectionTile::from_name)
                .unwrap_or_default(),
            home_section_preview: flag("home-section-preview"),
            home_background_choice: flag("home-background-choice"),
            panel_sections: text("panel-sections")
                .and_then(PanelSections::from_name)
                .unwrap_or_default(),
            quick_tiles: text("quick-tiles")
                .and_then(QuickTiles::from_name)
                .unwrap_or_default(),
            menu_entries: text("menu-entries")
                .and_then(MenuEntries::from_name)
                .unwrap_or_default(),
            playing_card: text("playing-card")
                .and_then(PlayingCard::from_name)
                .unwrap_or_default(),
        }
    }
}

/// The shape of the cards in fullscreen Home's carousel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileShape {
    /// Wide cards with the hero art.
    #[default]
    Landscape,
    /// Square cards.
    Square,
    /// Tall cards with the cover art.
    Portrait,
}

impl TileShape {
    /// Every shape, the default first.
    pub const ALL: [Self; 3] = [Self::Landscape, Self::Square, Self::Portrait];

    /// The shape's name in `theme.json`, also its CSS class's suffix.
    pub fn name(self) -> &'static str {
        match self {
            Self::Landscape => "landscape",
            Self::Square => "square",
            Self::Portrait => "portrait",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|shape| shape.name() == name)
    }
}

/// Where fullscreen Home's carousel sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeLayout {
    /// The carousel at the bottom, under the focused game's logo or title.
    #[default]
    HeroBottom,
    /// The carousel near the top, with the logo or title under it at the left.
    RowTop,
    /// One screen: the logo or title at the top, and under it every section as a row of
    /// its games.
    Shelves,
    /// One screen of rows under the top bar: a carousel of the Jump Back In games, a row
    /// of section tiles, then every other section as a row of its games.
    Explore,
}

impl HomeLayout {
    /// Every layout, the default first.
    pub const ALL: [Self; 4] = [Self::HeroBottom, Self::RowTop, Self::Shelves, Self::Explore];

    /// The layout's name in `theme.json`, also its CSS class's suffix.
    pub fn name(self) -> &'static str {
        match self {
            Self::HeroBottom => "hero-bottom",
            Self::RowTop => "row-top",
            Self::Shelves => "shelves",
            Self::Explore => "explore",
        }
    }

    /// Whether Home is one screen of rows that scroll, with no second screen and no
    /// background behind them: Shelves and Explore.
    pub fn one_screen(self) -> bool {
        matches!(self, Self::Shelves | Self::Explore)
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layout| layout.name() == name)
    }
}

/// What the row of fullscreen Home's first screen starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeStart {
    /// The sections, a tile each.
    #[default]
    Sections,
    /// The Recently Played games as cards, then the other sections as tiles.
    Games,
}

impl HomeStart {
    /// Every start, the default first.
    pub const ALL: [Self; 2] = [Self::Sections, Self::Games];

    /// The start's name in `theme.json`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Sections => "sections",
            Self::Games => "games",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|start| start.name() == name)
    }
}

/// Where the top bar's Home and Library tabs sit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TopBarTabs {
    /// At the start of the bar.
    #[default]
    Start,
    /// In the middle of the bar.
    Center,
}

impl TopBarTabs {
    /// Every place, the default first.
    pub const ALL: [Self; 2] = [Self::Start, Self::Center];

    /// The place's name in `theme.json`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tabs| tabs.name() == name)
    }
}

/// What a section's tile on fullscreen Home's first screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionTile {
    /// The section's art, `data/home/<name>.svg`, filling the tile.
    #[default]
    Art,
    /// A glyph, the section's name and its number of games.
    Label,
}

impl SectionTile {
    /// Every kind of tile, the default first.
    pub const ALL: [Self; 2] = [Self::Art, Self::Label];

    /// The kind's name in `theme.json`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Art => "art",
            Self::Label => "label",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tile| tile.name() == name)
    }
}

/// Where Quick Access's section strip sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelSections {
    /// Across the panel's top.
    #[default]
    Top,
    /// A rail of icons down the panel's inner edge, with the shown section's name over
    /// its content.
    Side,
}

impl PanelSections {
    /// Every place, the default first.
    pub const ALL: [Self; 2] = [Self::Top, Self::Side];

    /// The place's name in `theme.json`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Side => "side",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|sections| sections.name() == name)
    }
}

/// How Quick's tiles are laid out. Which tiles show and their order do not change with it
/// ([`crate::panels::quick_tiles`]); where each direction leads does
/// ([`crate::panels::step_arranged_tile`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuickTiles {
    /// A grid of three columns, each tile's icon over its name.
    #[default]
    Grid,
    /// Two columns of wide pills, each tile's icon beside its name.
    Pills,
    /// Full-width rows, each tile's second line at the row's end.
    List,
    /// Shelves, each a heading over a row that Left and Right move along: Devices and
    /// Library ([`crate::panels::TileShelf`]).
    Shelf,
}

impl QuickTiles {
    /// Every arrangement, the default first.
    pub const ALL: [Self; 4] = [Self::Grid, Self::Pills, Self::List, Self::Shelf];

    /// The arrangement's name in `theme.json`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::Pills => "pills",
            Self::List => "list",
            Self::Shelf => "shelf",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tiles| tiles.name() == name)
    }
}

/// How the menu drawer's places, collections and ways out are laid out. Which entries it
/// holds and their order do not change with it ([`crate::panels::drawer_entries`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuEntries {
    /// Rows, with the power dock under them.
    #[default]
    List,
    /// Three shelves of cards, each card with its icon and name: Go To, Collections and
    /// Leave ([`crate::panels::menu_shelves`]).
    Shelves,
}

impl MenuEntries {
    /// Every arrangement, the default first.
    pub const ALL: [Self; 2] = [Self::List, Self::Shelves];

    /// The arrangement's name in `theme.json`.
    pub fn name(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Shelves => "shelves",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|entries| entries.name() == name)
    }
}

/// How the Playing Now card is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayingCard {
    /// As it has been: "Playing Now" and how long the game has run, its title, then Return
    /// and Stop.
    #[default]
    Compact,
    /// A banner of the game's hero art, else its cover, across the panel's top, with the
    /// title over a scrim.
    Banner,
}

impl PlayingCard {
    /// Every way of drawing it, the default first.
    pub const ALL: [Self; 2] = [Self::Compact, Self::Banner];

    /// The way's name in `theme.json`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Banner => "banner",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|card| card.name() == name)
    }
}

impl Theme {
    /// The theme's stylesheet, loaded by path so `url()` resolves inside its folder.
    pub fn stylesheet(&self) -> PathBuf {
        self.dir.join("theme.css")
    }

    /// Whether this Pindu launcher can apply the theme: one written for a newer contract
    /// may rely on classes it does not have.
    pub fn supported(&self) -> bool {
        self.contract <= CONTRACT
    }

    /// The variant in force when the setting holds `id`: that one, or the theme's first
    /// for a missing or unknown id; `None` for a theme without variants.
    pub fn variant(&self, id: &str) -> Option<&Variant> {
        self.variants
            .iter()
            .find(|variant| variant.id == id)
            .or_else(|| self.variants.first())
    }
}

/// The themes in some folders, and the folders that are not valid themes.
#[derive(Debug, Default)]
pub struct Discovery {
    /// The themes found, sorted by name.
    pub themes: Vec<Theme>,
    /// A theme folder that could not be used, and why.
    pub skipped: Vec<(PathBuf, String)>,
    /// A theme found without something its `theme.json` asks for, such as its preview,
    /// and why.
    pub problems: Vec<(PathBuf, String)>,
}

/// The shipped design that took over from `id`, a design Pindu launcher used to ship, if it
/// is one: Ribbon and Panorama were retired for Lobby. A design of the user's own with
/// such an id is theirs to keep; the caller replaces only one that is not installed.
pub fn replacement(id: &str) -> Option<&'static str> {
    match id {
        "ribbon" | "panorama" => Some("lobby"),
        _ => None,
    }
}

/// Finds the themes in `folders`, each a folder of theme folders, sorted by name; a later
/// folder's theme replaces an earlier one with the same id.
pub fn discover(folders: &[&Path]) -> Discovery {
    let mut found = Discovery::default();
    for folder in folders {
        let Ok(entries) = fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            match read_theme(&dir) {
                Ok((theme, problem)) => {
                    if let Some(reason) = problem {
                        found.problems.push((dir, reason));
                    }
                    found.themes.retain(|known| known.id != theme.id);
                    found.themes.push(theme);
                }
                Err(reason) => found.skipped.push((dir, reason)),
            }
        }
    }
    found
        .themes
        .sort_by_cached_key(|theme| (theme.name.to_lowercase(), theme.id.clone()));
    found
}

/// The theme in `dir`, named after the folder, and why it lacks its preview if it asked for
/// one Pindu refused.
fn read_theme(dir: &Path) -> Result<(Theme, Option<String>), String> {
    let id = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !is_id(&id) {
        return Err("the folder name may only hold a–z, 0–9 and dashes".to_owned());
    }
    let json = fs::read(dir.join("theme.json")).map_err(|error| format!("theme.json: {error}"))?;
    let metadata: Json =
        serde_json::from_slice(&json).map_err(|error| format!("theme.json: {error}"))?;
    let text = |key: &str| metadata.get(key).and_then(Json::as_str).map(str::to_owned);
    let name = text("name").ok_or("theme.json has no name")?;
    let contract = metadata
        .get("contract")
        .and_then(Json::as_u64)
        .ok_or("theme.json has no contract version")?;
    if !dir.join("theme.css").is_file() {
        return Err("there is no theme.css".to_owned());
    }
    let variants = read_variants(dir, metadata.get("variants"))?;
    let (preview, problem) = match metadata.get("preview") {
        None => (None, None),
        Some(Json::String(path)) => match file_inside(dir, path, "preview") {
            Ok(file) => (Some(file), None),
            Err(reason) => (None, Some(reason)),
        },
        Some(_) => (None, Some("the preview is not a path".to_owned())),
    };
    let theme = Theme {
        id,
        name,
        description: text("description").unwrap_or_default(),
        dir: dir.to_owned(),
        contract,
        layout: ThemeLayout::from_json(metadata.get("layout")),
        variants,
        preview,
    };
    Ok((theme, problem))
}

/// Whether `id` may name a theme or a variant: a–z, 0–9 and dashes.
fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The variants in `theme.json`'s `variants` list, if it has one. A list Pindu cannot
/// follow makes the theme invalid rather than half applied.
fn read_variants(dir: &Path, variants: Option<&Json>) -> Result<Vec<Variant>, String> {
    let Some(variants) = variants else {
        return Ok(Vec::new());
    };
    let variants = variants
        .as_array()
        .ok_or("theme.json's variants is not a list")?;
    let mut read: Vec<Variant> = Vec::with_capacity(variants.len());
    for (index, variant) in variants.iter().enumerate() {
        let number = index + 1;
        let text = |key: &str| variant.get(key).and_then(Json::as_str);
        if !variant.is_object() {
            return Err(format!("variant {number} is not an object"));
        }
        let id = text("id")
            .filter(|id| is_id(id))
            .ok_or_else(|| format!("variant {number} needs an id of a–z, 0–9 and dashes"))?;
        if read.iter().any(|known| known.id == id) {
            return Err(format!("variant “{id}” is listed twice"));
        }
        let name = text("name")
            .filter(|name| !name.is_empty())
            .ok_or_else(|| format!("variant “{id}” has no name"))?;
        let stylesheet = match variant.get("stylesheet") {
            None => None,
            Some(Json::String(path)) => Some(
                file_inside(dir, path, "stylesheet")
                    .map_err(|reason| format!("variant “{id}”: {reason}"))?,
            ),
            Some(_) => return Err(format!("variant “{id}”'s stylesheet is not a path")),
        };
        read.push(Variant {
            id: id.to_owned(),
            name: name.to_owned(),
            stylesheet,
        });
    }
    Ok(read)
}

/// The file at `path`, relative to the theme folder `dir`, which it may not leave: not by
/// `..`, an absolute path or a symbolic link. `what` names it in the reason it is refused.
fn file_inside(dir: &Path, path: &str, what: &str) -> Result<PathBuf, String> {
    let relative = Path::new(path);
    let inside = relative
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
    if path.is_empty() || !inside {
        return Err(format!("the {what} {path} is not inside the theme folder"));
    }
    let file = dir.join(relative);
    let resolved = file
        .canonicalize()
        .map_err(|error| format!("the {what} {path}: {error}"))?;
    let folder = dir
        .canonicalize()
        .map_err(|error| format!("the theme folder: {error}"))?;
    if !resolved.starts_with(&folder) {
        return Err(format!("the {what} {path} is not inside the theme folder"));
    }
    if !resolved.is_file() {
        return Err(format!("the {what} {path} is not a file"));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn write_theme(root: &Path, id: &str, json: &str) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("theme.json"), json).unwrap();
        fs::write(dir.join("theme.css"), "").unwrap();
    }

    #[test]
    fn the_shipped_designs_are_valid_and_supported() {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/themes");

        let found = discover(&[&shipped]);

        assert!(found.skipped.is_empty(), "{:?}", found.skipped);
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        assert!(!found.themes.is_empty());
        for theme in &found.themes {
            assert!(theme.supported(), "{} needs a newer contract", theme.id);
            assert!(
                theme.dir.join("LICENSE").is_file(),
                "{} has no LICENSE",
                theme.id
            );
            assert!(theme.preview.is_some(), "{} has no preview", theme.id);
        }
    }

    #[test]
    fn retired_designs_are_replaced_by_lobby() {
        assert_eq!(replacement("ribbon"), Some("lobby"));
        assert_eq!(replacement("panorama"), Some("lobby"));
        for kept in ["lobby", "marquee", "shelves", "", "my-design"] {
            assert_eq!(replacement(kept), None, "{kept}");
        }
    }

    #[test]
    fn lobby_ships_and_the_retired_designs_do_not() {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/themes");

        let ids: Vec<String> = discover(&[&shipped])
            .themes
            .into_iter()
            .map(|theme| theme.id)
            .collect();

        assert!(ids.iter().any(|id| id == "lobby"), "{ids:?}");
        assert!(ids.iter().all(|id| replacement(id).is_none()), "{ids:?}");
    }

    #[test]
    fn a_theme_folder_is_found_with_its_name() {
        let bundled = tempfile::tempdir().unwrap();
        write_theme(
            bundled.path(),
            "ribbon",
            r#"{"contract": 1, "name": "Ribbon", "description": "Tiles across the top"}"#,
        );

        let found = discover(&[bundled.path()]);

        assert_eq!(found.themes.len(), 1);
        let theme = &found.themes[0];
        assert_eq!(theme.id, "ribbon");
        assert_eq!(theme.name, "Ribbon");
        assert_eq!(theme.description, "Tiles across the top");
        assert_eq!(theme.stylesheet(), bundled.path().join("ribbon/theme.css"));
    }

    #[test]
    fn a_user_theme_replaces_a_shipped_one_with_the_same_id() {
        let bundled = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        write_theme(
            bundled.path(),
            "ribbon",
            r#"{"contract": 1, "name": "Ribbon"}"#,
        );
        write_theme(
            user.path(),
            "ribbon",
            r#"{"contract": 1, "name": "My Ribbon"}"#,
        );

        let found = discover(&[bundled.path(), user.path()]);

        assert_eq!(found.themes.len(), 1);
        assert_eq!(found.themes[0].name, "My Ribbon");
        assert_eq!(found.themes[0].dir, user.path().join("ribbon"));
    }

    #[test]
    fn invalid_theme_folders_are_skipped_with_a_reason() {
        let user = tempfile::tempdir().unwrap();
        write_theme(user.path(), "broken", "{ not json");
        write_theme(user.path(), "Bad Id", r#"{"contract": 1, "name": "Bad"}"#);
        write_theme(
            user.path(),
            "no-css",
            r#"{"contract": 1, "name": "No CSS"}"#,
        );
        fs::remove_file(user.path().join("no-css/theme.css")).unwrap();
        fs::write(user.path().join("notes.txt"), "not a theme").unwrap();

        let found = discover(&[user.path()]);

        assert!(found.themes.is_empty());
        let mut skipped: Vec<_> = found
            .skipped
            .iter()
            .map(|(dir, _)| dir.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        skipped.sort();
        assert_eq!(skipped, ["Bad Id", "broken", "no-css"]);
    }

    #[test]
    fn themes_come_sorted_by_name() {
        let bundled = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        write_theme(bundled.path(), "a", r#"{"contract": 1, "name": "Zebra"}"#);
        write_theme(bundled.path(), "b", r#"{"contract": 1, "name": "apple"}"#);
        write_theme(user.path(), "c", r#"{"contract": 1, "name": "Mango"}"#);

        let found = discover(&[bundled.path(), user.path()]);

        let names: Vec<_> = found.themes.iter().map(|theme| &theme.name).collect();
        assert_eq!(names, ["apple", "Mango", "Zebra"]);
    }

    #[test]
    fn a_theme_for_a_newer_contract_is_listed_but_not_supported() {
        let user = tempfile::tempdir().unwrap();
        write_theme(
            user.path(),
            "future",
            r#"{"contract": 99, "name": "Future"}"#,
        );
        write_theme(user.path(), "today", r#"{"contract": 1, "name": "Today"}"#);

        let found = discover(&[user.path()]);

        let supported = |id: &str| {
            found
                .themes
                .iter()
                .find(|theme| theme.id == id)
                .map(Theme::supported)
        };
        assert_eq!(supported("future"), Some(false));
        assert_eq!(supported("today"), Some(true));
    }

    #[test]
    fn a_theme_chooses_its_home_tile_shape() {
        let user = tempfile::tempdir().unwrap();
        write_theme(
            user.path(),
            "square",
            r#"{"contract": 1, "name": "Square", "layout": {"home-tile": "square"}}"#,
        );
        write_theme(
            user.path(),
            "portrait",
            r#"{"contract": 1, "name": "Portrait", "layout": {"home-tile": "portrait"}}"#,
        );

        let found = discover(&[user.path()]);

        let shape = |id: &str| {
            found
                .themes
                .iter()
                .find(|theme| theme.id == id)
                .map(|theme| theme.layout.home_tile)
        };
        assert_eq!(shape("square"), Some(TileShape::Square));
        assert_eq!(shape("portrait"), Some(TileShape::Portrait));
    }

    #[test]
    fn a_theme_turns_the_top_bar_on() {
        let user = tempfile::tempdir().unwrap();
        write_theme(
            user.path(),
            "bar",
            r#"{"contract": 1, "name": "Bar", "layout": {"top-bar": true}}"#,
        );
        write_theme(
            user.path(),
            "odd",
            r#"{"contract": 1, "name": "Odd", "layout": {"top-bar": "yes"}}"#,
        );

        let found = discover(&[user.path()]);

        let top_bar = |id: &str| {
            found
                .themes
                .iter()
                .find(|theme| theme.id == id)
                .map(|theme| theme.layout.top_bar)
        };
        assert_eq!(top_bar("bar"), Some(true));
        assert_eq!(top_bar("odd"), Some(false));
    }

    #[test]
    fn a_missing_or_unknown_layout_keeps_todays() {
        let user = tempfile::tempdir().unwrap();
        write_theme(user.path(), "plain", r#"{"contract": 1, "name": "Plain"}"#);
        write_theme(
            user.path(),
            "odd",
            r#"{"contract": 1, "name": "Odd", "layout": {"home-tile": "round", "home": "left"}}"#,
        );

        let found = discover(&[user.path()]);

        assert_eq!(found.themes.len(), 2);
        for theme in &found.themes {
            assert_eq!(theme.layout, ThemeLayout::default());
            assert_eq!(theme.layout.home_tile, TileShape::Landscape);
            assert_eq!(theme.layout.home, HomeLayout::HeroBottom);
            assert!(!theme.layout.home_details);
        }
    }

    #[test]
    fn a_theme_can_put_the_home_row_at_the_top() {
        let user = tempfile::tempdir().unwrap();
        write_theme(
            user.path(),
            "ribbon",
            r#"{"contract": 1, "name": "Ribbon", "layout": {"home": "row-top"}}"#,
        );

        let found = discover(&[user.path()]);

        assert_eq!(found.themes[0].layout.home, HomeLayout::RowTop);
        assert_eq!(found.themes[0].layout.home_tile, TileShape::Landscape);
    }

    fn layout_of(layout: &str) -> ThemeLayout {
        let user = tempfile::tempdir().unwrap();
        write_theme(
            user.path(),
            "design",
            &format!(r#"{{"contract": 1, "name": "Design", "layout": {layout}}}"#),
        );
        let found = discover(&[user.path()]);
        assert!(found.skipped.is_empty(), "{:?}", found.skipped);
        found.themes[0].layout
    }

    #[test]
    fn a_theme_can_make_home_one_screen_of_shelves() {
        assert_eq!(
            layout_of(r#"{"home": "shelves"}"#).home,
            HomeLayout::Shelves
        );
        assert_eq!(HomeLayout::Shelves.name(), "shelves");
        assert_eq!(
            HomeLayout::ALL,
            [
                HomeLayout::HeroBottom,
                HomeLayout::RowTop,
                HomeLayout::Shelves,
                HomeLayout::Explore
            ]
        );
        assert_eq!(
            layout_of(r#"{"home": "rows"}"#).home,
            HomeLayout::HeroBottom
        );
        assert_eq!(CONTRACT, 1);
    }

    #[test]
    fn a_theme_can_make_home_explore_rows() {
        assert_eq!(
            layout_of(r#"{"home": "explore"}"#).home,
            HomeLayout::Explore
        );
        assert_eq!(HomeLayout::Explore.name(), "explore");
        assert_eq!(HomeLayout::ALL.last(), Some(&HomeLayout::Explore));
        assert_eq!(
            layout_of(r#"{"home": "Explore"}"#).home,
            HomeLayout::HeroBottom
        );
    }

    #[test]
    fn shelves_and_explore_are_one_screen_of_rows() {
        assert!(HomeLayout::Shelves.one_screen());
        assert!(HomeLayout::Explore.one_screen());
        assert!(!HomeLayout::HeroBottom.one_screen());
        assert!(!HomeLayout::RowTop.one_screen());
    }

    #[test]
    fn a_theme_can_centre_the_top_bar_tabs() {
        assert_eq!(
            layout_of(r#"{"top-bar-tabs": "center"}"#).top_bar_tabs,
            TopBarTabs::Center
        );
        assert_eq!(
            layout_of(r#"{"top-bar-tabs": "start"}"#).top_bar_tabs,
            TopBarTabs::Start
        );
        for odd in [
            r#"{"top-bar-tabs": "end"}"#,
            r#"{"top-bar-tabs": true}"#,
            "{}",
        ] {
            assert_eq!(layout_of(odd).top_bar_tabs, TopBarTabs::Start, "{odd}");
        }
        assert_eq!(TopBarTabs::Center.name(), "center");
    }

    #[test]
    fn adwaita_has_a_layout_of_its_own() {
        assert_eq!(
            ThemeLayout::adwaita(),
            ThemeLayout {
                home: HomeLayout::Explore,
                home_tile: TileShape::Portrait,
                home_tile_size: Some(116),
                top_bar: true,
                top_bar_tabs: TopBarTabs::Center,
                home_details: true,
                home_section_tile: SectionTile::Label,
                panel_sections: PanelSections::Top,
                quick_tiles: QuickTiles::Pills,
                menu_entries: MenuEntries::List,
                playing_card: PlayingCard::Compact,
                ..ThemeLayout::default()
            }
        );
        // What a `theme.json` leaves out stays today's.
        assert_eq!(ThemeLayout::default().home, HomeLayout::HeroBottom);
        assert!(!ThemeLayout::default().top_bar);
    }

    #[test]
    fn the_arrangement_options_name_their_values_the_default_first() {
        assert_eq!(PanelSections::ALL.map(PanelSections::name), ["top", "side"]);
        assert_eq!(
            QuickTiles::ALL.map(QuickTiles::name),
            ["grid", "pills", "list", "shelf"]
        );
        assert_eq!(MenuEntries::ALL.map(MenuEntries::name), ["list", "shelves"]);
        assert_eq!(
            PlayingCard::ALL.map(PlayingCard::name),
            ["compact", "banner"]
        );
        assert_eq!(PanelSections::default(), PanelSections::ALL[0]);
        assert_eq!(QuickTiles::default(), QuickTiles::ALL[0]);
        assert_eq!(MenuEntries::default(), MenuEntries::ALL[0]);
        assert_eq!(PlayingCard::default(), PlayingCard::ALL[0]);
    }

    #[test]
    fn a_theme_can_place_quick_access_s_sections() {
        for sections in PanelSections::ALL {
            assert_eq!(
                layout_of(&format!(r#"{{"panel-sections": "{}"}}"#, sections.name()))
                    .panel_sections,
                sections
            );
        }
        for odd in [
            r#"{"panel-sections": "bottom"}"#,
            r#"{"panel-sections": "Side"}"#,
            r#"{"panel-sections": true}"#,
            "{}",
        ] {
            assert_eq!(layout_of(odd).panel_sections, PanelSections::Top, "{odd}");
        }
    }

    #[test]
    fn a_theme_can_arrange_quick_s_tiles() {
        for tiles in QuickTiles::ALL {
            assert_eq!(
                layout_of(&format!(r#"{{"quick-tiles": "{}"}}"#, tiles.name())).quick_tiles,
                tiles
            );
        }
        for odd in [
            r#"{"quick-tiles": "shelves"}"#,
            r#"{"quick-tiles": "Pills"}"#,
            r#"{"quick-tiles": 2}"#,
            "{}",
        ] {
            assert_eq!(layout_of(odd).quick_tiles, QuickTiles::Grid, "{odd}");
        }
    }

    #[test]
    fn a_theme_can_arrange_the_menu_s_entries() {
        for entries in MenuEntries::ALL {
            assert_eq!(
                layout_of(&format!(r#"{{"menu-entries": "{}"}}"#, entries.name())).menu_entries,
                entries
            );
        }
        for odd in [
            r#"{"menu-entries": "shelf"}"#,
            r#"{"menu-entries": "grid"}"#,
            r#"{"menu-entries": null}"#,
            "{}",
        ] {
            assert_eq!(layout_of(odd).menu_entries, MenuEntries::List, "{odd}");
        }
    }

    #[test]
    fn a_theme_can_draw_the_playing_card_as_a_banner() {
        for card in PlayingCard::ALL {
            assert_eq!(
                layout_of(&format!(r#"{{"playing-card": "{}"}}"#, card.name())).playing_card,
                card
            );
        }
        for odd in [
            r#"{"playing-card": "hero"}"#,
            r#"{"playing-card": "Banner"}"#,
            r#"{"playing-card": false}"#,
            "{}",
        ] {
            assert_eq!(layout_of(odd).playing_card, PlayingCard::Compact, "{odd}");
        }
        // Adding options does not raise the contract.
        assert_eq!(CONTRACT, 1);
    }

    #[test]
    fn each_shipped_design_arranges_the_menus_its_own_way() {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/themes");
        let found = discover(&[&shipped]);
        let arrangement = |id: &str| {
            let layout = found
                .themes
                .iter()
                .find(|theme| theme.id == id)
                .map(|theme| theme.layout)
                .unwrap_or_else(|| panic!("{id} is not shipped"));
            (
                layout.panel_sections,
                layout.quick_tiles,
                layout.menu_entries,
                layout.playing_card,
            )
        };

        assert_eq!(
            arrangement("lobby"),
            (
                PanelSections::Top,
                QuickTiles::Grid,
                MenuEntries::List,
                PlayingCard::Compact
            )
        );
        assert_eq!(
            arrangement("marquee"),
            (
                PanelSections::Side,
                QuickTiles::List,
                MenuEntries::List,
                PlayingCard::Banner
            )
        );
        assert_eq!(
            arrangement("shelves"),
            (
                PanelSections::Top,
                QuickTiles::Shelf,
                MenuEntries::Shelves,
                PlayingCard::Compact
            )
        );
    }

    #[test]
    fn a_theme_can_start_home_with_the_recent_games() {
        assert_eq!(
            layout_of(r#"{"home-start": "games"}"#).home_start,
            HomeStart::Games
        );
        assert_eq!(
            layout_of(r#"{"home-start": "sections"}"#).home_start,
            HomeStart::Sections
        );
        for odd in [
            r#"{"home-start": "library"}"#,
            r#"{"home-start": true}"#,
            "{}",
        ] {
            assert_eq!(layout_of(odd).home_start, HomeStart::Sections, "{odd}");
        }
    }

    #[test]
    fn a_theme_can_label_its_section_tiles() {
        assert_eq!(
            layout_of(r#"{"home-section-tile": "label"}"#).home_section_tile,
            SectionTile::Label
        );
        assert_eq!(
            layout_of(r#"{"home-section-tile": "art"}"#).home_section_tile,
            SectionTile::Art
        );
        for odd in [
            r#"{"home-section-tile": "icon"}"#,
            r#"{"home-section-tile": 1}"#,
            "{}",
        ] {
            assert_eq!(layout_of(odd).home_section_tile, SectionTile::Art, "{odd}");
        }
    }

    #[test]
    fn a_theme_can_preview_the_focused_section() {
        assert!(layout_of(r#"{"home-section-preview": true}"#).home_section_preview);
        for off in [
            r#"{"home-section-preview": false}"#,
            r#"{"home-section-preview": "true"}"#,
            "{}",
        ] {
            assert!(!layout_of(off).home_section_preview, "{off}");
        }
    }

    #[test]
    fn a_theme_can_offer_the_background_choice() {
        assert!(layout_of(r#"{"home-background-choice": true}"#).home_background_choice);
        for off in [
            r#"{"home-background-choice": false}"#,
            r#"{"home-background-choice": 1}"#,
            "{}",
        ] {
            assert!(!layout_of(off).home_background_choice, "{off}");
        }
        // No design at all, Adwaita, does not offer it either.
        assert!(!ThemeLayout::default().home_background_choice);
    }

    #[test]
    fn a_theme_can_show_the_home_details() {
        let user = tempfile::tempdir().unwrap();
        write_theme(
            user.path(),
            "details",
            r#"{"contract": 1, "name": "Details", "layout": {"home-details": true}}"#,
        );
        write_theme(
            user.path(),
            "quoted",
            r#"{"contract": 1, "name": "Quoted", "layout": {"home-details": "true"}}"#,
        );

        let found = discover(&[user.path()]);

        let details = |id: &str| {
            found
                .themes
                .iter()
                .find(|theme| theme.id == id)
                .map(|theme| theme.layout.home_details)
        };
        assert_eq!(details("details"), Some(true));
        assert_eq!(details("quoted"), Some(false));
    }

    #[test]
    fn a_theme_can_choose_the_home_tile_size() {
        let user = tempfile::tempdir().unwrap();
        write_theme(
            user.path(),
            "small",
            r#"{"contract": 1, "name": "Small", "layout": {"home-tile-size": 112}}"#,
        );
        write_theme(user.path(), "plain", r#"{"contract": 1, "name": "Plain"}"#);

        let found = discover(&[user.path()]);

        let size = |id: &str| {
            found
                .themes
                .iter()
                .find(|theme| theme.id == id)
                .map(|theme| theme.layout.home_tile_size)
        };
        assert_eq!(size("small"), Some(Some(112)));
        assert_eq!(size("plain"), Some(None));
    }

    #[test]
    fn the_home_tile_size_is_clamped_and_an_invalid_one_ignored() {
        let user = tempfile::tempdir().unwrap();
        for (id, size) in [
            ("tiny", "10"),
            ("huge", "5000"),
            ("negative", "-3"),
            ("fraction", "112.4"),
            ("quoted", r#""112""#),
            ("null", "null"),
        ] {
            write_theme(
                user.path(),
                id,
                &format!(
                    r#"{{"contract": 1, "name": "{id}", "layout": {{"home-tile-size": {size}}}}}"#
                ),
            );
        }

        let found = discover(&[user.path()]);

        let size = |id: &str| {
            found
                .themes
                .iter()
                .find(|theme| theme.id == id)
                .map(|theme| theme.layout.home_tile_size)
        };
        assert_eq!(size("tiny"), Some(Some(64)));
        assert_eq!(size("huge"), Some(Some(400)));
        assert_eq!(size("negative"), Some(Some(64)));
        assert_eq!(size("fraction"), Some(Some(112)));
        assert_eq!(size("quoted"), Some(None));
        assert_eq!(size("null"), Some(None));
    }

    fn variant_ids(theme: &Theme) -> Vec<&str> {
        theme
            .variants
            .iter()
            .map(|variant| variant.id.as_str())
            .collect()
    }

    #[test]
    fn a_theme_lists_its_variants_in_order() {
        let user = tempfile::tempdir().unwrap();
        write_theme(
            user.path(),
            "duo",
            r#"{"contract": 1, "name": "Duo", "variants": [
                {"id": "classic", "name": "Classic"},
                {"id": "mono", "name": "Monochrome", "stylesheet": "variants/mono.css"}
            ]}"#,
        );
        fs::create_dir(user.path().join("duo/variants")).unwrap();
        fs::write(user.path().join("duo/variants/mono.css"), "").unwrap();

        let found = discover(&[user.path()]);

        assert!(found.skipped.is_empty(), "{:?}", found.skipped);
        let theme = &found.themes[0];
        assert_eq!(variant_ids(theme), ["classic", "mono"]);
        assert_eq!(theme.variants[0].name, "Classic");
        assert_eq!(theme.variants[0].stylesheet, None);
        assert_eq!(theme.variants[1].name, "Monochrome");
        assert_eq!(
            theme.variants[1].stylesheet,
            Some(user.path().join("duo/variants/mono.css"))
        );
    }

    #[test]
    fn a_theme_without_variants_has_none() {
        let user = tempfile::tempdir().unwrap();
        write_theme(user.path(), "plain", r#"{"contract": 1, "name": "Plain"}"#);
        write_theme(
            user.path(),
            "empty",
            r#"{"contract": 1, "name": "Empty", "variants": []}"#,
        );

        let found = discover(&[user.path()]);

        assert_eq!(found.themes.len(), 2);
        for theme in &found.themes {
            assert!(theme.variants.is_empty(), "{}", theme.id);
            assert_eq!(theme.variant("anything"), None);
        }
    }

    #[test]
    fn a_theme_with_malformed_variants_is_skipped_with_a_reason() {
        let user = tempfile::tempdir().unwrap();
        for (id, variants) in [
            ("not-a-list", r#"{"id": "a", "name": "A"}"#),
            ("not-an-object", r#"["a"]"#),
            ("no-id", r#"[{"name": "A"}]"#),
            ("no-name", r#"[{"id": "a"}]"#),
            ("empty-name", r#"[{"id": "a", "name": ""}]"#),
            ("bad-id", r#"[{"id": "Big A", "name": "A"}]"#),
            (
                "twice",
                r#"[{"id": "a", "name": "A"}, {"id": "a", "name": "B"}]"#,
            ),
            (
                "odd-sheet",
                r#"[{"id": "a", "name": "A", "stylesheet": 3}]"#,
            ),
            (
                "no-sheet",
                r#"[{"id": "a", "name": "A", "stylesheet": "gone.css"}]"#,
            ),
        ] {
            write_theme(
                user.path(),
                id,
                &format!(r#"{{"contract": 1, "name": "{id}", "variants": {variants}}}"#),
            );
        }

        let found = discover(&[user.path()]);

        assert!(found.themes.is_empty(), "{:?}", found.themes);
        assert_eq!(found.skipped.len(), 9);
        for (dir, reason) in &found.skipped {
            assert!(reason.contains("variant"), "{}: {reason}", dir.display());
        }
    }

    #[test]
    fn a_variant_stylesheet_outside_the_theme_folder_is_refused() {
        let user = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("evil.css"), "").unwrap();
        write_theme(user.path(), "other", r#"{"contract": 1, "name": "Other"}"#);
        let absolute = outside.path().join("evil.css");
        for (id, sheet) in [
            ("parent", "../other/theme.css".to_owned()),
            ("nested-parent", "assets/../../other/theme.css".to_owned()),
            ("absolute", absolute.display().to_string()),
        ] {
            write_theme(
                user.path(),
                id,
                &format!(
                    r#"{{"contract": 1, "name": "{id}", "variants": [{{"id": "a", "name": "A", "stylesheet": {}}}]}}"#,
                    Json::from(sheet)
                ),
            );
        }
        write_theme(
            user.path(),
            "linked",
            r#"{"contract": 1, "name": "Linked", "variants": [{"id": "a", "name": "A", "stylesheet": "evil.css"}]}"#,
        );
        std::os::unix::fs::symlink(&absolute, user.path().join("linked/evil.css")).unwrap();

        let found = discover(&[user.path()]);

        let ids: Vec<_> = found.themes.iter().map(|theme| theme.id.as_str()).collect();
        assert_eq!(ids, ["other"]);
        let mut skipped: Vec<_> = found
            .skipped
            .iter()
            .map(|(dir, _)| dir.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        skipped.sort();
        assert_eq!(skipped, ["absolute", "linked", "nested-parent", "parent"]);
    }

    #[test]
    fn a_theme_names_its_preview_inside_its_folder() {
        let user = tempfile::tempdir().unwrap();
        write_theme(
            user.path(),
            "pictured",
            r#"{"contract": 1, "name": "Pictured", "preview": "assets/preview.svg"}"#,
        );
        fs::create_dir(user.path().join("pictured/assets")).unwrap();
        fs::write(user.path().join("pictured/assets/preview.svg"), "<svg/>").unwrap();
        write_theme(user.path(), "plain", r#"{"contract": 1, "name": "Plain"}"#);

        let found = discover(&[user.path()]);

        let previews: Vec<_> = found
            .themes
            .iter()
            .map(|theme| (theme.id.as_str(), theme.preview.clone()))
            .collect();
        assert_eq!(
            previews,
            [
                (
                    "pictured",
                    Some(user.path().join("pictured/assets/preview.svg"))
                ),
                ("plain", None),
            ]
        );
        assert!(found.problems.is_empty(), "{:?}", found.problems);
    }

    #[test]
    fn a_bad_preview_is_refused_and_the_theme_still_lists() {
        let user = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let absolute = outside.path().join("evil.png");
        fs::write(&absolute, "").unwrap();
        write_theme(user.path(), "other", r#"{"contract": 1, "name": "Other"}"#);
        fs::write(user.path().join("other/preview.png"), "").unwrap();
        for (id, preview) in [
            ("missing", Json::from("gone.png")),
            ("parent", Json::from("../other/preview.png")),
            (
                "nested-parent",
                Json::from("assets/../../other/preview.png"),
            ),
            ("absolute", Json::from(absolute.display().to_string())),
            ("linked", Json::from("evil.png")),
            ("not-a-path", Json::from(3)),
        ] {
            write_theme(
                user.path(),
                id,
                &format!(r#"{{"contract": 1, "name": "{id}", "preview": {preview}}}"#),
            );
        }
        std::os::unix::fs::symlink(&absolute, user.path().join("linked/evil.png")).unwrap();

        let found = discover(&[user.path()]);

        assert!(found.skipped.is_empty(), "{:?}", found.skipped);
        let mut ids: Vec<_> = found
            .themes
            .iter()
            .filter(|theme| theme.id != "other")
            .map(|theme| {
                assert_eq!(theme.preview, None, "{}", theme.id);
                theme.id.as_str()
            })
            .collect();
        ids.sort_unstable();
        let mut troubled: Vec<_> = found
            .problems
            .iter()
            .map(|(dir, reason)| {
                assert!(reason.contains("preview"), "{}: {reason}", dir.display());
                dir.file_name().unwrap().to_string_lossy().into_owned()
            })
            .collect();
        troubled.sort();
        let expected = [
            "absolute",
            "linked",
            "missing",
            "nested-parent",
            "not-a-path",
            "parent",
        ];
        assert_eq!(ids, expected);
        assert_eq!(troubled, expected);
    }

    #[test]
    fn a_preview_path_through_dot_dot_is_refused_even_where_it_stays_inside() {
        let user = tempfile::tempdir().unwrap();
        write_theme(
            user.path(),
            "roundabout",
            r#"{"contract": 1, "name": "Roundabout", "preview": "assets/../preview.svg"}"#,
        );
        fs::create_dir(user.path().join("roundabout/assets")).unwrap();
        fs::write(user.path().join("roundabout/preview.svg"), "<svg/>").unwrap();

        let found = discover(&[user.path()]);

        assert_eq!(found.themes.len(), 1);
        assert_eq!(found.themes[0].preview, None);
        assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
    }

    #[test]
    fn a_missing_or_unknown_variant_falls_back_to_the_first() {
        let user = tempfile::tempdir().unwrap();
        write_theme(
            user.path(),
            "duo",
            r#"{"contract": 1, "name": "Duo", "variants": [
                {"id": "classic", "name": "Classic"},
                {"id": "mono", "name": "Monochrome"}
            ]}"#,
        );

        let found = discover(&[user.path()]);

        let theme = &found.themes[0];
        let chosen = |id: &str| theme.variant(id).map(|variant| variant.id.as_str());
        assert_eq!(chosen("mono"), Some("mono"));
        assert_eq!(chosen("classic"), Some("classic"));
        assert_eq!(chosen(""), Some("classic"));
        assert_eq!(chosen("gone"), Some("classic"));
    }
}
