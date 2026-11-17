//! Fullscreen mode's performance switches for a game we run ourselves
//! (`pindu_core::launch::offers_performance`): Performance Overlay (MangoHud) and GameMode,
//! in Quick Access's Performance and in the game's options sheet. They change the game's
//! own launch settings for its next launch (`Store::set_performance_tool`).
//! [`PerformanceSwitches`] keeps one state for one game, read once from the store, and
//! every switch attached to it shows that state, so Quick Access and the sheet always
//! agree. While the game runs, Quick Access's level says a change applies from its next
//! launch. Where MangoHud cannot work for the game (`permission_service::mangohud_works`),
//! its switches are insensitive and say why.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::model::PerformanceTool;

use crate::library_service::LibraryService;
use crate::models::GameObject;
use crate::permission_service;

/// The tools in the order the switches keep their state and list their switches.
const TOOLS: [PerformanceTool; 2] = [PerformanceTool::Overlay, PerformanceTool::GameMode];

/// The performance switches of one game: Quick Access's, and any others attached, all
/// showing one state.
#[derive(Debug, Clone)]
pub struct PerformanceSwitches(Rc<Inner>);

#[derive(Debug)]
struct Inner {
    library: LibraryService,
    /// The switches of each tool in [`TOOLS`], one pair per place that shows them.
    rows: RefCell<Vec<[adw::SwitchRow; 2]>>,
    /// The note that a change applies from the next launch, shown while the game runs.
    note: gtk::Label,
    /// The game the switches are for, and its `notify::playing` handler.
    game: RefCell<Option<(GameObject, glib::SignalHandlerId)>>,
    /// That game's Performance Overlay and GameMode as stored, which every switch shows;
    /// `None` until they are read, or for a game without launch settings.
    tools: Cell<Option<[bool; 2]>>,
    /// Whether the switches are being set to show those, rather than by the user.
    filling: Cell<bool>,
}

impl PerformanceSwitches {
    /// Switches that write through `library`, with `note` shown while their game runs.
    pub(crate) fn new(library: &LibraryService, note: &gtk::Label) -> Self {
        Self(Rc::new(Inner {
            library: library.clone(),
            rows: RefCell::new(Vec::new()),
            note: note.clone(),
            game: RefCell::new(None),
            tools: Cell::new(None),
            filling: Cell::new(false),
        }))
    }

    /// Adds a Performance Overlay and a GameMode switch, which show the game's state and
    /// write it when turned.
    pub fn attach(&self, overlay: &adw::SwitchRow, gamemode: &adw::SwitchRow) {
        for (index, row) in [overlay, gamemode].into_iter().enumerate() {
            let weak = Rc::downgrade(&self.0);
            row.connect_active_notify(move |row| {
                if let Some(inner) = weak.upgrade() {
                    Self(inner).switch(index, row.is_active());
                }
            });
        }
        self.0
            .rows
            .borrow_mut()
            .push([overlay.clone(), gamemode.clone()]);
        self.show_tools();
    }

    /// Makes the switches `game`'s, reading its settings unless they are its already.
    pub fn show_for(&self, game: &GameObject) {
        let inner = &self.0;
        let held = inner
            .game
            .borrow()
            .as_ref()
            .is_some_and(|(held, _)| held == game);
        if !held {
            self.forget();
            let weak = Rc::downgrade(inner);
            let handler = game.connect_notify_local(Some("playing"), move |_, _| {
                if let Some(inner) = weak.upgrade() {
                    Self(inner).show_note();
                }
            });
            inner.game.replace(Some((game.clone(), handler)));
            self.show_tools();
            let (library, id) = (inner.library.clone(), game.id());
            let weak: Weak<Inner> = Rc::downgrade(inner);
            glib::spawn_future_local(async move {
                let config = library.launch_config(&id).await;
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let still = inner
                    .game
                    .borrow()
                    .as_ref()
                    .is_some_and(|(game, _)| game.id() == id);
                if still {
                    inner
                        .tools
                        .set(config.map(|config| TOOLS.map(|tool| tool.is_on(&config))));
                    Self(inner).show_tools();
                }
            });
        }
        let works = permission_service::mangohud_works(&game.game());
        for [overlay, _] in inner.rows.borrow().iter() {
            show_overlay_works(overlay, works);
        }
        self.show_note();
    }

    /// Lets go of the game the switches were for: its settings are read afresh the next
    /// time, as the desktop's game page edits them too.
    pub fn forget(&self) {
        if let Some((game, handler)) = self.0.game.take() {
            game.disconnect(handler);
        }
        self.0.tools.set(None);
    }

    /// Shows the game's tools on every switch: off until they are read.
    fn show_tools(&self) {
        let inner = &self.0;
        let tools = inner.tools.get();
        inner.filling.set(true);
        for rows in inner.rows.borrow().iter() {
            for (index, row) in rows.iter().enumerate() {
                row.set_active(tools.is_some_and(|tools| tools[index]));
            }
        }
        inner.filling.set(false);
    }

    /// Quick Access says a change applies from the next launch while the game runs.
    fn show_note(&self) {
        let playing = self
            .0
            .game
            .borrow()
            .as_ref()
            .is_some_and(|(game, _)| game.is_playing());
        self.0.note.set_visible(playing);
    }

    /// A switch of the tool at `index` in [`TOOLS`] turned `on`: writes it for the game
    /// and shows it on the other switches. Before the game's settings are read, the switch
    /// goes back.
    fn switch(&self, index: usize, on: bool) {
        let inner = &self.0;
        if inner.filling.get() {
            return;
        }
        let id = inner.game.borrow().as_ref().map(|(game, _)| game.id());
        match (inner.tools.get(), id) {
            (Some(mut tools), Some(id)) if tools[index] != on => {
                tools[index] = on;
                inner.tools.set(Some(tools));
                inner.library.set_performance_tool(&id, TOOLS[index], on);
            }
            _ => {}
        }
        self.show_tools();
    }
}

/// A Performance Overlay switch says MangoHud is not installed where it cannot work for
/// the game. The stored setting stays, so it works again once it is installed.
fn show_overlay_works(row: &adw::SwitchRow, works: bool) {
    row.set_sensitive(works);
    row.set_subtitle(&if works {
        gettext("MangoHud")
    } else {
        gettext("MangoHud is not installed")
    });
}
