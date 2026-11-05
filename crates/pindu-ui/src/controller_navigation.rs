//! Controller navigation in the main window: directions move focus the way the arrow keys
//! do (`move-focus` on the window), the cover grid steps through its covers with
//! `scroll_to`, A activates the focused widget, B closes a menu or dialog or goes back, Y
//! searches from Home or the library (with the search keyboard in fullscreen mode, which
//! takes the buttons while it is open), LB/RB and LT/RT switch the fullscreen library's
//! tabs and sort order, and Guide enters fullscreen mode. The panels come next: in
//! fullscreen mode Start or Guide toggles the menu drawer, View or X opens the options
//! sheet for the focused game, and B closes them. While the launch overlay is up it takes
//! every button, and a held B cancels a game we start. Intents never become synthetic key
//! events, and only the active window acts on them.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use pindu_core::gamepad::{Direction, Intent, WindowFocus};
use pindu_core::grid;
use pindu_core::hints::Launching;
use pindu_core::menus::{self, Back};

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

    /// Acts on a controller intent, or on a key that stands for one in fullscreen mode
    /// (`pindu_core::keys`).
    pub(crate) fn handle_intent(&self, intent: Intent) {
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
        // The search keyboard takes every button while it is open, Start included.
        if self.search_keyboard_intent(intent) {
            return;
        }
        if self.handle_panel_intent(intent) {
            match intent {
                Intent::Back => self.play_sound(Sound::Back),
                Intent::Menu | Intent::Home | Intent::QuickAccess | Intent::Options => {
                    self.play_sound(Sound::Select)
                }
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
                // In fullscreen mode Y brings up the search keyboard with the entry.
                if self.start_search() && self.is_fullscreen() {
                    self.open_search_keyboard();
                }
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

    /// Moves through the open menu, steps through the cover grid when it has focus, and
    /// moves focus as the arrow keys would everywhere else, including out of the grid at
    /// its edges.
    fn navigate(&self, direction: Direction) {
        let focus_direction = match direction {
            Direction::Up => gtk::DirectionType::Up,
            Direction::Down => gtk::DirectionType::Down,
            Direction::Left => gtk::DirectionType::Left,
            Direction::Right => gtk::DirectionType::Right,
        };
        if let Some(menu) = self.open_menu() {
            // The window's `move-focus` would leave the menu at its ends for the page
            // behind it. Right opens a submenu, Left goes back from its title.
            menu.child_focus(focus_direction);
            return;
        }
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
        self.emit_move_focus(focus_direction);
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
    /// button clicks, a row activates, a menu button opens its menu and a menu item acts.
    fn activate_focus(&self) {
        let Some(focus) = gtk::prelude::GtkWindowExt::focus(self) else {
            return;
        };
        // Items of an open menu lie inside its button too: they activate as they are.
        if self.open_menu().is_none()
            && let Some(button) = focus
                .ancestor(gtk::MenuButton::static_type())
                .and_downcast::<gtk::MenuButton>()
        {
            open_without_grab(&button);
            return;
        }
        if !focus.activate() {
            // Covers are grid items, which activate through their list item action.
            let _ = focus.activate_action("listitem.activate", None);
        }
    }

    /// Steps out of the open submenu or closes the open menu, closes the open dialog,
    /// goes back from a section's games to Home's first screen, or leaves the shown page
    /// for the one before it.
    fn go_back(&self) {
        if let Some(menu) = self.open_menu() {
            let submenu = menu
                .downcast_ref::<gtk::PopoverMenu>()
                .and_then(|menu| menu.property::<Option<String>>("visible-submenu"));
            match (menus::back(submenu.as_deref()), self.submenu_title()) {
                // The title at the top of a submenu goes back to the item that opened it.
                (Back::StepOut, Some(title)) => {
                    title.activate();
                }
                _ => menu.popdown(),
            }
        } else if let Some(dialog) = self.visible_dialog() {
            dialog.close();
        } else if self.shows_home_section() {
            self.imp().home_page.show_sections();
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

    /// The title of the submenu that has focus, which leads back to its parent menu.
    fn submenu_title(&self) -> Option<gtk::Widget> {
        gtk::prelude::GtkWindowExt::focus(self)?
            .ancestor(gtk::Stack::static_type())
            .and_downcast::<gtk::Stack>()?
            .visible_child()?
            .first_child()
            .filter(|title| {
                title.find_property("menu-name").is_some()
                    && title.property::<Option<String>>("menu-name").is_some()
            })
    }
}

/// Opens the button's menu without a grab and gives it focus. The compositor lets a menu
/// grab input only right after a key or pointer press, which a controller press is not,
/// and dismisses a grabbing menu at once otherwise; without a grab, B closes the menu
/// instead of a click outside it. Once closed, the menu grabs again for the pointer and
/// the keyboard.
fn open_without_grab(button: &gtk::MenuButton) {
    if let Some(popover) = button.popover() {
        popover.set_autohide(false);
    }
    button.popup();
    let Some(popover) = button.popover() else {
        return;
    };
    if popover.is_visible() && popover.is_autohide() {
        // The button made a new menu as it opened (the game page's ⋮ menu): open that
        // one again, without a grab.
        popover.popdown();
        popover.set_autohide(false);
        popover.popup();
    }
    popover.child_focus(gtk::DirectionType::TabForward);
    let closed = Rc::new(RefCell::new(None));
    let handler = popover.connect_closed(glib::clone!(
        #[strong]
        closed,
        move |popover| {
            if let Some(handler) = closed.take() {
                popover.disconnect(handler);
            }
            // Changing it unrealizes the menu, which waits until it has finished closing.
            glib::idle_add_local_once(glib::clone!(
                #[weak]
                popover,
                move || popover.set_autohide(true)
            ));
        }
    ));
    closed.replace(Some(handler));
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
