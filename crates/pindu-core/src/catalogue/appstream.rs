//! Reading Flathub's AppStream catalogue: the `appstream.xml` Flatpak deploys beside its
//! compressed copy, one `<component>` an app, runtime or add-on.

use std::collections::BTreeMap;

use quick_xml::Reader;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};

/// What kind of component an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An application: `desktop-application`, or `desktop` as older entries say.
    Application,
    /// Anything else: a runtime, an add-on, a console application or a generic component.
    Other,
}

/// A block of an app's description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A paragraph.
    Paragraph(String),
    /// An item of a list.
    Item(String),
}

/// An app's licence, as the app page says it in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Licence {
    /// Free software, with its SPDX expression.
    Free(String),
    /// Proprietary: `LicenseRef-proprietary`.
    Proprietary,
    /// The catalogue does not say.
    Unknown,
}

/// The SPDX reference that marks a proprietary licence.
const PROPRIETARY: &str = "LicenseRef-proprietary";

impl Licence {
    /// The licence the SPDX expression `spdx` names: proprietary when it refers to
    /// `LicenseRef-proprietary`, unknown when empty.
    pub fn from_spdx(spdx: &str) -> Self {
        if spdx.contains(PROPRIETARY) {
            Self::Proprietary
        } else if spdx.is_empty() {
            Self::Unknown
        } else {
            Self::Free(spdx.to_owned())
        }
    }

    /// The licence as an SPDX expression, for which [`from_spdx`](Self::from_spdx) gives it
    /// back.
    pub fn spdx(&self) -> &str {
        match self {
            Self::Free(spdx) => spdx,
            Self::Proprietary => PROPRIETARY,
            Self::Unknown => "",
        }
    }
}

/// A screenshot, as addresses on Flathub's media server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screenshot {
    /// The thumbnail nearest 752 pixels wide, for a page of screenshots.
    pub thumbnail: String,
    /// The largest thumbnail, or the original when it has none, for a screenshot shown
    /// alone.
    pub image: String,
}

/// An entry of the catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The Flatpak app ID.
    pub id: String,
    /// What kind of component it is.
    pub kind: Kind,
    /// Its name.
    pub name: String,
    /// Its one-line summary.
    pub summary: String,
    /// Its description.
    pub description: Vec<Block>,
    /// Its freedesktop categories, `Game` and the additional ones.
    pub categories: Vec<String>,
    /// The words it is also found by.
    pub keywords: Vec<String>,
    /// Its licence.
    pub licence: Licence,
    /// Who makes it, when the catalogue says.
    pub developer: Option<String>,
    /// Whether Flathub has verified its developer.
    pub verified: bool,
    /// Its icon, as a path under the catalogue's folder: the 128 pixel one, else the 64.
    pub icon: Option<String>,
    /// Its screenshots, the default one first.
    pub screenshots: Vec<Screenshot>,
    /// The youngest age its content suits, derived from its OARS content rating; 0 for all
    /// ages, `None` when it has no rating.
    pub age: Option<u8>,
    /// Whether it says it supports a gamepad.
    pub gamepad: bool,
    /// The desktop entry that starts it, once installed.
    pub desktop_id: Option<String>,
}

/// Why a catalogue could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

/// The entries of the AppStream catalogue `xml`, with their texts in the first of
/// `languages` they are translated to, untranslated otherwise.
pub fn parse(xml: &str, languages: &[String]) -> Result<Vec<App>, ParseError> {
    let mut reader = Reader::from_str(xml);
    let mut parser = Parser {
        languages,
        path: Vec::new(),
        entry: None,
        text: String::new(),
        apps: Vec::new(),
    };
    let broken = |reader: &Reader<&[u8]>, error: &dyn std::fmt::Display| {
        ParseError(format!("at byte {}: {error}", reader.error_position()))
    };
    let mut root = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| broken(&reader, &error))?;
        if event == Event::Eof {
            break;
        }
        match event {
            Event::Start(start) | Event::Empty(start) if parser.path.is_empty() => {
                if start.name().as_ref() != "components" {
                    return Err(ParseError(format!(
                        "not an AppStream catalogue: its root is <{}>",
                        start.name().as_ref()
                    )));
                }
                root = true;
                parser
                    .open(&start)
                    .map_err(|error| broken(&reader, &error))?;
            }
            Event::Start(start) => parser
                .open(&start)
                .map_err(|error| broken(&reader, &error))?,
            Event::Empty(start) => {
                parser
                    .open(&start)
                    .map_err(|error| broken(&reader, &error))?;
                parser.close();
            }
            Event::End(_) => parser.close(),
            Event::Text(text) => parser.text.push_str(&text.xml10_content()),
            Event::CData(data) => parser.text.push_str(&data.xml10_content()),
            Event::GeneralRef(reference) => {
                match reference
                    .resolve_char_ref()
                    .map_err(|error| broken(&reader, &error))?
                {
                    Some(character) => parser.text.push(character),
                    None => parser
                        .text
                        .push_str(resolve_predefined_entity(&reference).unwrap_or_default()),
                }
            }
            _ => {}
        }
    }
    if !root {
        return Err(ParseError(
            "not an AppStream catalogue: no root element".to_owned(),
        ));
    }
    if let Some(open) = parser.path.last() {
        return Err(ParseError(format!("it ends inside <{}>", open.name)));
    }
    Ok(parser.apps)
}

