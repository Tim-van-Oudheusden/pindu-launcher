//! Get Games' lists file (`docs/get-games.md`, section 6.4): the categories that count as
//! games, a blocklist, extras and picks, each a list of Flathub app IDs (categories for
//! `categories`). The launcher's default is `data/get-games.yaml`; a distributor's file
//! replaces it whole.

use std::path::{Path, PathBuf};

use yaml_rust2::{Yaml, YamlLoader};

/// The launcher's own lists.
pub const DEFAULT: &str = include_str!("../../../../data/get-games.yaml");

/// The format version this launcher reads.
const VERSION: i64 = 1;

/// The editorial choices of Get Games.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lists {
    /// The AppStream categories whose apps count as games.
    pub categories: Vec<String>,
    /// Apps never shown, though they carry a game category.
    pub blocklist: Vec<String>,
    /// Apps outside the game categories that are offered anyway.
    pub extras: Vec<String>,
    /// Games shown first, in this order.
    pub picks: Vec<String>,
}

/// Why a lists file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListsError(pub String);

/// Where a distributor puts its lists, under the host's `/usr` (`host_usr`).
pub fn distributor_path(host_usr: &Path) -> PathBuf {
    host_usr.join("share/pindu-launcher/distributor/get-games.yaml")
}

/// The lists in the YAML `text`.
pub fn parse(text: &str) -> Result<Lists, ListsError> {
    let documents =
        YamlLoader::load_from_str(text).map_err(|error| ListsError(error.to_string()))?;
    let Some(Yaml::Hash(lists)) = documents.into_iter().next() else {
        return Err(ListsError("not a mapping of lists".to_owned()));
    };
    match lists.get(&Yaml::String("version".to_owned())) {
        Some(Yaml::Integer(VERSION)) => {}
        Some(version) => {
            return Err(ListsError(format!(
                "version {version:?}, where this launcher reads {VERSION}"
            )));
        }
        None => return Err(ListsError("no version".to_owned())),
    }
    let list = |key: &str| -> Result<Option<Vec<String>>, ListsError> {
        match lists.get(&Yaml::String(key.to_owned())) {
            None | Some(Yaml::Null) => Ok(None),
            Some(Yaml::Array(entries)) => entries
                .iter()
                .map(|entry| match entry {
                    Yaml::String(id) => Ok(id.clone()),
                    other => Err(ListsError(format!("{key}: {other:?} is not an ID"))),
                })
                .collect::<Result<_, _>>()
                .map(Some),
            Some(other) => Err(ListsError(format!("{key}: {other:?} is not a list"))),
        }
    };
    Ok(Lists {
        categories: list("categories")?.unwrap_or_else(|| vec!["Game".to_owned()]),
        blocklist: list("blocklist")?.unwrap_or_default(),
        extras: list("extras")?.unwrap_or_default(),
        picks: list("picks")?.unwrap_or_default(),
    })
}

/// The lists that apply: the distributor's file when there is one (`distributor`, its
/// text), else the launcher's default. A distributor's file that cannot be read gives the
/// default too, with why.
pub fn read(distributor: Option<&str>) -> (Lists, Option<ListsError>) {
    let default = || parse(DEFAULT).unwrap_or_default();
    match distributor.map(parse) {
        None => (default(), None),
        Some(Ok(lists)) => (lists, None),
        Some(Err(error)) => (default(), Some(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[String]) -> Vec<&str> {
        list.iter().map(String::as_str).collect()
    }

    #[test]
    fn the_default_lists_offer_games_with_kodi_and_firefox_and_block_tools() {
        let lists = parse(DEFAULT).unwrap();
        assert_eq!(ids(&lists.categories), ["Game"]);
        assert_eq!(ids(&lists.extras), ["tv.kodi.Kodi", "org.mozilla.firefox"]);
        assert_eq!(
            lists.picks.first().map(String::as_str),
            Some("net.supertuxkart.SuperTuxKart")
        );
        assert!(
            lists
                .blocklist
                .iter()
                .any(|id| id == "com.steamgriddb.SGDBoop")
        );
        assert_eq!(lists.blocklist.len(), 62);
    }

    #[test]
    fn pindu_oss_shipped_file_reads_as_it_is() {
        let text = include_str!("../../tests/fixtures/get-games/pindu-os.yaml");
        let lists = parse(text).unwrap();
        assert_eq!(ids(&lists.categories), ["Game"]);
        assert_eq!(
            ids(&lists.blocklist),
            ["com.steamgriddb.SGDBoop", "io.github.benjamimgois.goverlay"]
        );
        assert_eq!(ids(&lists.extras), ["tv.kodi.Kodi", "org.mozilla.firefox"]);
        assert_eq!(lists.picks.len(), 4);
    }

    #[test]
    fn a_missing_list_is_empty_and_missing_categories_mean_games() {
        let lists = parse("version: 1\nextras: [tv.kodi.Kodi]\n").unwrap();
        assert_eq!(ids(&lists.categories), ["Game"]);
        assert!(lists.blocklist.is_empty());
        assert!(lists.picks.is_empty());
        assert_eq!(ids(&lists.extras), ["tv.kodi.Kodi"]);
    }

    #[test]
    fn another_version_text_that_is_not_lists_or_an_entry_that_is_not_an_id_is_an_error() {
        assert!(parse("version: 2\ncategories: [Game]\n").is_err());
        assert!(parse("categories: [Game]\n").is_err());
        assert!(parse("version: 1\nblocklist: com.example.App\n").is_err());
        assert!(parse("version: 1\nblocklist:\n  - {id: com.example.App}\n").is_err());
        assert!(parse("version: 1\ncategories: [Game\n").is_err());
        assert!(parse("- just\n- a list\n").is_err());
    }

    #[test]
    fn a_distributors_file_replaces_the_default_whole() {
        let (lists, error) = read(Some(
            "version: 1\nblocklist: [io.github.benjamimgois.goverlay]\n",
        ));
        assert_eq!(error, None);
        assert_eq!(ids(&lists.blocklist), ["io.github.benjamimgois.goverlay"]);
        assert!(lists.extras.is_empty());
    }

    #[test]
    fn without_a_distributors_file_the_default_applies() {
        assert_eq!(read(None), (parse(DEFAULT).unwrap(), None));
    }

    #[test]
    fn a_broken_distributors_file_gives_the_default_and_says_why() {
        let (lists, error) = read(Some("version: 1\nblocklist: [\n"));
        assert_eq!(lists, parse(DEFAULT).unwrap());
        assert!(error.is_some());
    }

    #[test]
    fn the_distributors_file_is_under_usr_share() {
        assert_eq!(
            distributor_path(Path::new("/run/host/usr")),
            Path::new("/run/host/usr/share/pindu-launcher/distributor/get-games.yaml")
        );
    }
}
