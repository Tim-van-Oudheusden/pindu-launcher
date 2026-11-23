//! The library's categories: a closed set based on Steam's genres, and the tables that map
//! each provider's genres onto it, so a provider adds games to the set's categories, never
//! new categories (#759). The code and the stored data call them genres, as the launchers
//! do; fullscreen Home and the user read "category".
//!
//! The tables live in one data file, `category/mappings.json`, keyed by provider: Steam
//! by genre ID, Wikidata by the genre's item ID (QID), never its label, so the result does
//! not depend on the language labels come in, and Heroic by the store's genre name,
//! ignoring case. An ID can map to several categories, or to none, and an ID a table does
//! not list maps to none.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde_json::Value as Json;

/// A provider whose genres map onto [`CATEGORIES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// Steam's `appinfo.vdf` genre IDs.
    Steam,
    /// Wikidata's genre (`P136`) items, by QID.
    Wikidata,
    /// Heroic's store genres (`extra.genres`), by name.
    Heroic,
}

impl Provider {
    /// Every provider, in the order they are declared, so `provider as usize` indexes [`Tables`].
    const ALL: [Self; 3] = [Self::Steam, Self::Wikidata, Self::Heroic];

    /// The provider's key in the data file.
    fn key(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Wikidata => "wikidata",
            Self::Heroic => "heroic",
        }
    }
}

/// Every category, in the order Pindu lists them: Steam's ten genres, then the ones added
/// for games Steam has no genre for.
pub const CATEGORIES: [&str; 14] = [
    "Action",
    "Strategy",
    "RPG",
    "Casual",
    "Racing",
    "Sports",
    "Indie",
    "Adventure",
    "Simulation",
    "Massively Multiplayer",
    "Shooter",
    "Platformer",
    "Puzzle",
    "Fighting",
];

/// Whether `genre` is one of [`CATEGORIES`], written as the set writes it.
pub fn is_category(genre: &str) -> bool {
    CATEGORIES.contains(&genre)
}

/// The categories `provider`'s genre `ids` map to, each once, in [`CATEGORIES`]' order.
pub fn categories<I>(provider: Provider, ids: I) -> Vec<String>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let table = &TABLES.0[provider as usize];
    let mut found = [false; CATEGORIES.len()];
    for id in ids {
        for &category in table.get(&key(id.as_ref())).into_iter().flatten() {
            found[category] = true;
        }
    }
    CATEGORIES
        .iter()
        .zip(found)
        .filter(|(_, found)| *found)
        .map(|(category, _)| (*category).to_owned())
        .collect()
}

/// Each provider's table, in [`Provider::ALL`]'s order: the indexes into [`CATEGORIES`]
/// each genre ID ([`key`]) maps to.
struct Tables([HashMap<String, Vec<usize>>; Provider::ALL.len()]);

static TABLES: LazyLock<Tables> = LazyLock::new(|| {
    Tables::parse(include_str!("category/mappings.json"))
        .unwrap_or_else(|error| panic!("category/mappings.json: {error}"))
});

impl Tables {
    /// The tables in the data file's `text`; an error for an entry without an ID, a
    /// category not in [`CATEGORIES`] or an ID listed twice.
    fn parse(text: &str) -> Result<Self, String> {
        let json: Json = serde_json::from_str(text).map_err(|error| error.to_string())?;
        let mut tables: [HashMap<String, Vec<usize>>; Provider::ALL.len()] = Default::default();
        for (provider, table) in Provider::ALL.into_iter().zip(&mut tables) {
            let entries = json[provider.key()]
                .as_array()
                .ok_or_else(|| format!("no {} table", provider.key()))?;
            for entry in entries {
                let id = entry["id"]
                    .as_str()
                    .ok_or_else(|| format!("a {} entry has no id", provider.key()))?;
                let categories = entry["categories"]
                    .as_array()
                    .ok_or_else(|| format!("{id} has no categories"))?
                    .iter()
                    .map(|category| {
                        category
                            .as_str()
                            .and_then(|name| CATEGORIES.iter().position(|c| *c == name))
                            .ok_or_else(|| format!("{id}: {category} is not a category"))
                    })
                    .collect::<Result<_, _>>()?;
                if table.insert(key(id), categories).is_some() {
                    return Err(format!("{id} is listed twice"));
                }
            }
        }
        Ok(Self(tables))
    }
}

