//! Merging and splitting games by hand in the main window: the Merge With, Preferred Copy
//! and Split items of the desktop game menus, their window actions, and the toasts that
//! say what happened. On the desktop Merge With asks with the search dialog
//! (`merge_dialog`); in fullscreen mode it opens the options sheet's list of games, which
//! a controller moves through. A split is remembered (`Store::split_group`), so neither
//! the automatic grouping nor a suggestion brings the copies back together.

use adw::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};
use pindu_core::library::GameCopy;
use pindu_core::model::GameId;

use crate::merge_dialog::MergeDialog;
use crate::models::GameObject;
use crate::sources::source_name;
use crate::window::PinduWindow;

/// The Merge With item for a game's desktop menus and, for a group, its Preferred Copy
/// submenu (the copy it shows marked) and Split Into Separate Games.
pub fn group_menu(window: &PinduWindow, game: &GameObject) -> gio::Menu {
    let target = game.id().as_str().to_variant();
    let menu = gio::Menu::new();
    let merge = gio::MenuItem::new(Some(&gettext("_Merge With…")), None);
    merge.set_action_and_target_value(Some("win.merge-game"), Some(&target));
    menu.append_item(&merge);
    let entry = game.entry();
    if entry.copies.len() > 1 {
        let copies = gio::Menu::new();
        for copy in &entry.copies {
            let item = gio::MenuItem::new(Some(&copy_label(copy)), None);
            item.set_action_and_target_value(
                Some("win.prefer-copy"),
                Some(&copy.game.id.as_str().to_variant()),
            );
            copies.append_item(&item);
        }
        menu.append_submenu(Some(&gettext("Preferred _Copy")), &copies);
        // The radio items mark the copy the group shows: the first one.
        window.mark_preferred_copy(&entry.copies[0].game.id);
        let split = gio::MenuItem::new(Some(&gettext("_Split Into Separate Games")), None);
        split.set_action_and_target_value(Some("win.split-group"), Some(&target));
        menu.append_item(&split);
    }
    menu
}

/// A copy as a menu names it: its source, and whether it is installed.
fn copy_label(copy: &GameCopy) -> String {
    let source = source_name(copy.game.source());
    if playable(copy) {
        source
    } else {
        gettext("{source}, not installed").replace("{source}", &source)
    }
}

/// Whether a copy can be started: installed and still in the library.
fn playable(copy: &GameCopy) -> bool {
    copy.game.installed && copy.game.in_library()
}

/// A copy in fullscreen mode's Preferred Copy list: its source and whether it is
/// installed, with a check on the copy the group shows and Play starts.
pub(crate) fn copy_choice_row(copy: &GameCopy, preferred: bool) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(source_name(copy.game.source()))
        .subtitle(if playable(copy) {
            gettext("Installed")
        } else {
            gettext("Not installed")
        })
        .use_markup(false)
        .activatable(true)
        .build();
    if preferred {
        row.add_suffix(
            &gtk::Image::builder()
                .icon_name("object-select-symbolic")
                .tooltip_text(gettext("Preferred copy"))
                .build(),
        );
    }
    row
}

impl PinduWindow {
    pub(crate) fn setup_group_actions(&self) {
        self.add_game_action("merge-game", Self::ask_merge);
        self.add_game_action("split-group", Self::split_group);
        let pair = glib::VariantTy::new("(ss)").expect("valid type");
        let merge_with = gio::ActionEntry::builder("merge-with")
            .parameter_type(Some(pair))
            .activate(|window: &Self, _, target| {
                let pair = target.and_then(<(String, String)>::from_variant);
                if let Some((game, other)) = pair
                    && let (Ok(game), Ok(other)) = (game.parse(), other.parse())
                {
                    window.merge_games(&game, &other);
                }
            })
            .build();
        // Stateful, so the desktop menu's Preferred Copy items show which copy is chosen.
        let prefer = gio::ActionEntry::builder("prefer-copy")
            .parameter_type(Some(glib::VariantTy::STRING))
            .state(String::new().to_variant())
            .change_state(|window: &Self, action, state| {
                let Some(state) = state else {
                    return;
                };
                if let Some(copy) = state.str().and_then(|id| id.parse().ok()) {
                    action.set_state(state);
                    window.prefer_copy(&copy);
                }
            })
            .build();
        self.add_action_entries([merge_with, prefer]);
    }

