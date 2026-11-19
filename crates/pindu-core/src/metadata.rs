//! A game's metadata kept by origin.
//!
//! Developer, genres and description can each come from the game's source, an online
//! provider or the user. The store keeps every origin's value with when it was set, and a
//! sync writes only the source's; what a game shows is [`resolve`]d from them, the user's
//! value first, then a provider's, then the source's.

use crate::model::{Game, MetadataField, MetadataOrigin, Timestamp};

/// A game's developer, genres and description: the values one origin gives, or, resolved,
/// the ones the game shows. An empty field has no value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    /// Who made the game.
    pub developer: Option<String>,
    /// The game's genres.
    pub genres: Vec<String>,
    /// A description of the game.
    pub description: Option<String>,
}

impl Metadata {
    /// The metadata a game carries.
    pub fn of(game: &Game) -> Self {
        Self {
            developer: game.developer.clone(),
            genres: game.genres.clone(),
            description: game.description.clone(),
        }
    }

    /// Puts this metadata on a game, replacing what it carried.
    pub fn apply_to(self, game: &mut Game) {
        game.developer = self.developer;
        game.genres = self.genres;
        game.description = self.description;
    }

    /// The fields that have a value, each with it.
    pub fn values(&self) -> Vec<FieldValue> {
        let mut values = Vec::new();
        values.extend(self.developer.clone().map(FieldValue::Developer));
        if !self.genres.is_empty() {
            values.push(FieldValue::Genres(self.genres.clone()));
        }
        values.extend(self.description.clone().map(FieldValue::Description));
        values
    }

    fn set(&mut self, value: FieldValue) {
        match value {
            FieldValue::Developer(developer) => self.developer = Some(developer),
            FieldValue::Genres(genres) => self.genres = genres,
            FieldValue::Description(description) => self.description = Some(description),
        }
    }
}

/// One field's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    /// Who made the game.
    Developer(String),
    /// The game's genres.
    Genres(Vec<String>),
    /// A description of the game.
    Description(String),
}

impl FieldValue {
    /// The field this is a value of.
    pub fn field(&self) -> MetadataField {
        match self {
            Self::Developer(_) => MetadataField::Developer,
            Self::Genres(_) => MetadataField::Genres,
            Self::Description(_) => MetadataField::Description,
        }
    }
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

/// The value a game shows for `field`: the one from the origin that wins, the user's
/// before a provider's before the source's. `None` when no origin has one.
pub fn shown(values: &[MetadataValue], field: MetadataField) -> Option<&MetadataValue> {
    values
        .iter()
        .filter(|value| value.value.field() == field)
        .max_by_key(|value| value.origin)
}

/// The metadata a game shows, each field [`shown`] from its values.
pub fn resolve(values: &[MetadataValue]) -> Metadata {
    let mut metadata = Metadata::default();
    for field in MetadataField::ALL {
        if let Some(shown) = shown(values, *field) {
            metadata.set(shown.value.clone());
        }
    }
    metadata
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
        match field {
            MetadataField::Developer => FieldValue::Developer(text),
            MetadataField::Genres => FieldValue::Genres(vec![text]),
            MetadataField::Description => FieldValue::Description(text),
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
            let overridden = values
                .iter()
                .any(|v| v.origin != MetadataOrigin::Source && v.value.field() == field);
            if overridden {
                assert_eq!(after, before, "seed {seed}");
            } else {
                let mut expected = before.clone();
                expected.set(value_of(field, "synced".into()));
                assert_eq!(after, expected, "seed {seed}");
            }
        }
    }

    #[test]
    fn metadata_without_values_resolves_to_none() {
        assert_eq!(resolve(&[]), Metadata::default());
    }

    #[test]
    fn empty_fields_have_no_values() {
        let metadata = Metadata {
            developer: Some("Valve".into()),
            genres: Vec::new(),
            description: None,
        };

        assert_eq!(
            metadata.values(),
            vec![FieldValue::Developer("Valve".into())]
        );
    }
}
