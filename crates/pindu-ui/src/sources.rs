//! User-facing names of game sources, when they last synced, how an external importer runs,
//! and the source order a game's copies follow.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::gio;
use pindu_core::game_groups;
use pindu_core::model::Source;
use pindu_core::sections;
use pindu_core::sources::external::listing::{Listed, Running};
use pindu_core::sync_age::SyncAge;

/// The setting that stores the source order (see [`game_groups::source_order`]).
pub const SOURCE_ORDER_KEY: &str = "source-order";

/// The user's source order for a game's copies, every source once (see
/// [`game_groups::source_order`]).
pub fn source_order(settings: &gio::Settings) -> Vec<Source> {
    game_groups::source_order(
        settings
            .strv(SOURCE_ORDER_KEY)
            .iter()
            .map(|value| value.as_str()),
    )
}

/// Moves `source` one place up (`earlier`) or down in the user's source order.
fn move_source(settings: &gio::Settings, source: Source, earlier: bool) {
    let order = game_groups::moved_source(&source_order(settings), source, earlier);
    let values: Vec<&str> = order.iter().map(|source| source.as_str()).collect();
    let _ = settings.set_strv(SOURCE_ORDER_KEY, values.as_slice());
}

/// Fills `list` with the user's source order, one row per source with Move Up and Move
/// Down, and fills it again whenever the order changes, here or elsewhere. Preferences and
/// fullscreen mode's Sources level both show it; with a controller, Right on a row reaches
/// its buttons and A presses one, and focus follows the source it moved.
pub fn bind_source_order(list: &gtk::ListBox, settings: &gio::Settings) {
    let moved: Rc<Cell<Option<(Source, bool)>>> = Rc::default();
    fill_source_order(list, settings, &moved);
    let list = list.downgrade();
    settings.connect_changed(Some(SOURCE_ORDER_KEY), move |settings, _| {
        if let Some(list) = list.upgrade() {
            fill_source_order(&list, settings, &moved);
        }
    });
}

/// One row per source in the user's order; when focus was in `list`, it goes to the
/// button that moved `moved`'s source, or to its other button at either end.
fn fill_source_order(
    list: &gtk::ListBox,
    settings: &gio::Settings,
    moved: &Rc<Cell<Option<(Source, bool)>>>,
) {
    let had_focus = list.focus_child().is_some();
    list.remove_all();
    let order = source_order(settings);
    let last = order.len().saturating_sub(1);
    for (index, &source) in order.iter().enumerate() {
        let row = adw::ActionRow::builder().title(source_name(source)).build();
        row.add_prefix(
            &gtk::Image::builder()
                .icon_name(source_icon(source))
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build(),
        );
        let up = move_button("go-up-symbolic", &gettext("Move Up"), index > 0);
        let down = move_button("go-down-symbolic", &gettext("Move Down"), index < last);
        for (button, earlier) in [(&up, true), (&down, false)] {
            let settings = settings.clone();
            let moved = Rc::clone(moved);
            button.connect_clicked(move |_| {
                moved.set(Some((source, earlier)));
                move_source(&settings, source, earlier);
            });
            row.add_suffix(button);
        }
        list.append(&row);
        if let Some((moved_source, earlier)) = moved.get()
            && had_focus
            && moved_source == source
        {
            let (same, other) = if earlier { (&up, &down) } else { (&down, &up) };
            if same.is_sensitive() {
                same.grab_focus();
            } else {
                other.grab_focus();
            }
        }
    }
}

/// A flat Move Up or Move Down button, named for screen readers by its tooltip.
fn move_button(icon: &str, label: &str, sensitive: bool) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(label)
        .valign(gtk::Align::Center)
        .sensitive(sensitive)
        .css_classes(["flat"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

/// The name of a source as the user sees it.
pub fn source_name(source: Source) -> String {
    match source {
        Source::Steam => "Steam".to_owned(),
        Source::HeroicEpic => gettext("Heroic: Epic"),
        Source::HeroicGog => gettext("Heroic: GOG"),
        Source::HeroicAmazon => gettext("Heroic: Amazon"),
        Source::HeroicSideload => gettext("Heroic: Sideloaded"),
        Source::Lutris => "Lutris".to_owned(),
        Source::Bottles => "Bottles".to_owned(),
        Source::Faugus => "Faugus".to_owned(),
        Source::Legendary => "Legendary".to_owned(),
        Source::Itch => "itch".to_owned(),
        Source::Desktop => gettext("Desktop"),
        Source::Manual => gettext("Added by Hand"),
        Source::External => gettext("External Importers"),
    }
}

/// The names of `sources` as a list in a sentence: "Steam", "Steam and Lutris", "Steam,
/// Lutris and Bottles".
pub fn source_list(sources: &[Source]) -> String {
    let names: Vec<String> = sources.iter().copied().map(source_name).collect();
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => {
            // Translators: the separator between the first items of a list.
            let items = rest.join(&gettext(", "));
            // Translators: a list's first items and its last, as in "Steam, Lutris and Bottles".
            gettext("{items} and {last}")
                .replace("{items}", &items)
                .replace("{last}", last)
        }
    }
}

/// The symbolic icon that stands for a source wherever the window names it: its own
/// (`sections::source_icon`), or a generic one for a launcher whose mark Pindu does not
/// ship.
pub fn source_icon(source: Source) -> &'static str {
    sections::source_icon(source).unwrap_or("applications-games-symbolic")
}

/// How long ago a source last synced, as Preferences says it.
pub fn sync_age_text(age: SyncAge) -> String {
    let count = |text: String, n: u32| text.replace("{count}", &n.to_string());
    match age {
        // Translators: when a source last synced; it never has.
        SyncAge::Never => gettext("Never"),
        // Translators: when a source last synced; under a minute ago.
        SyncAge::JustNow => gettext("Just now"),
        SyncAge::Minutes(n) => count(ngettext("{count} minute ago", "{count} minutes ago", n), n),
        SyncAge::Hours(n) => count(ngettext("{count} hour ago", "{count} hours ago", n), n),
        SyncAge::Days(n) => count(ngettext("{count} day ago", "{count} days ago", n), n),
    }
}

/// Where an external importer runs, as both shells say it; `None` when its manifest
/// cannot be read.
pub fn running_text(running: Running) -> Option<String> {
    match running {
        Running::InItsSandbox => Some(gettext("In its own sandbox, without network")),
        Running::WithYourPermissions => Some(gettext("Runs with your permissions")),
        Running::NeedsHostAccess => Some(gettext("Needs host access to run")),
        Running::Unknown => None,
    }
}

/// An external importer in one line, as fullscreen mode's Sources shows it: whether it is
/// experimental, where it runs, and whether its source is on this computer or it cannot
/// run.
pub fn helper_summary(helper: &Listed) -> String {
    let mut parts = Vec::new();
    if helper.experimental {
        parts.push(gettext("Experimental"));
    }
    match running_text(helper.running) {
        Some(running) => parts.push(running),
        None => parts.push(gettext("Cannot run")),
    }
    if helper.running != Running::Unknown {
        parts.push(if helper.found.is_some() {
            gettext("Found on this computer")
        } else {
            gettext("Not found on this computer")
        });
    }
    // Translators: the separator between the facts about one source.
    parts.join(&gettext(" · "))
}
