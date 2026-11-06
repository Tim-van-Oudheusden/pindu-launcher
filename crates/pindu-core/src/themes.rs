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
    pub name: String,
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
    /// `"home-details"`: show the focused game's details on Home.
    pub home_details: bool,
    /// `"home-tile-size"`: the shorter side of Home's cards in pixels, within
    /// `HOME_TILE_SIZES`; `None` keeps each shape's own size.
    pub home_tile_size: Option<i32>,
}

/// The sizes, in pixels, a theme can give the shorter side of Home's cards.
pub const HOME_TILE_SIZES: RangeInclusive<i32> = 64..=400;

impl ThemeLayout {
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
            home_details: flag("home-details"),
            home_tile_size: value("home-tile-size").and_then(Json::as_f64).map(|size| {
                (size.round() as i32).clamp(*HOME_TILE_SIZES.start(), *HOME_TILE_SIZES.end())
            }),
        }
    }
}

/// The shape of the cards in fullscreen Home's carousel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileShape {
    /// Wide cards with the hero art.
    #[default]
    Landscape,
    Square,
    /// Tall cards with the cover art.
    Portrait,
}

impl TileShape {
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
}

impl HomeLayout {
    pub const ALL: [Self; 2] = [Self::HeroBottom, Self::RowTop];

    /// The layout's name in `theme.json`, also its CSS class's suffix.
    pub fn name(self) -> &'static str {
        match self {
            Self::HeroBottom => "hero-bottom",
            Self::RowTop => "row-top",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layout| layout.name() == name)
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
    pub themes: Vec<Theme>,
    /// A theme folder that could not be used, and why.
    pub skipped: Vec<(PathBuf, String)>,
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
                Ok(theme) => {
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

/// The theme in `dir`, named after the folder.
fn read_theme(dir: &Path) -> Result<Theme, String> {
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
    Ok(Theme {
        id,
        name,
        description: text("description").unwrap_or_default(),
        dir: dir.to_owned(),
        contract,
        layout: ThemeLayout::from_json(metadata.get("layout")),
        variants: read_variants(dir, metadata.get("variants"))?,
    })
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
                variant_stylesheet(dir, path)
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

/// The stylesheet at `path`, relative to the theme folder `dir`, which it may not leave:
/// not by `..`, an absolute path or a symbolic link.
fn variant_stylesheet(dir: &Path, path: &str) -> Result<PathBuf, String> {
    let relative = Path::new(path);
    let inside = relative
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
    if path.is_empty() || !inside {
        return Err(format!(
            "the stylesheet {path} is not inside the theme folder"
        ));
    }
    let file = dir.join(relative);
    let resolved = file
        .canonicalize()
        .map_err(|error| format!("the stylesheet {path}: {error}"))?;
    let folder = dir
        .canonicalize()
        .map_err(|error| format!("the theme folder: {error}"))?;
    if !resolved.starts_with(&folder) {
        return Err(format!(
            "the stylesheet {path} is not inside the theme folder"
        ));
    }
    if !resolved.is_file() {
        return Err(format!("the stylesheet {path} is not a file"));
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
        assert!(!found.themes.is_empty());
        for theme in &found.themes {
            assert!(theme.supported(), "{} needs a newer contract", theme.id);
            assert!(
                theme.dir.join("LICENSE").is_file(),
                "{} has no LICENSE",
                theme.id
            );
        }
        assert!(
            found.themes.iter().any(|theme| theme.variants.len() >= 2),
            "a shipped design is the worked example of variants"
        );
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