/// The languages a session with the locale `locale` (`lang_COUNTRY.ENCODING@MODIFIER`, as
/// `LANG` holds it) reads, most specific first: `lang_COUNTRY`, then `lang`. None for the C
/// and POSIX locales.
pub fn languages(locale: &str) -> Vec<String> {
    let locale = locale.split(['.', '@']).next().unwrap_or_default();
    if locale.is_empty() || locale == "C" || locale == "POSIX" {
        return Vec::new();
    }
    let mut languages = vec![locale.to_owned()];
    if let Some((language, _)) = locale.split_once('_') {
        languages.push(language.to_owned());
    }
    languages
}

/// The youngest age, from 0, that the OARS rating `id` at each level suits (none, mild,
/// moderate, intense): Common Sense Media's ages as AppStream maps them
/// (`as_content_rating_attribute_to_csm_age`).
const OARS_AGES: [(&str, [u8; 4]); 28] = [
    ("violence-cartoon", [0, 3, 4, 6]),
    ("violence-fantasy", [0, 3, 7, 8]),
    ("violence-realistic", [0, 4, 9, 14]),
    ("violence-bloodshed", [0, 9, 11, 18]),
    ("violence-sexual", [0, 18, 18, 18]),
    ("drugs-alcohol", [0, 11, 13, 16]),
    ("drugs-narcotics", [0, 12, 14, 17]),
    ("drugs-tobacco", [0, 10, 13, 13]),
    ("sex-nudity", [0, 12, 14, 14]),
    ("sex-themes", [0, 13, 14, 15]),
    ("language-profanity", [0, 8, 11, 14]),
    ("language-humor", [0, 3, 8, 14]),
    ("language-discrimination", [0, 9, 10, 11]),
    ("money-advertising", [0, 7, 8, 10]),
    ("money-gambling", [0, 7, 10, 18]),
    ("money-purchasing", [0, 12, 14, 15]),
    ("social-chat", [0, 4, 10, 13]),
    ("social-audio", [0, 15, 15, 15]),
    ("social-contacts", [0, 12, 12, 12]),
    ("social-info", [0, 0, 13, 13]),
    ("social-location", [0, 13, 13, 13]),
    ("sex-homosexuality", [0, 13, 14, 15]),
    ("sex-prostitution", [0, 12, 14, 18]),
    ("sex-adultery", [0, 8, 10, 18]),
    ("sex-appearance", [0, 10, 10, 15]),
    ("violence-worship", [0, 13, 15, 18]),
    ("violence-desecration", [0, 13, 15, 18]),
    ("violence-slavery", [0, 13, 15, 18]),
];

/// The age OARS rating `id` at level `value` suits ([`OARS_AGES`]); 0 for a rating it
/// does not know.
fn oars_age(id: &str, value: &str) -> u8 {
    let level = match value {
        "mild" => 1,
        "moderate" => 2,
        "intense" => 3,
        _ => 0,
    };
    OARS_AGES
        .iter()
        .find(|(key, _)| *key == id)
        .map_or(0, |(_, ages)| ages[level])
}

/// The width a screenshot page shows a thumbnail at.
const THUMBNAIL_WIDTH: u32 = 752;

/// An element the reader is inside, with the attributes that matter.
struct Element {
    name: String,
    /// Its `xml:lang`, or the nearest enclosing element's.
    lang: Option<String>,
    /// Its `type`.
    kind: Option<String>,
    width: Option<u32>,
    scale: Option<u32>,
    key: Option<String>,
    id: Option<String>,
    /// Where its text, with that of the elements in it, starts in the parser's.
    start: usize,
}