/// A genre ID as the tables keep it: Heroic's names are matched ignoring case, and
/// Steam's and Wikidata's IDs do not change by it.
fn key(id: &str) -> String {
    id.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wikidata_genre_maps_by_its_qid() {
        // 4X.
        assert_eq!(categories(Provider::Wikidata, ["Q603555"]), ["Strategy"]);
    }

    #[test]
    fn a_wikidata_genre_can_map_to_several_categories() {
        // action-adventure game.
        assert_eq!(
            categories(Provider::Wikidata, ["Q343568"]),
            ["Action", "Adventure"]
        );
    }

    #[test]
    fn an_unmapped_genre_adds_no_category() {
        // Science fiction video game, a theme; then an ID no table lists.
        assert!(categories(Provider::Wikidata, ["Q27670585", "Q1"]).is_empty());
        assert!(categories(Provider::Steam, ["37", "70", "999"]).is_empty());
    }

    #[test]
    fn genres_map_to_each_category_once_in_the_sets_order() {
        // Stellaris: real-time strategy, 4X, grand strategy wargame, science fiction.
        assert_eq!(
            categories(
                Provider::Wikidata,
                ["Q208189", "Q603555", "Q3139142", "Q27670585"]
            ),
            ["Strategy"]
        );
        // First-person shooter, then action-adventure game.
        assert_eq!(
            categories(Provider::Wikidata, ["Q185029", "Q343568"]),
            ["Action", "Adventure", "Shooter"]
        );
    }

    #[test]
    fn steams_genre_ids_map_to_steams_names() {
        let ids = ["1", "2", "3", "4", "9", "18", "23", "25", "28", "29"];
        for (id, name) in ids.into_iter().zip(&CATEGORIES[..10]) {
            assert_eq!(categories(Provider::Steam, [id]), [*name], "{id}");
        }
    }

    #[test]
    fn heroics_store_genres_map_through_aliases_ignoring_case() {
        assert_eq!(
            categories(
                Provider::Heroic,
                ["Role-playing (RPG)", "real time strategy (rts)"]
            ),
            ["Strategy", "RPG"]
        );
        assert_eq!(
            categories(Provider::Heroic, ["Platform"]),
            ["Action", "Platformer"]
        );
        assert!(categories(Provider::Heroic, ["Open World"]).is_empty());
    }

    #[test]
    fn the_data_file_names_only_categories_of_the_set_and_each_id_once() {
        let table =
            |entries: &str| format!(r#"{{"steam": [], "heroic": [], "wikidata": [{entries}]}}"#);
        assert!(Tables::parse(include_str!("category/mappings.json")).is_ok());
        assert!(Tables::parse(&table(r#"{"id": "Q1", "categories": ["Strategy"]}"#)).is_ok());
        assert!(Tables::parse(&table(r#"{"id": "Q1", "categories": ["4X"]}"#)).is_err());
        assert!(
            Tables::parse(&table(
                r#"{"id": "Q1", "categories": []}, {"id": "Q1", "categories": []}"#
            ))
            .is_err()
        );
        assert!(Tables::parse(&table(r#"{"categories": []}"#)).is_err());
        assert!(Tables::parse(r#"{"steam": []}"#).is_err());
    }

    #[test]
    fn every_category_is_one_a_provider_can_give() {
        for category in CATEGORIES {
            assert!(is_category(category));
            let given = TABLES
                .0
                .iter()
                .flat_map(HashMap::values)
                .flatten()
                .any(|&index| CATEGORIES[index] == category);
            assert!(given, "{category}");
        }
        assert!(!is_category("4X"));
        assert!(!is_category("strategy"));
    }
}
