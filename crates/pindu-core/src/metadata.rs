//! A game's metadata kept by origin.
//!
//! A custom title, developer, genres, description, release year, controller support and
//! Steam Deck rating are kept separately from the game's source record. The store
//! keeps every origin's value with when it was set, and a sync writes only the source's;
//! what a game shows is [`resolve`]d from them, the user's value first, then a provider's,
//! then the source's. The user's values are written by [`edit_by_user`] from what they type
//! over the details a game shows.

use crate::game_groups;
use crate::model::{
    ControllerSupport, DeckCompatibility, Game, GameId, MetadataField, MetadataOrigin, Timestamp,
};
use crate::store::{self, Store};

/// A game's custom title, developer, genres, description, release year, controller
/// support and Steam Deck rating: the values one origin gives, or, resolved, the ones
/// the game shows. An empty field has no value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    /// A title to display instead of the one in the source record.
    pub title: Option<String>,
    /// Who made the game.
    pub developer: Option<String>,
    /// The game's genres.
    pub genres: Vec<String>,
    /// A description of the game.
    pub description: Option<String>,
    /// The year the game was released.
    pub release_year: Option<i32>,
    /// How well the game supports a controller.
    pub controller_support: Option<ControllerSupport>,
    /// Steam's Steam Deck rating.
    pub deck_compatibility: Option<DeckCompatibility>,
}

impl Metadata {
    /// The metadata a game carries.
    pub fn of(game: &Game) -> Self {
        Self {
            title: game.custom_title.clone(),
            developer: game.developer.clone(),
            genres: game.genres.clone(),
            description: game.description.clone(),
            release_year: game.release_year,
            controller_support: game.controller_support,
            deck_compatibility: game.deck_compatibility,
        }
    }

    /// Puts this metadata on a game, replacing what it carried.
    pub fn apply_to(self, game: &mut Game) {
        game.custom_title = self.title;
        game.developer = self.developer;
        game.genres = self.genres;
        game.description = self.description;
        game.release_year = self.release_year;
        game.controller_support = self.controller_support;
        game.deck_compatibility = self.deck_compatibility;
    }

    /// The details as the user typed them: each trimmed, an empty one without a value,
    /// and `genres` split at commas, each genre once.
    pub fn typed(developer: &str, genres: &str, description: &str) -> Self {
        let text = |text: &str| Some(text.trim().to_owned()).filter(|text| !text.is_empty());
        let mut split = Vec::new();
        for genre in genres.split(',').filter_map(text) {
            if !split.contains(&genre) {
                split.push(genre);
            }
        }
        Self {
            developer: text(developer),
            genres: split,
            description: text(description),
            ..Self::default()
        }
    }

    /// A release year as the user typed it: four digits, trimmed, not starting with 0;
    /// `None` when empty.
    ///
    /// # Errors
    ///
    /// [`NotAYear`] for other text, which the user has to correct rather than lose.
    pub fn typed_year(text: &str) -> Result<Option<i32>, NotAYear> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let digits = text.len() == 4 && text.bytes().all(|byte| byte.is_ascii_digit());
        match text.parse() {
            Ok(year @ 1000..) if digits => Ok(Some(year)),
            _ => Err(NotAYear),
        }
    }

    /// The fields that have a value, each with it.
    pub fn values(&self) -> Vec<FieldValue> {
        let mut values = Vec::new();
        values.extend(self.title.clone().map(FieldValue::Title));
        values.extend(self.developer.clone().map(FieldValue::Developer));
        if !self.genres.is_empty() {
            values.push(FieldValue::Genres(self.genres.clone()));
        }
        values.extend(self.description.clone().map(FieldValue::Description));
        values.extend(self.release_year.map(FieldValue::ReleaseYear));
        values.extend(self.controller_support.map(FieldValue::ControllerSupport));
        values.extend(self.deck_compatibility.map(FieldValue::DeckCompatibility));
        values
    }

    /// The value of one field, if it has one.
    fn value(&self, field: MetadataField) -> Option<FieldValue> {
        self.values()
            .into_iter()
            .find(|value| value.field() == field)
    }

    fn set(&mut self, value: FieldValue) {
        match value {
            FieldValue::Title(title) => self.title = Some(title),
            FieldValue::Developer(developer) => self.developer = Some(developer),
            FieldValue::Genres(genres) => self.genres = genres,
            FieldValue::Description(description) => self.description = Some(description),
            FieldValue::ReleaseYear(year) => self.release_year = Some(year),
            FieldValue::ControllerSupport(support) => self.controller_support = Some(support),
            FieldValue::DeckCompatibility(deck) => self.deck_compatibility = Some(deck),
        }
    }
}

