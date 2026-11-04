//! Faugus Launcher: games from its plain-JSON library, `games.json` in its data folder.
//!
//! Faugus has no URI handler, so its games start on the host through
//! `faugus-launcher --game <id>` (or its Flatpak command), which also tracks their
//! playtime; we import that playtime instead of timing sessions. Faugus writes some
//! booleans and numbers as strings, so values are read tolerantly. Nothing is written.

use std::path::{Path, PathBuf};

use serde_json::Value as Json;

use super::{ImportError, ImportedGame, Snapshot};
use crate::model::{ArtLayout, LaunchConfig, LaunchKind, Platform, Source, Timestamp};

/// Faugus's Flatpak ID; its data folder lives under `~/.var/app/<id>`.
const FLATPAK_ID: &str = "io.github.Faugus.faugus-launcher";

/// Imports the Faugus data folder at `root`. `utc_offset` (seconds east of UTC) turns the
/// local times Faugus records into timestamps.
pub fn import(root: &Path, utc_offset: i64) -> Result<Snapshot, ImportError> {
    let games_path = root.join("games.json");
    let fail = |message: String| ImportError {
        path: games_path.clone(),
        message,
    };
    let text = std::fs::read_to_string(&games_path).map_err(|error| fail(error.to_string()))?;
    let games: Vec<Json> = serde_json::from_str(&text).map_err(|error| fail(error.to_string()))?;
    let flatpak = root.components().any(|part| part.as_os_str() == FLATPAK_ID);

    let mut snapshot = Snapshot::new(Source::Faugus);
    for entry in &games {
        let id = text_of(&entry["gameid"]);
        let (Some(id), Some(title)) = (id, text_of(&entry["title"])) else {
            let id = id.unwrap_or("?");
            snapshot
                .errors
                .push(format!("Faugus game {id} has no game ID or title"));
            continue;
        };
        let mut game = ImportedGame::new(id, title, LaunchKind::HostCommand);
        game.installed = true;
        game.platform = Some(if text_of(&entry["runner"]) == Some("Linux-Native") {
            Platform::Linux
        } else {
            Platform::Windows
        });
        game.install_path = text_of(&entry["path"])
            .and_then(|path| Path::new(path).parent())
            .map(Path::to_owned);
        game.playtime_secs = number(&entry["playtime"]).filter(|secs| *secs > 0);
        game.last_played_at =
            text_of(&entry["last_played"]).and_then(|time| parse_iso_timestamp(time, utc_offset));
        game.launch_config = Some(launch_command(id, flatpak));
        game.artwork = artwork(root, id, entry);
        game.source_flags.hidden = flag(&entry["hidden"]);
        game.source_flags.categories = match &entry["category"] {
            Json::String(name) if !name.is_empty() => vec![name.clone()],
            Json::Array(names) => names
                .iter()
                .filter_map(text_of)
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        snapshot.games.push(game);
    }
    Ok(snapshot)
}

/// `faugus-launcher --game <id>`, or the same through Faugus's Flatpak.
fn launch_command(id: &str, flatpak: bool) -> LaunchConfig {
    let game = ["--game".to_owned(), id.to_owned()];
    let (executable, arguments) = if flatpak {
        let run = ["run", "--command=/app/bin/faugus-launcher", FLATPAK_ID].map(str::to_owned);
        ("flatpak", run.into_iter().chain(game).collect())
    } else {
        ("faugus-launcher", game.to_vec())
    };
    LaunchConfig {
        executable: executable.into(),
        arguments,
        ..LaunchConfig::default()
    }
}

/// The cover and icon Faugus recorded (or keeps by game ID), and its banner.
fn artwork(root: &Path, id: &str, entry: &Json) -> Vec<(ArtLayout, PathBuf)> {
    let recorded = |key: &str, folder: &str| {
        text_of(&entry[key])
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .or_else(|| Some(root.join(folder).join(format!("{id}.png"))))
    };
    [
        (ArtLayout::Cover, recorded("cover", "covers")),
        (
            ArtLayout::Hero,
            Some(root.join("banners").join(format!("{id}.png"))),
        ),
        (ArtLayout::Icon, recorded("icon", "icons")),
    ]
    .into_iter()
    .filter_map(|(layout, path)| Some((layout, path?)))
    .filter(|(_, path)| path.is_file())
    .collect()
}

/// A non-empty string.
fn text_of(value: &Json) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

/// An integer, or a string holding one.
fn number(value: &Json) -> Option<i64> {
    match value {
        Json::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|n| n as i64)),
        Json::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// A boolean, or a string spelling one (`"True"`, `"true"`); anything else is false.
fn flag(value: &Json) -> bool {
    match value {
        Json::Bool(flag) => *flag,
        Json::String(text) => text.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

/// Parses an ISO 8601 date and time as Python's `isoformat()` writes it:
/// `YYYY-MM-DD[T ]HH:MM:SS[.ffffff][Z|±HH:MM]`. A time without an offset is local time,
/// `utc_offset` seconds east of UTC.
fn parse_iso_timestamp(text: &str, utc_offset: i64) -> Option<Timestamp> {
    let (date, time) = text.split_once(['T', ' '])?;
    let mut date_parts = date.splitn(3, '-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (
        date_parts.next()??,
        date_parts.next()??,
        date_parts.next()??,
    );
    let (clock, offset) = if let Some(clock) = time.strip_suffix('Z') {
        (clock, 0)
    } else if let Some(at) = time.rfind(['+', '-']) {
        let (clock, zone) = time.split_at(at);
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let (hours, minutes) = zone[1..].split_once(':').unwrap_or((&zone[1..], "0"));
        (
            clock,
            sign * (hours.parse::<i64>().ok()? * 3600 + minutes.parse::<i64>().ok()? * 60),
        )
    } else {
        (time, utc_offset)
    };
    let clock = clock.split_once('.').map_or(clock, |(whole, _)| whole);
    let mut clock_parts = clock.splitn(3, ':').map(|part| part.parse::<i64>().ok());
    let (hour, minute, second) = (
        clock_parts.next()??,
        clock_parts.next()??,
        clock_parts.next().flatten().unwrap_or(0),
    );
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_isoformat_times_parse_in_every_form() {
        assert_eq!(parse_iso_timestamp("1970-01-01T00:00:00", 0), Some(0));
        assert_eq!(
            parse_iso_timestamp("2024-02-29T23:59:59", 0),
            Some(1_709_251_199)
        );
        assert_eq!(
            parse_iso_timestamp("2024-02-29 23:59:59.999999", 0),
            Some(1_709_251_199)
        );
        assert_eq!(
            parse_iso_timestamp("2024-02-29T23:59:59Z", 7200),
            Some(1_709_251_199)
        );
        assert_eq!(
            parse_iso_timestamp("2024-03-01T01:59:59+02:00", 0),
            Some(1_709_251_199)
        );
        assert_eq!(
            parse_iso_timestamp("2024-02-29T18:29:59.5-05:30", 0),
            Some(1_709_251_199)
        );
    }

    #[test]
    fn a_time_without_an_offset_is_local_time() {
        // 01:00 in UTC+1 is midnight UTC.
        assert_eq!(parse_iso_timestamp("1970-01-01T01:00:00", 3600), Some(0));
        assert_eq!(parse_iso_timestamp("1969-12-31T19:00:00", -18_000), Some(0));
    }

    #[test]
    fn malformed_times_are_ignored() {
        for text in [
            "",
            "yesterday",
            "2024-02-29",
            "2024-13-01T00:00:00",
            "2024-02-29T25:00:00",
        ] {
            assert_eq!(parse_iso_timestamp(text, 0), None, "{text}");
        }
    }
}