/// A translated text: the best one so far, ranked by the session's languages.
#[derive(Default)]
struct Translated {
    rank: Option<usize>,
    text: String,
}

impl Translated {
    fn offer(&mut self, rank: usize, text: String) {
        if self.rank.is_none_or(|best| rank < best) {
            self.rank = Some(rank);
            self.text = text;
        }
    }
}

/// A screenshot while it is read.
#[derive(Default)]
struct Shot {
    thumbnails: Vec<(u32, String)>,
    source: Option<String>,
}

/// A component while it is read.
#[derive(Default)]
struct Entry {
    id: String,
    application: bool,
    name: Translated,
    summary: Translated,
    /// Each language's description, by rank.
    descriptions: BTreeMap<usize, Vec<Block>>,
    categories: Vec<String>,
    keywords: Vec<String>,
    licence: String,
    developer: Translated,
    verified: bool,
    /// The largest cached icon so far, with its width.
    icon: Option<(u32, String)>,
    screenshots: Vec<Screenshot>,
    shot: Shot,
    age: Option<u8>,
    gamepad: bool,
    desktop_id: Option<String>,
}

impl Entry {
    fn finish(self) -> App {
        let licence = Licence::from_spdx(&self.licence);
        App {
            id: self.id,
            kind: if self.application {
                Kind::Application
            } else {
                Kind::Other
            },
            name: self.name.text,
            summary: self.summary.text,
            description: self.descriptions.into_values().next().unwrap_or_default(),
            categories: self.categories,
            keywords: self.keywords,
            licence,
            developer: self.developer.rank.map(|_| self.developer.text),
            verified: self.verified,
            icon: self
                .icon
                .map(|(width, file)| format!("icons/{width}x{width}/{file}")),
            screenshots: self.screenshots,
            age: self.age,
            gamepad: self.gamepad,
            desktop_id: self.desktop_id,
        }
    }
}

/// The reader's state.
struct Parser<'a> {
    languages: &'a [String],
    path: Vec<Element>,
    entry: Option<Entry>,
    /// The text of the component being read; each element's is the part since it opened.
    text: String,
    apps: Vec<App>,
}

