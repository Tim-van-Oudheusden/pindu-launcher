//! The catalogue's index (`docs/get-games.md`, section 6.6): the entries Get Games can
//! offer, kept as JSON in the cache folder so later starts skip parsing the 50 MB catalogue.
//! It is rebuilt when the catalogue, the session's languages or the lists' categories and
//! extras change, and is safe to delete.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::{Value, json};

use super::appstream::{self, App, Block, Kind, Licence, Screenshot};
use super::lists::Lists;
use crate::add_store::Installation;
use crate::paths::APP_DIR;

/// Why the catalogue could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogueError {
    /// The installation has no catalogue: Flatpak has not downloaded it yet.
    Missing,
    /// The catalogue is there but could not be read, with why.
    Broken(String),
}

/// Where the entries came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The index, as an earlier start left it.
    Index,
    /// The catalogue, parsed again because the index was missing or stale.
    Catalogue,
}

/// The index of `installation`'s catalogue, in the cache folder (`$XDG_CACHE_HOME`, else
/// `~/.cache`).
pub fn index_path(
    xdg_cache_home: Option<&Path>,
    home: &Path,
    installation: Installation,
) -> PathBuf {
    let cache_home = match xdg_cache_home {
        Some(path) if path.is_absolute() => path.to_owned(),
        _ => home.join(".cache"),
    };
    let file = match installation {
        Installation::System => "system.json",
        Installation::User => "user.json",
    };
    cache_home.join(APP_DIR).join("catalogue").join(file)
}

/// The entries of the catalogue in `active` (its folder, [`super::active_dir`]) that `lists`
/// may offer: applications in its categories or among its extras, in the first of
/// `languages`. They come from the index at `index` when it was made from this catalogue
/// for these languages and lists; otherwise the catalogue is parsed and the index written
/// again, and failing to write it only costs the next start a parse.
pub fn load(
    active: &Path,
    index: &Path,
    languages: &[String],
    lists: &Lists,
) -> Result<(Vec<App>, Origin), CatalogueError> {
    let catalogue = active.join("appstream.xml");
    let unreadable = |error: std::io::Error| match error.kind() {
        ErrorKind::NotFound => CatalogueError::Missing,
        _ => CatalogueError::Broken(error.to_string()),
    };
    let metadata = fs::metadata(&catalogue).map_err(unreadable)?;
    let stamp = stamp(&metadata, languages, lists);
    if let Some(apps) = read_index(index, &stamp) {
        return Ok((apps, Origin::Index));
    }
    let xml = fs::read_to_string(&catalogue).map_err(unreadable)?;
    let apps: Vec<App> = appstream::parse(&xml, languages)
        .map_err(|error| CatalogueError::Broken(error.0))?
        .into_iter()
        .filter(|app| super::offerable(app, lists))
        .collect();
    // A cache: without it the next start parses the catalogue again.
    let _ = write_index(index, stamp, &apps);
    Ok((apps, Origin::Catalogue))
}

/// The version of the index's format; an index in another is made again.
const FORMAT: u64 = 2;

/// What an index was made from: the catalogue file as it was then, the languages and the
/// parts of the lists that decide which entries it holds.
fn stamp(metadata: &fs::Metadata, languages: &[String], lists: &Lists) -> Value {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_nanos().to_string());
    json!({
        "format": FORMAT,
        "modified": modified,
        "size": metadata.len(),
        "languages": languages,
        "categories": lists.categories,
        "extras": lists.extras,
    })
}

/// The entries in the index at `path` when it was made as `stamp` says.
fn read_index(path: &Path, stamp: &Value) -> Option<Vec<App>> {
    let index: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    if index.get("stamp")? != stamp {
        return None;
    }
    index
        .get("apps")?
        .as_array()?
        .iter()
        .map(app_from_json)
        .collect()
}

/// Writes the index of `apps`, made as `stamp` says, to `path`, whole or not at all.
fn write_index(path: &Path, stamp: Value, apps: &[App]) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let index = json!({
        "stamp": stamp,
        "apps": apps.iter().map(app_to_json).collect::<Vec<_>>(),
    });
    let partial = path.with_extension("json.partial");
    fs::write(&partial, index.to_string())?;
    fs::rename(&partial, path)
}

