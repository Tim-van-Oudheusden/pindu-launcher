//! Controller navigation in the main window: directions move focus the way the arrow keys
//! do (`move-focus` on the window), the cover grid steps through its covers with
//! `scroll_to`, A activates the focused widget, B closes a menu or dialog or goes back, Y
//! searches from Home or the library, LB/RB and LT/RT switch the fullscreen library's tabs
//! and sort order, and Guide enters fullscreen mode. The panels come first: in fullscreen
//! mode Start or Guide toggles the menu drawer, View or X opens the options sheet for the
//! focused game, and B closes them. While the launch overlay is up it takes every button,
//! and a held B cancels a game we start. Intents never become synthetic key events, and
//! only the active window acts on them.

use std::collections::BTreeSet;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use pindu_core::gamepad::{Direction, Intent, WindowFocus};
use pindu_core::grid;
use pindu_core::hints::Launching;

use crate::application::PinduApplication;
use crate::fullscreen::Sound;
use crate::models::GameObject;
use crate::window::PinduWindow;

impl PinduWindow {
    /// Acts on controller intents while controllers are listened to.
    pub(crate) fn setup_controller_navigation(&self, application: &PinduApplication) {
        application.gamepad_service().connect_intent(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |intent| window.handle_intent(intent)
        ));
        self.update_listening();
        self.settings().connect_changed(
            Some("controller-navigation"),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _| window.update_listening()
            ),
        );
    }

    /// Opens the controllers while the `controller-navigation` setting is on or the window
    /// is in fullscreen mode, and lets go of them otherwise.
    pub(crate) fn update_listening(&self) {
        if let Some(application) = self.application().and_downcast::<PinduApplication>() {
            application
                .gamepad_service()
                .set_listening(self.listens_to_controllers());
        }
    }

    fn listens_to_controllers(&self) -> bool {
        self.is_fullscreen() || self.settings().boolean("controller-navigation")
    }

    fn handle_intent(&self, intent: Intent) {
        if !self.takes_controller() || !self.listens_to_controllers() {
            return;
        }
        // As with the keyboard, focus shows once the controller moves it.
        self.set_focus_visible(true);
        if self.launching() != Launching::No {
            if intent == Intent::BackHeld {
                self.cancel_launch();
                self.play_sound(Sound::Back);
            }
            return;
        }
        if self.handle_panel_intent(intent) {
            match intent {
                Intent::Back => self.play_sound(Sound::Back),
                Intent::Menu | Intent::Home | Intent::Options => self.play_sound(Sound::Select),
                _ => {}
            }
            return;
        }
        match intent {
            Intent::Navigate(direction) => {
                self.navigate(direction);
                self.play_sound(Sound::Move);
            }
            Intent::Activate => {
                self.activate_focus();
                self.play_sound(Sound::Select);
            }
            Intent::Back => {
                self.go_back();
                self.play_sound(Sound::Back);
            }
            Intent::Home => self.enter_fullscreen(),
            Intent::Search => {
                self.start_search();
                self.play_sound(Sound::Select);
            }
            Intent::PreviousTab | Intent::NextTab if self.shows_tabs() => {
                if self.step_tab(intent == Intent::NextTab) {
                    self.play_sound(Sound::Move);
                }
            }
            Intent::PreviousSort | Intent::NextSort if self.shows_tabs() => {
                self.cycle_sort(intent == Intent::NextSort);
                self.play_sound(Sound::Move);
            }
            _ => {}
        }
    }

    /// Whether this window has focus, so the controller is its (see `WindowFocus`).
    fn takes_controller(&self) -> bool {
        let compositor_focused = self
            .surface()
            .and_downcast::<gtk::gdk::Toplevel>()
            .is_some_and(|toplevel| toplevel.state().contains(gtk::gdk::ToplevelState::FOCUSED));
        WindowFocus {
            compositor_focused,
            keyboard_focused: self.is_active(),
        }
        .takes_controller()
    }

    /// Steps through the cover grid when it has focus, and moves focus as the arrow keys
    /// would everywhere else, including out of the grid at its edges.
    fn navigate(&self, direction: Direction) {
        let grid = &self.imp().grid;
        let Some(model) = grid.model() else {
            return;
        };
        if let Some(position) = self.focused_grid_position() {
            let columns = grid_columns(grid);
            if let Some(target) = grid::step(position, direction, model.n_items(), columns) {
                self.focus_cover(target);
                return;
            }
            self.imp().grid_cursor.set(position);
        }
        let previous = gtk::prelude::GtkWindowExt::focus(self);
        let in_grid = self.focused_tile().is_some();
        // At its edge the grid would wrap to the next row itself: leave it instead.
        grid.set_can_focus(!in_grid);
        self.emit_move_focus(match direction {
            Direction::Up => gtk::DirectionType::Up,
            Direction::Down => gtk::DirectionType::Down,
            Direction::Left => gtk::DirectionType::Left,
            Direction::Right => gtk::DirectionType::Right,
        });
        grid.set_can_focus(true);
        match gtk::prelude::GtkWindowExt::focus(self) {
            // Nothing lies that way: the window dropped focus, so keep it where it was.
            None => {
                if let Some(previous) = previous {
                    previous.grab_focus();
                }
            }
            // GTK's grid takes focus on whichever cover it likes, often one far out of
            // view: coming in, focus goes to the cover the controller was last on instead.
            Some(_) if !in_grid && self.focused_tile().is_some() && model.n_items() > 0 => {
                let cursor = self.imp().grid_cursor.get().min(model.n_items() - 1);
                self.focus_cover(cursor);
            }
            Some(_) => {}
        }
    }

    /// Scrolls to the cover at `position` and focuses it, remembering it for the shown view.
    pub(crate) fn focus_cover(&self, position: u32) {
        self.imp().grid_cursor.set(position);
        self.remember_cover(position);
        self.imp().grid.scroll_to(
            position,
            gtk::ListScrollFlags::FOCUS | gtk::ListScrollFlags::SELECT,
            None,
        );
    }

    /// The model position of the cover with focus, if a cover has it.
    fn focused_grid_position(&self) -> Option<u32> {
        let game = self.focused_tile()?.game()?;
        let model = self.imp().grid.model()?;
        (0..model.n_items()).find(|&position| {
            model
                .item(position)
                .and_downcast::<GameObject>()
                .is_some_and(|item| item == game)
        })
    }

    /// Activates the focused widget as Enter or Space would: a cover opens its game, a
    /// button clicks, a row activates.
    fn activate_focus(&self) {
        let Some(focus) = gtk::prelude::GtkWindowExt::focus(self) else {
            return;
        };
        if !focus.activate() {
            // Covers are grid items, which activate through their list item action.
            let _ = focus.activate_action("listitem.activate", None);
        }
    }

    /// Closes the open menu or dialog, or leaves the shown page for the one before it.
    fn go_back(&self) {
        if let Some(menu) = self.open_menu() {
            menu.popdown();
        } else if let Some(dialog) = self.visible_dialog() {
            dialog.close();
        } else {
            self.imp().navigation_view.pop();
        }
    }

    /// The menu (a popover) that has focus, if one is open.
    pub(crate) fn open_menu(&self) -> Option<gtk::Popover> {
        gtk::prelude::GtkWindowExt::focus(self)
            .and_then(|focus| focus.ancestor(gtk::Popover::static_type()))
            .and_downcast::<gtk::Popover>()
    }
}

/// How many covers a row of the grid holds: GTK does not say, so count the distinct
/// columns its tiles sit in, by their centres, which fullscreen mode's lift of the focused
/// cover (a scale about its centre) leaves in place.
fn grid_columns(grid: &gtk::GridView) -> u32 {
    let mut columns = BTreeSet::new();
    let mut child = grid.first_child();
    while let Some(widget) = child {
        if widget.is_child_visible()
            && let Some(bounds) = widget.compute_bounds(grid)
        {
            columns.insert((bounds.x() + bounds.width() / 2.0).round() as i32);
        }
        child = widget.next_sibling();
    }
    columns.len().max(1) as u32
}