impl Parser<'_> {
    /// How well text in `lang` suits the session: its place in the languages, untranslated
    /// text after them all, `None` for a language the session does not read.
    fn rank(&self, lang: Option<&str>) -> Option<usize> {
        match lang {
            None => Some(self.languages.len()),
            Some(lang) => self.languages.iter().position(|language| language == lang),
        }
    }

    fn open(&mut self, start: &BytesStart<'_>) -> Result<(), quick_xml::Error> {
        let mut element = Element {
            name: start.name().as_ref().to_owned(),
            lang: self.path.last().and_then(|parent| parent.lang.clone()),
            kind: None,
            width: None,
            scale: None,
            key: None,
            id: None,
            start: self.text.len(),
        };
        for attribute in start.attributes() {
            let attribute = attribute.map_err(quick_xml::Error::InvalidAttr)?;
            let value = attribute.value.into_owned();
            match attribute.key.as_ref() {
                "xml:lang" => element.lang = Some(value),
                "type" => element.kind = Some(value),
                "width" => element.width = value.parse().ok(),
                "scale" => element.scale = value.parse().ok(),
                "key" => element.key = Some(value),
                "id" => element.id = Some(value),
                _ => {}
            }
        }
        // Each child of the root is a component, which only counts once it has an ID.
        if self.path.len() == 1 {
            self.entry = Some(Entry {
                application: matches!(
                    element.kind.as_deref(),
                    Some("desktop-application" | "desktop")
                ),
                ..Entry::default()
            });
        }
        if let Some(entry) = &mut self.entry
            && element.name == "content_rating"
        {
            entry.age = Some(entry.age.unwrap_or(0));
        }
        self.path.push(element);
        Ok(())
    }

    fn close(&mut self) {
        let Some(element) = self.path.pop() else {
            return;
        };
        if self.path.len() == 1 {
            self.text.clear();
            if let Some(entry) = self.entry.take()
                && !entry.id.is_empty()
            {
                self.apps.push(entry.finish());
            }
            return;
        }
        let rank = self.rank(element.lang.as_deref());
        // The app's own description, not a release's: components > component > description.
        let in_description = self
            .path
            .get(2)
            .is_some_and(|open| open.name == "description");
        let parent = self.path.last().map_or("", |parent| parent.name.as_str());
        let Some(entry) = &mut self.entry else {
            return;
        };
        let text = &self.text[element.start..];
        let value = || text.split_whitespace().collect::<Vec<_>>().join(" ");
        match (parent, element.name.as_str()) {
            ("component", "id") => entry.id = value(),
            ("component", "name") => {
                if let Some(rank) = rank {
                    entry.name.offer(rank, value());
                }
            }
            ("component", "summary") => {
                if let Some(rank) = rank {
                    entry.summary.offer(rank, value());
                }
            }
            ("component", "developer_name") | ("developer", "name") => {
                if let Some(rank) = rank {
                    entry.developer.offer(rank, value());
                }
            }
            ("component", "project_license") => entry.licence = value(),
            ("categories", "category") => entry.categories.push(value()),
            ("keywords", "keyword") => {
                let keyword = value();
                if rank.is_some() && !entry.keywords.contains(&keyword) {
                    entry.keywords.push(keyword);
                }
            }
            (_, "p" | "li") if in_description => {
                if let Some(rank) = rank {
                    let text = value();
                    let block = if element.name == "p" {
                        Block::Paragraph(text)
                    } else {
                        Block::Item(text)
                    };
                    entry.descriptions.entry(rank).or_default().push(block);
                }
            }
            ("custom", "value")
                if element.key.as_deref() == Some("flathub::verification::verified") =>
            {
                entry.verified = value() == "true";
            }
            ("component", "icon")
                if element.kind.as_deref() == Some("cached") && element.scale.unwrap_or(1) == 1 =>
            {
                let width = element.width.unwrap_or(64);
                if entry.icon.as_ref().is_none_or(|(best, _)| width > *best) {
                    entry.icon = Some((width, value()));
                }
            }
            ("screenshot", "image") if rank.is_some() => match element.kind.as_deref() {
                Some("thumbnail") => entry
                    .shot
                    .thumbnails
                    .push((element.width.unwrap_or(0), value())),
                _ => entry.shot.source = Some(value()),
            },
            ("screenshots", "screenshot") => {
                let shot = std::mem::take(&mut entry.shot);
                let largest = shot
                    .thumbnails
                    .iter()
                    .max_by_key(|(width, _)| *width)
                    .map(|(_, url)| url.clone())
                    .or(shot.source);
                let nearest = shot
                    .thumbnails
                    .iter()
                    .min_by_key(|(width, _)| width.abs_diff(THUMBNAIL_WIDTH))
                    .map(|(_, url)| url.clone());
                if let Some(image) = largest {
                    entry.screenshots.push(Screenshot {
                        thumbnail: nearest.unwrap_or_else(|| image.clone()),
                        image,
                    });
                }
            }
            ("content_rating", "content_attribute") => {
                let age = oars_age(element.id.as_deref().unwrap_or_default(), &value());
                entry.age = Some(entry.age.unwrap_or(0).max(age));
            }
            ("recommends" | "requires" | "supports", "control") => {
                entry.gamepad |= value() == "gamepad";
            }
            ("component", "launchable")
                if element.kind.as_deref() == Some("desktop-id") && entry.desktop_id.is_none() =>
            {
                entry.desktop_id = Some(value());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/catalogue/appstream/flathub/x86_64/active/appstream.xml");
        std::fs::read_to_string(path).unwrap()
    }

    fn app_in(id: &str, languages: &[&str]) -> App {
        let languages: Vec<String> = languages.iter().map(|l| (*l).to_owned()).collect();
        parse(&fixture(), &languages)
            .unwrap()
            .into_iter()
            .find(|app| app.id == id)
            .unwrap()
    }

    fn app(id: &str) -> App {
        app_in(id, &[])
    }

    #[test]
    fn an_entry_has_its_id_kind_name_and_summary() {
        let app = app("net.supertuxkart.SuperTuxKart");
        assert_eq!(app.kind, Kind::Application);
        assert_eq!(app.name, "SuperTuxKart");
        assert_eq!(app.summary, "A 3D open-source kart racing game");
    }

    #[test]
    fn every_component_is_read_and_only_applications_are_applications() {
        let apps = parse(&fixture(), &[]).unwrap();
        assert_eq!(apps.len(), 31);
        let kind = |id: &str| apps.iter().find(|app| app.id == id).unwrap().kind;
        assert_eq!(kind("org.mozilla.firefox"), Kind::Application);
        assert_eq!(kind("org.phoenicis.playonlinux"), Kind::Application);
        assert_eq!(kind("page.codeberg.Qwel.egui_picross"), Kind::Other);
        assert_eq!(kind("io.github.theforceengine.tfe.Debug"), Kind::Other);
        assert_eq!(kind("org.freedesktop.Platform"), Kind::Other);
    }

    #[test]
    fn entities_are_read_as_the_characters_they_stand_for() {
        assert_eq!(app("com.frogatto.Frogatto").name, "Frogatto & Friends");
        assert!(app("org.sonic3air.Sonic3AIR").summary.contains("3&K"));
    }

    #[test]
    fn texts_are_in_the_sessions_language_when_translated_else_untranslated() {
        let german = app_in("net.supertuxkart.SuperTuxKart", &["de_DE", "de"]);
        assert_eq!(german.summary, "Ein Open-Source 3D-Kart-Rennspiel");
        assert_eq!(german.name, "SuperTuxKart");
        let Some(Block::Paragraph(first)) = german.description.first() else {
            panic!("no paragraph: {:?}", german.description);
        };
        assert!(
            first.starts_with("Karts. Nitro. Action! SuperTuxKart ist ein Open-Source"),
            "{first}"
        );
        let english = app("net.supertuxkart.SuperTuxKart");
        let Some(Block::Paragraph(first)) = english.description.first() else {
            panic!("no paragraph");
        };
        assert!(first.starts_with("Karts. Nitro. Action! SuperTuxKart is a 3D"));
    }

    #[test]
    fn a_description_is_paragraphs_and_list_items_with_their_spaces_collapsed() {
        let description = app("net.wz2100.wz2100").description;
        assert!(
            description
                .iter()
                .any(|block| matches!(block, Block::Item(_)))
        );
        assert!(description.iter().all(|block| match block {
            Block::Paragraph(text) | Block::Item(text) =>
                !text.contains("  ") && !text.contains('\n') && text.trim() == text,
        }));
    }

    #[test]
    fn categories_and_keywords_are_listed() {
        let kart = app("net.supertuxkart.SuperTuxKart");
        assert_eq!(kart.categories, ["Game", "ArcadeGame"]);
        assert_eq!(kart.keywords, ["tux", "game", "race"]);
    }

    #[test]
    fn a_licence_is_free_proprietary_or_unknown() {
        assert_eq!(
            app("net.supertuxkart.SuperTuxKart").licence,
            Licence::Free("GPL-3.0+".to_owned())
        );
        assert_eq!(app("com.valvesoftware.Steam").licence, Licence::Proprietary);
        assert_eq!(app("net.clonehero.CloneHero").licence, Licence::Proprietary);
    }

    #[test]
    fn the_developer_and_flathubs_verification_are_read() {
        let kart = app("net.supertuxkart.SuperTuxKart");
        assert_eq!(kart.developer.as_deref(), Some("SuperTuxKart Team"));
        assert!(kart.verified);
        assert!(!app("org.supertuxproject.SuperTux").verified);
    }

    #[test]
    fn the_icon_is_the_largest_cached_one_as_a_path_under_the_catalogue() {
        assert_eq!(
            app("net.supertuxkart.SuperTuxKart").icon.as_deref(),
            Some("icons/128x128/net.supertuxkart.SuperTuxKart.png")
        );
        assert_eq!(
            app("com.valvesoftware.Steam.Utility.InhibitScreensaver").icon,
            None
        );
    }

    #[test]
    fn a_screenshot_has_a_thumbnail_near_752_pixels_and_its_largest_image() {
        let first = app("net.supertuxkart.SuperTuxKart").screenshots[0].clone();
        assert!(
            first.thumbnail.ends_with("/image-1_752x423@1.png"),
            "{first:?}"
        );
        assert!(
            first.image.ends_with("/image-1_1248x702@1.png"),
            "{first:?}"
        );
        assert!(
            app("io.github.theforceengine.tfe.Debug")
                .screenshots
                .is_empty()
        );
    }

    #[test]
    fn the_age_is_the_highest_its_content_rating_asks_for() {
        assert_eq!(app("org.zdoom.UZDoom").age, Some(18));
        assert_eq!(app("net.supertuxkart.SuperTuxKart").age, Some(13));
        assert_eq!(
            app("com.valvesoftware.Steam.Utility.InhibitScreensaver").age,
            None
        );
    }

    #[test]
    fn an_empty_content_rating_suits_all_ages() {
        assert_eq!(app("org.gnome.Chess").age, Some(0));
    }

    #[test]
    fn gamepad_support_and_the_desktop_entry_are_read() {
        let kart = app("net.supertuxkart.SuperTuxKart");
        assert!(kart.gamepad);
        assert_eq!(
            kart.desktop_id.as_deref(),
            Some("net.supertuxkart.SuperTuxKart.desktop")
        );
        assert!(!app("org.gnome.Calculator").gamepad);
    }

    #[test]
    fn a_broken_catalogue_or_another_xml_file_says_so() {
        assert!(parse("<components><component><id>x</id>", &[]).is_err());
        assert!(parse("<html><body/></html>", &[]).is_err());
        assert!(parse("not xml at all", &[]).is_err());
    }

    #[test]
    fn a_locale_gives_its_language_with_and_without_the_country() {
        assert_eq!(languages("de_DE.UTF-8"), ["de_DE", "de"]);
        assert_eq!(languages("pt_BR.UTF-8@latin"), ["pt_BR", "pt"]);
        assert_eq!(languages("fr"), ["fr"]);
        assert!(languages("C.UTF-8").is_empty());
        assert!(languages("POSIX").is_empty());
        assert!(languages("").is_empty());
    }

    /// The one entry of a catalogue holding only `component`'s inside.
    fn one(component: &str) -> App {
        let xml = format!(
            "<components><component type=\"desktop-application\">\
             <id>org.example.App</id>{component}</component></components>"
        );
        parse(&xml, &[]).unwrap().remove(0)
    }

    #[test]
    fn release_notes_are_not_the_description() {
        let example = one("<description><p>A game.</p></description>\
             <releases><release version=\"2\"><description><ul><li>Fixed it.</li></ul>\
             </description></release></releases>");
        assert_eq!(
            example.description,
            [Block::Paragraph("A game.".to_owned())]
        );
        let sgdboop = app("com.steamgriddb.SGDBoop");
        assert!(
            !sgdboop
                .description
                .contains(&Block::Item("Fixed icons not applying.".to_owned()))
        );
    }

    #[test]
    fn the_first_of_two_untranslated_texts_stays() {
        let app = one("<summary>First</summary><summary>Second</summary>");
        assert_eq!(app.summary, "First");
    }

    #[test]
    fn cdata_is_text() {
        let app = one("<summary><![CDATA[Tanks & <Lasers>]]></summary>");
        assert_eq!(app.summary, "Tanks & <Lasers>");
    }

    #[test]
    fn inline_markup_keeps_the_text_around_it() {
        let app = one(
            "<description><p>Play <em>now</em> or <code>later</code></p>\
             <ul><li>One <em>two</em></li></ul></description>",
        );
        assert_eq!(
            app.description,
            [
                Block::Paragraph("Play now or later".to_owned()),
                Block::Item("One two".to_owned())
            ]
        );
    }

    #[test]
    fn a_scaled_icon_or_a_second_of_the_same_size_is_not_the_icon() {
        let app = one(
            "<icon type=\"cached\" width=\"128\" height=\"128\" scale=\"2\">big.png</icon>\
             <icon type=\"cached\" width=\"64\" height=\"64\">small.png</icon>\
             <icon type=\"cached\" width=\"64\" height=\"64\">other.png</icon>",
        );
        assert_eq!(app.icon.as_deref(), Some("icons/64x64/small.png"));
    }

    #[test]
    fn a_screenshot_in_another_language_is_left_out() {
        let app = one("<screenshots><screenshot type=\"default\">\
             <image type=\"thumbnail\" width=\"752\">plain.png</image>\
             <image type=\"thumbnail\" width=\"752\" xml:lang=\"de\">german.png</image>\
             </screenshot></screenshots>");
        assert_eq!(
            app.screenshots,
            [Screenshot {
                thumbnail: "plain.png".to_owned(),
                image: "plain.png".to_owned()
            }]
        );
    }

    #[test]
    fn the_desktop_entry_is_the_first_desktop_id_launchable() {
        let app = one(
            "<launchable type=\"service\">org.example.Service</launchable>\
             <launchable type=\"desktop-id\">first.desktop</launchable>\
             <launchable type=\"desktop-id\">second.desktop</launchable>",
        );
        assert_eq!(app.desktop_id.as_deref(), Some("first.desktop"));
    }

    #[test]
    fn each_oars_level_has_its_own_age() {
        assert_eq!(oars_age("violence-cartoon", "none"), 0);
        assert_eq!(oars_age("violence-cartoon", "mild"), 3);
        assert_eq!(oars_age("violence-cartoon", "moderate"), 4);
        assert_eq!(oars_age("violence-cartoon", "intense"), 6);
        assert_eq!(oars_age("unknown-attribute", "intense"), 0);
    }
}