fn app_to_json(app: &App) -> Value {
    let description: Vec<Value> = app
        .description
        .iter()
        .map(|block| match block {
            Block::Paragraph(text) => json!({ "p": text }),
            Block::Item(text) => json!({ "li": text }),
        })
        .collect();
    let screenshots: Vec<Value> = app
        .screenshots
        .iter()
        .map(|shot| json!([shot.thumbnail, shot.image]))
        .collect();
    json!({
        "id": app.id,
        "name": app.name,
        "summary": app.summary,
        "description": description,
        "categories": app.categories,
        "keywords": app.keywords,
        "licence": app.licence.spdx(),
        "developer": app.developer,
        "verified": app.verified,
        "icon": app.icon,
        "screenshots": screenshots,
        "age": app.age,
        "gamepad": app.gamepad,
        "desktop_id": app.desktop_id,
    })
}

fn app_from_json(value: &Value) -> Option<App> {
    let text = |key: &str| value.get(key)?.as_str().map(str::to_owned);
    let texts = |key: &str| {
        value
            .get(key)?
            .as_array()?
            .iter()
            .map(|text| text.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
    };
    let optional = |key: &str| match value.get(key)? {
        Value::Null => Some(None),
        text => text.as_str().map(|text| Some(text.to_owned())),
    };
    let description = value
        .get("description")?
        .as_array()?
        .iter()
        .map(|block| {
            if let Some(text) = block.get("p") {
                text.as_str().map(|text| Block::Paragraph(text.to_owned()))
            } else {
                block
                    .get("li")?
                    .as_str()
                    .map(|text| Block::Item(text.to_owned()))
            }
        })
        .collect::<Option<Vec<_>>>()?;
    let screenshots = value
        .get("screenshots")?
        .as_array()?
        .iter()
        .map(|shot| {
            Some(Screenshot {
                thumbnail: shot.get(0)?.as_str()?.to_owned(),
                image: shot.get(1)?.as_str()?.to_owned(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let age = match value.get("age")? {
        Value::Null => None,
        age => Some(u8::try_from(age.as_u64()?).ok()?),
    };
    Some(App {
        id: text("id")?,
        kind: Kind::Application,
        name: text("name")?,
        summary: text("summary")?,
        description,
        categories: texts("categories")?,
        keywords: texts("keywords")?,
        licence: Licence::from_spdx(&text("licence")?),
        developer: optional("developer")?,
        verified: value.get("verified")?.as_bool()?,
        icon: optional("icon")?,
        screenshots,
        age,
        gamepad: value.get("gamepad")?.as_bool()?,
        desktop_id: optional("desktop_id")?,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::catalogue::{Kind, lists};

    fn fixture_active() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/catalogue/appstream/flathub/x86_64/active")
    }

    /// A copy of the fixture catalogue, as Flatpak deployed it at `modified`, and where its
    /// index goes.
    fn installation(modified: SystemTime) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let active = root.path().join("active");
        fs::create_dir(&active).unwrap();
        let xml = active.join("appstream.xml");
        fs::copy(fixture_active().join("appstream.xml"), &xml).unwrap();
        fs::File::options()
            .write(true)
            .open(&xml)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let index = root.path().join("cache/catalogue/system.json");
        (root, active, index)
    }

    fn day(n: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000 + n * 86_400)
    }

    fn default_lists() -> Lists {
        lists::parse(lists::DEFAULT).unwrap()
    }

    #[test]
    fn the_index_is_in_the_cache_folder_by_installation() {
        let home = Path::new("/home/pindu");
        assert_eq!(
            index_path(None, home, Installation::System),
            Path::new("/home/pindu/.cache/pindu-launcher/catalogue/system.json")
        );
        assert_eq!(
            index_path(Some(Path::new("/tmp/cache")), home, Installation::User),
            Path::new("/tmp/cache/pindu-launcher/catalogue/user.json")
        );
        assert_eq!(
            index_path(Some(Path::new("cache")), home, Installation::System),
            Path::new("/home/pindu/.cache/pindu-launcher/catalogue/system.json")
        );
    }

    #[test]
    fn the_first_start_parses_the_catalogue_and_later_ones_read_the_index() {
        let (_root, active, index) = installation(day(0));
        let (parsed, origin) = load(&active, &index, &[], &default_lists()).unwrap();
        assert_eq!(origin, Origin::Catalogue);
        assert!(index.is_file());
        let (read, origin) = load(&active, &index, &[], &default_lists()).unwrap();
        assert_eq!(origin, Origin::Index);
        assert_eq!(read, parsed);
    }

    #[test]
    fn a_new_catalogue_is_parsed_again() {
        let (_root, active, index) = installation(day(0));
        load(&active, &index, &[], &default_lists()).unwrap();
        fs::File::options()
            .write(true)
            .open(active.join("appstream.xml"))
            .unwrap()
            .set_modified(day(1))
            .unwrap();
        let (_, origin) = load(&active, &index, &[], &default_lists()).unwrap();
        assert_eq!(origin, Origin::Catalogue);
    }

    #[test]
    fn a_catalogue_of_another_size_is_parsed_again_though_its_time_is_the_same() {
        let (_root, active, index) = installation(day(0));
        load(&active, &index, &[], &default_lists()).unwrap();
        let xml = active.join("appstream.xml");
        let text = fs::read_to_string(&xml).unwrap();
        fs::write(
            &xml,
            text.replacen("SuperTuxKart</name>", "SuperTuxKart 2</name>", 1),
        )
        .unwrap();
        fs::File::options()
            .write(true)
            .open(&xml)
            .unwrap()
            .set_modified(day(0))
            .unwrap();
        let (apps, origin) = load(&active, &index, &[], &default_lists()).unwrap();
        assert_eq!(origin, Origin::Catalogue);
        assert!(apps.iter().any(|app| app.name == "SuperTuxKart 2"));
    }

    #[test]
    fn other_languages_or_lists_parse_it_again() {
        let (_root, active, index) = installation(day(0));
        load(&active, &index, &[], &default_lists()).unwrap();
        let german = ["de".to_owned()];
        let (apps, origin) = load(&active, &index, &german, &default_lists()).unwrap();
        assert_eq!(origin, Origin::Catalogue);
        let kart = apps
            .iter()
            .find(|app| app.id == "net.supertuxkart.SuperTuxKart")
            .unwrap();
        assert_eq!(kart.summary, "Ein Open-Source 3D-Kart-Rennspiel");
        let lists = Lists {
            extras: vec!["org.gnome.Calculator".to_owned()],
            ..default_lists()
        };
        let (apps, origin) = load(&active, &index, &german, &lists).unwrap();
        assert_eq!(origin, Origin::Catalogue);
        assert!(apps.iter().any(|app| app.id == "org.gnome.Calculator"));
    }

    #[test]
    fn the_index_holds_applications_in_the_game_categories_or_among_the_extras() {
        let (_root, active, index) = installation(day(0));
        load(&active, &index, &[], &default_lists()).unwrap();
        let (apps, _) = load(&active, &index, &[], &default_lists()).unwrap();
        let has = |id: &str| apps.iter().any(|app| app.id == id);
        assert!(has("net.supertuxkart.SuperTuxKart"));
        assert!(has("tv.kodi.Kodi"));
        assert!(has("com.steamgriddb.SGDBoop"));
        assert!(!has("org.gnome.Calculator"));
        assert!(!has("org.freedesktop.Platform"));
        assert!(!has("io.github.theforceengine.tfe.Debug"));
        assert!(apps.iter().all(|app| app.kind == Kind::Application));
    }

    #[test]
    fn a_broken_index_is_rebuilt_from_the_catalogue() {
        let (_root, active, index) = installation(day(0));
        load(&active, &index, &[], &default_lists()).unwrap();
        fs::write(&index, "{\"version\": 1, \"apps\": [").unwrap();
        let (apps, origin) = load(&active, &index, &[], &default_lists()).unwrap();
        assert_eq!(origin, Origin::Catalogue);
        assert!(!apps.is_empty());
    }

    #[test]
    fn no_catalogue_is_missing_and_a_broken_one_says_why() {
        let (_root, active, index) = installation(day(0));
        fs::write(active.join("appstream.xml"), "<components><component>").unwrap();
        assert!(matches!(
            load(&active, &index, &[], &default_lists()),
            Err(CatalogueError::Broken(_))
        ));
        fs::remove_file(active.join("appstream.xml")).unwrap();
        assert_eq!(
            load(&active, &index, &[], &default_lists()),
            Err(CatalogueError::Missing)
        );
    }

    #[test]
    fn an_unwritable_index_still_gives_the_catalogue() {
        let (root, active, _) = installation(day(0));
        let blocked = root.path().join("file");
        fs::write(&blocked, "").unwrap();
        let (apps, origin) =
            load(&active, &blocked.join("system.json"), &[], &default_lists()).unwrap();
        assert_eq!(origin, Origin::Catalogue);
        assert!(!apps.is_empty());
    }
}
