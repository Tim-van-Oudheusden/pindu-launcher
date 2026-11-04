//! How Play starts a game and how "Open in launcher" reaches its launcher.

use crate::model::{Game, LaunchKind, Source};

/// The way to start a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Hand the game to its launcher through a URI (the OpenURI portal in the sandbox).
    /// Fire and forget: there is no process to follow.
    StoreUri(String),
}

/// The route Play takes for a game, or `None` when this version cannot start it.
pub fn route(game: &Game) -> Option<Route> {
    match game.launch_kind {
        LaunchKind::StoreUri => store_uri(game).map(Route::StoreUri),
        LaunchKind::HostCommand
        | LaunchKind::DesktopEntry
        | LaunchKind::Proton
        | LaunchKind::RuntimeNative => None,
    }
}

fn store_uri(game: &Game) -> Option<String> {
    let id = game.id.native_id();
    match game.source() {
        Source::Steam if game.steam_launch_dialog => Some(format!("steam://launch/{id}/dialog")),
        Source::Steam => Some(format!("steam://rungameid/{id}")),
        source => {
            let runner = heroic_runner(source)?;
            Some(format!(
                "heroic://launch?appName={}&runner={runner}",
                percent_encode(id)
            ))
        }
    }
}

/// A URI that brings up the game's launcher: Steam's page for the game, or Heroic
/// itself, which has no URI for a game's page.
pub fn launcher_uri(game: &Game) -> Option<String> {
    match game.source() {
        Source::Steam => Some(format!("steam://nav/games/details/{}", game.id.native_id())),
        source if heroic_runner(source).is_some() => Some("heroic://ping".to_owned()),
        _ => None,
    }
}

fn heroic_runner(source: Source) -> Option<&'static str> {
    Some(match source {
        Source::HeroicEpic => "legendary",
        Source::HeroicGog => "gog",
        Source::HeroicAmazon => "nile",
        Source::HeroicSideload => "sideload",
        _ => return None,
    })
}

/// Percent-encodes everything but RFC 3986 unreserved characters.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Game, GameId, LaunchKind, Source};

    fn store_game(source: Source, native_id: &str) -> Game {
        Game::new(
            GameId::new(source, native_id),
            "Game",
            LaunchKind::StoreUri,
            0,
        )
    }

    #[test]
    fn steam_games_start_by_app_id() {
        let game = store_game(Source::Steam, "570");

        assert_eq!(
            route(&game),
            Some(Route::StoreUri("steam://rungameid/570".into()))
        );
    }

    #[test]
    fn steam_games_can_ask_steam_for_launch_options() {
        let mut game = store_game(Source::Steam, "570");
        game.steam_launch_dialog = true;

        assert_eq!(
            route(&game),
            Some(Route::StoreUri("steam://launch/570/dialog".into()))
        );
    }

    #[test]
    fn heroic_games_name_their_runner() {
        let cases = [
            (Source::HeroicEpic, "Sugar", "legendary"),
            (Source::HeroicGog, "1207658924", "gog"),
            (Source::HeroicAmazon, "amzn1.adg.product.0001", "nile"),
            (Source::HeroicSideload, "aB3dE", "sideload"),
        ];
        for (source, app_name, runner) in cases {
            assert_eq!(
                route(&store_game(source, app_name)),
                Some(Route::StoreUri(format!(
                    "heroic://launch?appName={app_name}&runner={runner}"
                ))),
                "{source}"
            );
        }
    }

    #[test]
    fn app_names_are_percent_encoded() {
        let game = store_game(Source::HeroicSideload, "my game&more=1");

        assert_eq!(
            route(&game),
            Some(Route::StoreUri(
                "heroic://launch?appName=my%20game%26more%3D1&runner=sideload".into()
            ))
        );
    }

    #[test]
    fn games_without_a_store_launcher_have_no_store_route() {
        let manual = Game::new(
            GameId::new(Source::Manual, "x"),
            "X",
            LaunchKind::HostCommand,
            0,
        );

        assert_eq!(route(&manual), None);
    }

    #[test]
    fn open_in_launcher_shows_the_game_in_steam_or_starts_heroic() {
        assert_eq!(
            launcher_uri(&store_game(Source::Steam, "570")).as_deref(),
            Some("steam://nav/games/details/570")
        );
        assert_eq!(
            launcher_uri(&store_game(Source::HeroicGog, "1")).as_deref(),
            Some("heroic://ping")
        );
        assert_eq!(
            launcher_uri(&Game::new(
                GameId::new(Source::Manual, "x"),
                "X",
                LaunchKind::HostCommand,
                0
            )),
            None
        );
    }
}
