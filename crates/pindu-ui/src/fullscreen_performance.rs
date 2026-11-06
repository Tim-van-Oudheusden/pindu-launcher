//! Fullscreen mode's performance switches for a game we run ourselves
//! (`pindu_core::launch::offers_performance`): Performance Overlay (MangoHud) and GameMode,
//! in Quick Access's Performance and in the game's options sheet. They change the game's
//! own launch settings for its next launch (`Store::set_performance_tool`). The window
//! keeps one state for one game, read once from the store, and every switch shows it, so
//! Quick Access and the sheet always agree. While the game runs, Quick Access's level says
//! a change applies from its next launch.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::model::PerformanceTool;

use crate::models::GameObject;
use crate::window::PinduWindow;

/// The tools in the order the window keeps their state and lists their switches.
const TOOLS: [PerformanceTool; 2] = [PerformanceTool::Overlay, PerformanceTool::GameMode];

impl PinduWindow {
    /// Each switch writes its tool for the game the switches are for.
    pub(crate) fn setup_performance(&self) {
        for (index, rows) in self.performance_rows().into_iter().enumerate() {
            for row in rows {
                row.connect_active_notify(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |row| window.switch_performance(index, row.is_active())
                ));
            }
        }
        // Outside fullscreen mode the game page edits the same settings, so they are read
        // afresh the next time.
        self.connect_fullscreened_notify(|window| {
            if !window.is_fullscreen() {
                window.forget_performance_game();
            }
        });
    }

    /// The switches of each tool in [`TOOLS`]: Quick Access's, then the options sheet's.
    fn performance_rows(&self) -> [[adw::SwitchRow; 2]; 2] {
        let imp = self.imp();
        [
            [
                imp.quick_access_overlay_row.get(),
                imp.options_overlay_row.get(),
            ],
            [
                imp.quick_access_gamemode_row.get(),
                imp.options_gamemode_row.get(),
            ],
        ]
    }

    /// Makes the switches `game`'s, reading its settings unless they are its already, and
    /// names it in Quick Access's heading.
    pub(crate) fn show_performance_for(&self, game: &GameObject) {
        let imp = self.imp();
        let held = imp
            .performance_game
            .borrow()
            .as_ref()
            .is_some_and(|(held, _)| held == game);
        if !held {
            self.forget_performance_game();
            let handler = game.connect_notify_local(
                Some("playing"),
                glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |_, _| window.show_performance_note()
                ),
            );
            imp.performance_game.replace(Some((game.clone(), handler)));
            self.show_performance_tools();
            let (library, id) = (self.library_service(), game.id());
            let window = self.downgrade();
            glib::spawn_future_local(async move {
                let config = library.launch_config(&id).await;
                let Some(window) = window.upgrade() else {
                    return;
                };
                let imp = window.imp();
                let still = imp
                    .performance_game
                    .borrow()
                    .as_ref()
                    .is_some_and(|(game, _)| game.id() == id);
                if still {
                    imp.performance_tools
                        .set(config.map(|config| TOOLS.map(|tool| tool.is_on(&config))));
                    window.show_performance_tools();
                }
            });
        }
        imp.quick_access_performance_title
            .set_label(&gettext("Performance for {game}").replace("{game}", &game.game().title));
        self.show_performance_note();
    }

    /// Lets go of the game the switches were for.
    fn forget_performance_game(&self) {
        let imp = self.imp();
        if let Some((game, handler)) = imp.performance_game.take() {
            game.disconnect(handler);
        }
        imp.performance_tools.set(None);
    }

    /// Shows the game's tools on every switch: off until they are read.
    fn show_performance_tools(&self) {
        let imp = self.imp();
        let tools = imp.performance_tools.get();
        imp.performance_filling.set(true);
        for (index, rows) in self.performance_rows().iter().enumerate() {
            for row in rows {
                row.set_active(tools.is_some_and(|tools| tools[index]));
            }
        }
        imp.performance_filling.set(false);
    }

    /// Quick Access says a change applies from the next launch while the game runs.
    fn show_performance_note(&self) {
        let imp = self.imp();
        let playing = imp
            .performance_game
            .borrow()
            .as_ref()
            .is_some_and(|(game, _)| game.is_playing());
        imp.quick_access_performance_note.set_visible(playing);
    }

    /// A switch of the tool at `index` in [`TOOLS`] turned `on`: writes it for the game
    /// and shows it on the other switch. Before the game's settings are read, the switch
    /// goes back.
    fn switch_performance(&self, index: usize, on: bool) {
        let imp = self.imp();
        if imp.performance_filling.get() {
            return;
        }
        let id = imp
            .performance_game
            .borrow()
            .as_ref()
            .map(|(game, _)| game.id());
        match (imp.performance_tools.get(), id) {
            (Some(mut tools), Some(id)) if tools[index] != on => {
                tools[index] = on;
                imp.performance_tools.set(Some(tools));
                self.library_service()
                    .set_performance_tool(&id, TOOLS[index], on);
            }
            _ => {}
        }
        self.show_performance_tools();
    }
}