/// Text the user typed as a release year that is not one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAYear;

/// One field's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    /// A custom title for display.
    Title(String),
    /// Who made the game.
    Developer(String),
    /// The game's genres.
    Genres(Vec<String>),
    /// A description of the game.
    Description(String),
    /// The year the game was released.
    ReleaseYear(i32),
    /// How well the game supports a controller.
    ControllerSupport(ControllerSupport),
    /// Steam's Steam Deck rating.
    DeckCompatibility(DeckCompatibility),
}

impl FieldValue {
    /// The field this is a value of.
    pub fn field(&self) -> MetadataField {
        match self {
            Self::Title(_) => MetadataField::Title,
            Self::Developer(_) => MetadataField::Developer,
            Self::Genres(_) => MetadataField::Genres,
            Self::Description(_) => MetadataField::Description,
            Self::ReleaseYear(_) => MetadataField::ReleaseYear,
            Self::ControllerSupport(_) => MetadataField::ControllerSupport,
            Self::DeckCompatibility(_) => MetadataField::DeckCompatibility,
        }
    }
}

/// A Steam Deck rating as the store and backups keep it:
/// `{"rating": "verified", "tested_at": 1700000000}`.
pub(crate) fn deck_json(deck: &DeckCompatibility) -> serde_json::Value {
    serde_json::json!({
        "rating": deck.rating.as_str(),
        "tested_at": deck.tested_at,
    })
}

/// A Steam Deck rating from [`deck_json`]'s form; `None` when it names no rating this
/// version knows.
pub(crate) fn deck_from_json(json: &serde_json::Value) -> Option<DeckCompatibility> {
    Some(DeckCompatibility {
        rating: json["rating"].as_str()?.parse().ok()?,
        tested_at: json["tested_at"].as_i64(),
    })
}

/// A stored value: one field of a game from one origin, and when that origin set it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataValue {
    /// The value, which names its field.
    pub value: FieldValue,
    /// Where the value came from.
    pub origin: MetadataOrigin,
    /// When the origin set this value.
    pub updated_at: Timestamp,
}

/// The value that wins for `field`: the user's before a provider's before the source's.
/// `None` when no origin has one. For genres a provider's adds to the source's rather than
/// replacing it ([`resolve`]); this is still the value that ranks highest.
pub fn shown(values: &[MetadataValue], field: MetadataField) -> Option<&MetadataValue> {
    values
        .iter()
        .filter(|value| value.value.field() == field)
        .max_by_key(|value| value.origin)
}

/// The metadata a game shows, each field [`shown`] from its values, except genres: the
/// user's when they set some, otherwise the source's with a provider's added (#759), each
/// once, ignoring case. A provider's genres are already Pindu's categories
/// ([`crate::category`]), so they fill gaps in the source's rather than replace them.
pub fn resolve(values: &[MetadataValue]) -> Metadata {
    let mut metadata = Metadata::default();
    for field in MetadataField::ALL {
        if let Some(shown) = shown(values, *field) {
            metadata.set(shown.value.clone());
        }
    }
    if !values.iter().any(|value| {
        value.origin == MetadataOrigin::User && value.value.field() == MetadataField::Genres
    }) {
        metadata.genres = Vec::new();
        for origin in [MetadataOrigin::Source, MetadataOrigin::Provider] {
            for value in values.iter().filter(|value| value.origin == origin) {
                if let FieldValue::Genres(genres) = &value.value {
                    for genre in genres {
                        if !metadata
                            .genres
                            .iter()
                            .any(|g| g.to_lowercase() == genre.to_lowercase())
                        {
                            metadata.genres.push(genre.clone());
                        }
                    }
                }
            }
        }
    }
    metadata
}

