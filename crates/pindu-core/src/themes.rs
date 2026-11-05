//! Fullscreen designs: theme folders, each `themes/<id>/` with a `theme.json` that names
//! it and a `theme.css` with its rules. Pindu ships some; users add their own beside
//! `fullscreen.css`, and theirs win over a shipped one with the same id.

use std::fs;
use std::path::{Path, PathBuf};

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
}

/// The layout choices a theme makes in its `theme.json` `layout` object; a missing or
/// unknown value keeps the default, today's layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ThemeLayout {
    /// `"home-tile"`: the shape of Home's cards.
    pub home_tile: TileShape,
    /// `"top-bar"`: whether fullscreen mode shows a bar with page tabs and a clock.
    pub top_bar: bool,
}

impl ThemeLayout {
    fn from_json(layout: Option<&Json>) -> Self {
        let text = |key: &str| {
            layout
                .and_then(|layout| layout.get(key))
                .and_then(Json::as_str)
        };
        Self {
            home_tile: text("home-tile")
                .and_then(TileShape::from_name)
                .unwrap_or_default(),
            top_bar: layout
                .and_then(|layout| layout.get("top-bar"))
                .and_then(Json::as_bool)
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
    Square,
    /// Tall cards with the cover art.
    Portrait,
}

impl TileShape {
    /// The shape's name in `theme.json`, also its CSS class's suffix.
    pub fn name(self) -> &'static str {
        match self {
            Self::Landscape => "landscape",
            Self::Square => "square",
            Self::Portrait => "portrait",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        [Self::Landscape, Self::Square, Self::Portrait]
            .into_iter()
            .find(|shape| shape.name() == name)
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
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
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
    })
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
            r#"{"contract": 1, "name": "Odd", "layout": {"home-tile": "round"}}"#,
        );

        let found = discover(&[user.path()]);

        assert_eq!(found.themes.len(), 2);
        for theme in &found.themes {
            assert_eq!(theme.layout, ThemeLayout::default());
            assert_eq!(theme.layout.home_tile, TileShape::Landscape);
        }
    }
}
