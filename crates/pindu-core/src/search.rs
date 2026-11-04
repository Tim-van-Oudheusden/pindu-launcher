//! Title matching for library search: case- and accent-insensitive, on substrings and
//! word prefixes, ignoring a leading "The".

/// Lowercases, folds Latin accents to plain letters, turns punctuation into spaces,
/// collapses spaces, and drops a leading "the ".
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            match fold(c) {
                Some(plain) => out.push_str(plain),
                None => out.push(c),
            }
        } else {
            pending_space = true;
        }
    }
    match out.strip_prefix("the ") {
        Some(rest) => rest.to_owned(),
        None => out,
    }
}

/// The name a title sorts by.
pub fn sort_name(title: &str) -> String {
    normalize(title)
}

/// A search as typed, prepared once and matched against many normalised titles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    text: String,
    words: Vec<String>,
}

impl Query {
    pub fn new(text: &str) -> Self {
        let text = normalize(text);
        let words = text
            .split(' ')
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect();
        Self { text, words }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The normalised query text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether a normalised title matches: it contains the query, or every query word
    /// starts one of its words, in any order.
    pub fn matches(&self, title: &str) -> bool {
        self.text.is_empty()
            || title.contains(&self.text)
            || self.words.iter().all(|word| {
                title
                    .split(' ')
                    .any(|title_word| title_word.starts_with(word.as_str()))
            })
    }
}

/// The plain letters of a lowercase accented Latin letter.
fn fold(c: char) -> Option<&'static str> {
    Some(match c {
        'à'..='å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ð' | 'ď' | 'đ' => "d",
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĳ' => "ij",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ŉ' => "n",
        'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' => "t",
        'þ' => "th",
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(query: &str, title: &str) -> bool {
        Query::new(query).matches(&normalize(title))
    }

    #[test]
    fn matching_ignores_case() {
        assert!(matches("RING", "Elden Ring"));
        assert!(matches("elden", "ELDEN RING"));
    }

    #[test]
    fn matching_ignores_accents() {
        assert!(matches("pokemon", "Pokémon Legends: Arceus"));
        assert!(matches("Pokémon", "Pokemon Snap"));
        assert!(matches("cafe", "Café Ñandú"));
        assert!(matches("nandu", "Café Ñandú"));
        assert!(matches("strasse", "Straße"));
    }

    #[test]
    fn substrings_match_anywhere() {
        assert!(matches("lden r", "Elden Ring"));
        assert!(!matches("dota", "Elden Ring"));
    }

    #[test]
    fn word_prefixes_match_in_any_order() {
        assert!(matches("wit wild", "The Witcher 3: Wild Hunt"));
        assert!(matches("hunt witch", "The Witcher 3: Wild Hunt"));
        assert!(!matches("wit wolf", "The Witcher 3: Wild Hunt"));
    }

    #[test]
    fn a_leading_the_is_ignored_on_either_side() {
        assert!(matches("witcher", "The Witcher 3"));
        assert!(matches("the witcher", "Witcher 3"));
        assert!(matches("The Witcher", "The Witcher 3"));
    }

    #[test]
    fn punctuation_separates_words() {
        assert!(matches("witcher 3 wild", "The Witcher 3: Wild Hunt"));
        assert!(matches("half life", "Half-Life 2"));
    }

    #[test]
    fn an_empty_query_matches_everything() {
        assert!(matches("", "Anything"));
        assert!(matches("   ", "Anything"));
    }

    #[test]
    fn sorting_names_drop_the_leading_the() {
        assert_eq!(sort_name("The Witcher 3"), "witcher 3");
        assert_eq!(sort_name("Theme Hospital"), "theme hospital");
        assert_eq!(sort_name("Élite"), "elite");
    }
}