/// Saves what the user wrote over the details `game` showed (`shown`) as the user's
/// values, for every copy of its group. A field left as shown, such as one the user cannot
/// edit there, keeps each copy's own user value, if any; a field cleared drops the user's
/// value, so the game falls back to a provider's, then the source's; and a value the game
/// would fall back to anyway is not kept as the user's, so a sync that changes it still
/// shows.
pub fn edit_by_user(
    store: &Store,
    game: &GameId,
    shown: &Metadata,
    edited: &Metadata,
    now: Timestamp,
) -> store::Result<()> {
    store.in_transaction(|store| {
        for copy in game_groups::copies_of(store, game)? {
            let values = store.metadata_values(&copy)?;
            let user = user_values(&values, shown, edited);
            store.set_metadata(&copy, MetadataOrigin::User, &user, now)?;
        }
        Ok(())
    })
}

/// The user's values once they edited `shown` into `edited`, given a game's stored
/// `values`; see [`edit_by_user`].
fn user_values(values: &[MetadataValue], shown: &Metadata, edited: &Metadata) -> Metadata {
    let (user, others): (Vec<_>, Vec<_>) = values
        .iter()
        .cloned()
        .partition(|value| value.origin == MetadataOrigin::User);
    let (user, fallback) = (resolve(&user), resolve(&others));
    let mut kept = Metadata::default();
    for &field in MetadataField::ALL {
        let edited = edited.value(field);
        let value = if edited == shown.value(field) {
            user.value(field)
        } else if edited == fallback.value(field) {
            None
        } else {
            edited
        };
        if let Some(value) = value {
            kept.set(value);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    // Property-style tests: random sets of values from fixed seeds, checked against the
    // rule that the highest origin with a value wins.

    /// A small xorshift generator, so a failing seed replays exactly.
    struct Rng(u64);

    impl Rng {
        fn new(seed: u64) -> Self {
            Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
        }

        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }

        fn shuffle<T>(&mut self, items: &mut [T]) {
            for index in (1..items.len()).rev() {
                items.swap(index, self.below(index + 1));
            }
        }
    }

    fn value_of(field: MetadataField, text: String) -> FieldValue {
        // Fields that are not text take a value derived from it, so equal text gives an
        // equal value.
        let number = text.bytes().map(i32::from).sum::<i32>();
        match field {
            MetadataField::Title => FieldValue::Title(text),
            MetadataField::Developer => FieldValue::Developer(text),
            MetadataField::Genres => FieldValue::Genres(vec![text]),
            MetadataField::Description => FieldValue::Description(text),
            MetadataField::ReleaseYear => FieldValue::ReleaseYear(1970 + number),
            MetadataField::ControllerSupport => FieldValue::ControllerSupport(if number % 2 == 0 {
                ControllerSupport::Full
            } else {
                ControllerSupport::Partial
            }),
            // The text's bytes summed tell the values apart well enough.
            MetadataField::DeckCompatibility => FieldValue::DeckCompatibility(DeckCompatibility {
                rating: crate::model::DeckRating::Verified,
                tested_at: Some(text.bytes().map(Timestamp::from).sum()),
            }),
        }
    }

    /// Each field from each origin has a value about half the time, as the store allows
    /// at most one per field and origin; the values come in a random order.
    fn random_values(random: &mut Rng) -> Vec<MetadataValue> {
        let mut values = Vec::new();
        for &field in MetadataField::ALL {
            for &origin in MetadataOrigin::ALL {
                if random.below(2) == 0 {
                    values.push(MetadataValue {
                        value: value_of(field, format!("{origin} {}", random.below(100))),
                        origin,
                        updated_at: random.below(1_000) as Timestamp,
                    });
                }
            }
        }
        random.shuffle(&mut values);
        values
    }

    fn highest(values: &[MetadataValue], field: MetadataField) -> Option<&MetadataValue> {
        [
            MetadataOrigin::User,
            MetadataOrigin::Provider,
            MetadataOrigin::Source,
        ]
        .into_iter()
        .find_map(|origin| {
            values
                .iter()
                .find(|value| value.origin == origin && value.value.field() == field)
        })
    }

    #[test]
    fn the_user_wins_then_a_provider_then_the_source() {
        for seed in 0..500 {
            let values = random_values(&mut Rng::new(seed));
            for &field in MetadataField::ALL {
                assert_eq!(
                    shown(&values, field),
                    highest(&values, field),
                    "seed {seed}"
                );
            }
        }
    }

    #[test]
    fn the_order_values_come_in_does_not_matter() {
        for seed in 0..500 {
            let mut random = Rng::new(seed);
            let mut values = random_values(&mut random);
            let resolved = resolve(&values);
            random.shuffle(&mut values);
            assert_eq!(resolve(&values), resolved, "seed {seed}");
        }
    }

    #[test]
    fn a_new_source_value_changes_only_fields_no_other_origin_sets() {
        for seed in 0..500 {
            let mut random = Rng::new(seed);
            let mut values = random_values(&mut random);
            let before = resolve(&values);
            let field = MetadataField::ALL[random.below(MetadataField::ALL.len())];
            values.retain(|v| !(v.origin == MetadataOrigin::Source && v.value.field() == field));
            values.push(MetadataValue {
                value: value_of(field, "synced".into()),
                origin: MetadataOrigin::Source,
                updated_at: 2_000,
            });
            let after = resolve(&values);
            // A provider's genres add to the source's (#759), so only the user's override
            // them; every other field is overridden by any origin above the source.
            let overrides = |origin| {
                origin == MetadataOrigin::User
                    || (origin == MetadataOrigin::Provider && field != MetadataField::Genres)
            };
            let overridden = values
                .iter()
                .any(|v| overrides(v.origin) && v.value.field() == field);
            if overridden {
                assert_eq!(after, before, "seed {seed}");
            } else {
                let mut expected = before.clone();
                expected.set(value_of(field, "synced".into()));
                if field == MetadataField::Genres {
                    let provider = values.iter().filter_map(|v| match &v.value {
                        FieldValue::Genres(genres) if v.origin == MetadataOrigin::Provider => {
                            Some(genres.clone())
                        }
                        _ => None,
                    });
                    expected.genres.extend(provider.flatten());
                }
                assert_eq!(after, expected, "seed {seed}");
            }
        }
    }

    #[test]
    fn metadata_without_values_resolves_to_none() {
        assert_eq!(resolve(&[]), Metadata::default());
    }

    fn genres(origin: MetadataOrigin, genres: &[&str]) -> MetadataValue {
        MetadataValue {
            value: FieldValue::Genres(genres.iter().map(|&genre| genre.to_owned()).collect()),
            origin,
            updated_at: 0,
        }
    }

    #[test]
    fn a_providers_genres_add_to_the_sources_and_the_users_replace_both() {
        let source = genres(MetadataOrigin::Source, &["Strategy", "Indie"]);
        let provider = genres(MetadataOrigin::Provider, &["Action", "strategy"]);
        let user = genres(MetadataOrigin::User, &["4X"]);

        assert_eq!(
            resolve(&[provider.clone(), source.clone()]).genres,
            ["Strategy", "Indie", "Action"],
            "the source's first, then the provider's it lacks, ignoring case"
        );
        assert_eq!(
            resolve(std::slice::from_ref(&provider)).genres,
            ["Action", "strategy"]
        );
        assert_eq!(
            resolve(std::slice::from_ref(&source)).genres,
            ["Strategy", "Indie"]
        );
        assert_eq!(resolve(&[source, provider, user]).genres, ["4X"]);
    }

    #[test]
    fn empty_fields_have_no_values() {
        let metadata = Metadata {
            developer: Some("Valve".into()),
            genres: Vec::new(),
            description: None,
            ..Metadata::default()
        };

        assert_eq!(
            metadata.values(),
            vec![FieldValue::Developer("Valve".into())]
        );
    }

    #[test]
    fn typed_details_are_trimmed_with_genres_split_at_commas() {
        assert_eq!(
            Metadata::typed("  Valve ", "Strategy, , MOBA,Strategy ,", "\n Every day.\n"),
            Metadata {
                developer: Some("Valve".into()),
                genres: vec!["Strategy".into(), "MOBA".into()],
                description: Some("Every day.".into()),
                ..Default::default()
            }
        );
        assert_eq!(Metadata::typed(" ", " , ", "\n"), Metadata::default());
    }

    #[test]
    fn a_typed_year_is_four_digits_or_nothing() {
        assert_eq!(Metadata::typed_year(" 2004\n"), Ok(Some(2004)));
        assert_eq!(Metadata::typed_year(" "), Ok(None));
        for text in [
            "04", "20040", "+2004", "-2004", "2004.0", "MMIV", "2004 AD", "0000",
        ] {
            assert_eq!(Metadata::typed_year(text), Err(NotAYear), "{text:?}");
        }
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    use crate::model::{LaunchKind, Source};

    fn store_with(native_ids: &[&str]) -> (tempfile::TempDir, Store, Vec<GameId>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        let ids = native_ids
            .iter()
            .map(|native_id| {
                let id = GameId::new(Source::Steam, native_id);
                let game = Game::new(id.clone(), "Dota 2", LaunchKind::StoreUri, 0);
                store.insert_game(&game).unwrap();
                id
            })
            .collect();
        (dir, store, ids)
    }

    fn developer(developer: &str) -> Metadata {
        Metadata {
            developer: Some(developer.into()),
            ..Default::default()
        }
    }

    fn shown_metadata(store: &Store, id: &GameId) -> Metadata {
        Metadata::of(&store.game(id).unwrap().unwrap())
    }

    /// The user edits what `id` shows into `edited`.
    fn edit(store: &Store, id: &GameId, edited: &Metadata, now: Timestamp) {
        let shown = shown_metadata(store, id);
        edit_by_user(store, id, &shown, edited, now).unwrap();
    }

    fn sync(store: &Store, id: &GameId, metadata: &Metadata, now: Timestamp) {
        store
            .set_metadata(id, MetadataOrigin::Source, metadata, now)
            .unwrap();
    }

    #[test]
    fn clearing_a_users_value_falls_back_to_the_providers_then_the_sources() {
        let (_dir, store, ids) = store_with(&["570"]);
        let id = &ids[0];
        sync(&store, id, &developer("Valve"), 10);
        store
            .set_metadata(id, MetadataOrigin::Provider, &developer("Valve Corp"), 10)
            .unwrap();
        edit(&store, id, &developer("Me"), 20);
        assert_eq!(shown_metadata(&store, id), developer("Me"));

        edit(&store, id, &Metadata::default(), 30);
        assert_eq!(shown_metadata(&store, id), developer("Valve Corp"));

        store
            .set_metadata(id, MetadataOrigin::Provider, &Metadata::default(), 40)
            .unwrap();
        assert_eq!(shown_metadata(&store, id), developer("Valve"));
    }

    #[test]
    fn a_users_release_year_survives_a_sync_until_cleared() {
        let (_dir, store, ids) = store_with(&["570"]);
        let id = &ids[0];
        let year = |year| Metadata {
            release_year: Some(year),
            ..Default::default()
        };
        sync(&store, id, &year(2013), 10);
        edit(&store, id, &year(2011), 20);
        sync(&store, id, &year(2014), 30);
        assert_eq!(shown_metadata(&store, id), year(2011));

        edit(&store, id, &Metadata::default(), 40);
        assert_eq!(shown_metadata(&store, id), year(2014));
    }

    #[test]
    fn the_users_value_survives_a_sync_and_fields_left_as_shown_follow_it() {
        let (_dir, store, ids) = store_with(&["570"]);
        let id = &ids[0];
        sync(&store, id, &developer("Valve"), 10);
        edit(
            &store,
            id,
            &Metadata {
                developer: Some("Valve".into()),
                genres: vec!["Strategy".into()],
                ..Default::default()
            },
            20,
        );

        let synced = Metadata {
            developer: Some("Valve Corporation".into()),
            genres: vec!["Action".into()],
            description: Some("Every day".into()),
            ..Default::default()
        };
        sync(&store, id, &synced, 30);

        assert_eq!(
            shown_metadata(&store, id),
            Metadata {
                genres: vec!["Strategy".into()],
                ..synced
            }
        );
    }

    #[test]
    fn a_field_left_as_shown_keeps_the_users_value() {
        let (_dir, store, ids) = store_with(&["570"]);
        let id = &ids[0];
        sync(&store, id, &developer("Valve"), 10);
        edit(&store, id, &developer("Me"), 20);

        let mut edited = developer("Me");
        edited.description = Some("Mine".into());
        edit(&store, id, &edited, 30);
        sync(&store, id, &developer("Valve Corporation"), 40);

        assert_eq!(shown_metadata(&store, id), edited);
        let user_developer = store
            .metadata_values(id)
            .unwrap()
            .into_iter()
            .find(|value| value.origin == MetadataOrigin::User)
            .map(|value| (value.value, value.updated_at));
        assert_eq!(
            user_developer,
            Some((FieldValue::Description("Mine".into()), 30))
        );
    }

    #[test]
    fn typing_what_the_game_falls_back_to_keeps_no_users_value() {
        let (_dir, store, ids) = store_with(&["570"]);
        let id = &ids[0];
        sync(&store, id, &developer("Valve"), 10);
        edit(&store, id, &developer("Me"), 20);

        edit(&store, id, &developer("Valve"), 30);
        sync(&store, id, &developer("Valve Corporation"), 40);

        assert_eq!(shown_metadata(&store, id), developer("Valve Corporation"));
    }

    #[test]
    fn a_field_the_edit_leaves_out_keeps_the_users_value() {
        let (_dir, store, ids) = store_with(&["570"]);
        let id = &ids[0];
        let deck = Metadata {
            deck_compatibility: Some(DeckCompatibility {
                rating: crate::model::DeckRating::Verified,
                tested_at: None,
            }),
            ..Default::default()
        };
        store
            .set_metadata(id, MetadataOrigin::User, &deck, 10)
            .unwrap();

        // The desktop dialog edits the text fields only.
        let shown = Metadata::typed("", "", "");
        let edited = Metadata::typed("Valve", "", "");
        edit_by_user(&store, id, &shown, &edited, 20).unwrap();

        assert_eq!(
            shown_metadata(&store, id),
            Metadata {
                developer: Some("Valve".into()),
                ..deck
            }
        );
    }

    #[test]
    fn an_edit_of_a_group_is_saved_for_every_copy() {
        let (_dir, store, ids) = store_with(&["570", "571"]);
        sync(&store, &ids[0], &developer("Valve"), 10);
        sync(&store, &ids[1], &developer("Valve Corporation"), 10);
        store.merge_games(&ids).unwrap();

        let mut edited = shown_metadata(&store, &ids[0]);
        edited.genres = vec!["Strategy".into()];
        edit(&store, &ids[0], &edited, 20);

        for (id, developer) in ids.iter().zip(["Valve", "Valve Corporation"]) {
            let shown = shown_metadata(&store, id);
            assert_eq!(shown.developer.as_deref(), Some(developer));
            assert_eq!(shown.genres, ["Strategy"]);
        }
    }
}