    /// Marks `copy` in the Preferred Copy items as the menu opens.
    fn mark_preferred_copy(&self, copy: &GameId) {
        if let Some(action) = self
            .lookup_action("prefer-copy")
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_state(&copy.as_str().to_variant());
        }
    }

    /// Asks which game to merge `id` with: the search dialog on the desktop, the options
    /// sheet's list in fullscreen mode.
    fn ask_merge(&self, id: &GameId) {
        let Some(game) = self.library().game_object(id) else {
            return;
        };
        if self.is_fullscreen() {
            self.open_merge_options(&game);
            return;
        }
        let window = self.downgrade();
        let first = game.id();
        MergeDialog::choose(self, &game, self.library().all_games(), move |other| {
            if let Some(window) = window.upgrade() {
                window.merge_games(&first, &other);
            }
        });
    }

    /// Merges two games, with every copy of their groups, into one game, and says so.
    pub(crate) fn merge_games(&self, game: &GameId, other: &GameId) {
        let titles = (
            self.library().game_title(game).unwrap_or_default(),
            self.library().game_title(other).unwrap_or_default(),
        );
        self.merge_selection(vec![game.clone(), other.clone()], move |_| {
            gettext("“{first}” and “{second}” are now one game")
                .replace("{first}", &titles.0)
                .replace("{second}", &titles.1)
        });
    }

    /// Merges `games` into one game, then shows the toast `done` words for how many there
    /// were and the open game page again with its copies.
    pub(crate) fn merge_selection(
        &self,
        games: Vec<GameId>,
        done: impl FnOnce(usize) -> String + 'static,
    ) {
        let service = self.library_service();
        let window = self.downgrade();
        glib::spawn_future_local(async move {
            let first = games.first().cloned();
            let count = games.len();
            let merged = service.merge_games(games).await;
            let Some(window) = window.upgrade() else {
                return;
            };
            if !merged {
                window.add_toast(adw::Toast::new(&gettext("Could not merge the games")));
                return;
            }
            window.add_toast(
                adw::Toast::builder()
                    .title(done(count))
                    .use_markup(false)
                    .build(),
            );
            if let Some(first) = first {
                window.show_game_page_again(&first);
            }
        });
    }

    /// Splits the group holding `id` into its games, remembering that they are not the
    /// same game, and says so.
    fn split_group(&self, id: &GameId) {
        let service = self.library_service();
        let title = self.library().game_title(id).unwrap_or_default();
        let (window, id) = (self.downgrade(), id.clone());
        glib::spawn_future_local(async move {
            let split = service.split_group(&id).await;
            let Some(window) = window.upgrade() else {
                return;
            };
            let Some(count) = split else {
                window.add_toast(adw::Toast::new(&gettext("Could not split the game")));
                return;
            };
            let message = ngettext(
                "“{title}” split into {n} game",
                "“{title}” split into {n} games",
                count as u32,
            )
            .replace("{title}", &title)
            .replace("{n}", &count.to_string());
            window.add_toast(
                adw::Toast::builder()
                    .title(message)
                    .use_markup(false)
                    .build(),
            );
            window.show_game_page_again(&id);
        });
    }

    /// Makes `copy` the one its group shows and Play starts.
    pub(crate) fn prefer_copy(&self, copy: &GameId) {
        let service = self.library_service();
        let (window, copy) = (self.downgrade(), copy.clone());
        glib::spawn_future_local(async move {
            let chosen = service.set_preferred_copy(&copy).await;
            if let Some(window) = window.upgrade() {
                if chosen {
                    window.show_game_page_again(&copy);
                } else {
                    window.add_toast(adw::Toast::new(&gettext(
                        "Could not choose the preferred copy",
                    )));
                }
            }
        });
    }

    /// Shows the open game page again for the game holding `id`, so it lists the copies
    /// as they now are; nothing when no game page shows.
    fn show_game_page_again(&self, id: &GameId) {
        if self.visible_game_page().is_some() {
            self.show_details(id);
        }
    }
}
