//! Fullscreen mode: the same window and models, presented for a controller at a distance.
//! The window goes fullscreen and the mode follows its `fullscreened` property: top bars
//! hide, type grows, controllers are listened to, focus is kept on the page, and Home is
//! the first page, with the library and game pages above it. Entering and leaving go
//! through `win.fullscreen-mode` (main menu, F11, `--fullscreen`, Guide) and
//! `win.leave-fullscreen` (Esc, and the menu drawer's "Exit Fullscreen Mode").

use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::sound::tone_wav;

use crate::models::GameObject;
use crate::window::PinduWindow;

/// The window's style class in fullscreen mode, which `style.css` scales type up with.
const STYLE_CLASS: &str = "fullscreen-mode";

impl PinduWindow {
    pub(crate) fn setup_fullscreen(&self) {
        let mode = gio::SimpleAction::new_stateful("fullscreen-mode", None, &false.to_variant());
        mode.connect_change_state(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, state| {
                if state.and_then(bool::from_variant) == Some(true) {
                    window.enter_fullscreen();
                } else {
                    window.leave_fullscreen();
                }
            }
        ));
        self.add_action(&mode);
        let leave = gio::ActionEntry::builder("leave-fullscreen")
            .activate(|window: &Self, _, _| window.leave_fullscreen())
            .build();
        self.add_action_entries([leave]);
        // Esc leaves fullscreen mode once nothing nearer the focus (a dialog, the search
        // entry) took it; F11 is an application accelerator.
        let escape = gtk::ShortcutController::new();
        escape.set_propagation_phase(gtk::PropagationPhase::Bubble);
        escape.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::NamedAction::new("win.leave-fullscreen")),
        ));
        self.add_controller(escape);

        self.connect_fullscreened_notify(|window| window.show_fullscreen_mode());
        // Pages pushed later get their bars hidden too.
        self.imp()
            .navigation_view
            .connect_visible_page_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.update_top_bars()
            ));
        self.connect_focus_widget_notify(|window| {
            if !window.is_fullscreen() {
                return;
            }
            // The library's top bar shows while its search entry has focus.
            window.update_top_bars();
            if gtk::prelude::GtkWindowExt::focus(window).is_none() {
                // Wait for the change that dropped focus (a closing dialog, a hidden bar)
                // to finish before taking it back.
                glib::idle_add_local_once(glib::clone!(
                    #[weak]
                    window,
                    move || window.focus_page()
                ));
            }
        });
        self.show_fullscreen_mode();
    }

    /// Enters fullscreen mode; the window follows once the compositor makes it fullscreen.
    pub fn enter_fullscreen(&self) {
        self.fullscreen();
    }

    /// Leaves fullscreen mode. The menu drawer's "Exit Fullscreen Mode" calls this through
    /// `win.leave-fullscreen`.
    pub fn leave_fullscreen(&self) {
        self.unfullscreen();
    }

    /// Shows the window in fullscreen mode or as the desktop window, as it is fullscreen.
    fn show_fullscreen_mode(&self) {
        let fullscreen = self.is_fullscreen();
        if fullscreen {
            self.add_css_class(STYLE_CLASS);
        } else {
            self.remove_css_class(STYLE_CLASS);
        }
        self.update_listening();
        self.arrange_pages(fullscreen);
        self.show_sidebar_beside_library();
        self.update_top_bars();
        if let Some(action) = self
            .lookup_action("fullscreen-mode")
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_state(&fullscreen.to_variant());
        }
        if let Some(action) = self
            .lookup_action("leave-fullscreen")
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(fullscreen);
        }
        if fullscreen {
            self.set_focus_visible(true);
            // Focus in a bar that is now hidden goes to the page.
            let focus = gtk::prelude::GtkWindowExt::focus(self);
            if focus.is_none_or(|focus| focus.ancestor(adw::HeaderBar::static_type()).is_some()) {
                self.focus_page();
            }
            if self.settings().boolean("navigation-sounds") {
                self.imp().sounds.get_or_init(NavigationSounds::default);
            }
        }
    }

    /// Home: its carousel follows the "Jump back in" games, its Library tile goes to the
    /// library (`win.show-library`), and the sidebar shows only beside the library.
    pub(crate) fn setup_home(&self) {
        let show_library = gio::ActionEntry::builder("show-library")
            .activate(|window: &Self, _, _| window.show_library())
            .build();
        self.add_action_entries([show_library]);
        // In fullscreen mode the library opens on the cover the controller was last on,
        // rather than on the widget that had focus when the page was last left, unless
        // Y brought it up to search.
        self.imp().library_page.connect_shown(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                let searching = window
                    .imp()
                    .search_entry
                    .state_flags()
                    .contains(gtk::StateFlags::FOCUS_WITHIN);
                if window.is_fullscreen() && !searching {
                    // After the page has given focus back to where it was left.
                    glib::idle_add_local_once(glib::clone!(
                        #[weak]
                        window,
                        move || window.focus_page()
                    ));
                }
            }
        ));
        self.library().connect_counts_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.update_home()
        ));
        self.imp()
            .navigation_view
            .connect_visible_page_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.show_sidebar_beside_library()
            ));
        self.update_home();
    }

    fn update_home(&self) {
        self.imp()
            .home_page
            .set_games(&self.library().jump_back_in());
    }

    /// Shows the library page: back down to it, or up from Home to it.
    pub(crate) fn show_library(&self) {
        let imp = self.imp();
        let library = imp.library_page.get();
        let in_stack = imp
            .navigation_view
            .navigation_stack()
            .iter::<adw::NavigationPage>()
            .flatten()
            .any(|page| page == library);
        if in_stack {
            imp.navigation_view.pop_to_page(&library);
        } else {
            imp.navigation_view.push(&library);
        }
    }

    /// Fullscreen mode opens on Home, which the library and game pages go above; the
    /// desktop window keeps the library at the bottom, under any game page.
    fn arrange_pages(&self, fullscreen: bool) {
        let imp = self.imp();
        let home = imp.home_page.get().upcast::<adw::NavigationPage>();
        let library = imp.library_page.get();
        let stack: Vec<adw::NavigationPage> = imp
            .navigation_view
            .navigation_stack()
            .iter::<adw::NavigationPage>()
            .flatten()
            .collect();
        if fullscreen {
            if stack.first() != Some(&home) {
                imp.navigation_view.replace(&[home]);
            }
        } else if stack.contains(&home) {
            let pages: Vec<adw::NavigationPage> = std::iter::once(library.clone())
                .chain(
                    stack
                        .into_iter()
                        .filter(|page| *page != home && *page != library),
                )
                .collect();
            imp.navigation_view.replace(&pages);
        }
    }

    /// Home and the pages opened from it fill the window in fullscreen mode: the sidebar,
    /// which picks the library's view, hides unless the library shows, and comes back as
    /// it was after.
    fn show_sidebar_beside_library(&self) {
        let imp = self.imp();
        let split_view = &imp.split_view;
        let hide = self.is_fullscreen()
            && imp
                .navigation_view
                .visible_page()
                .is_some_and(|page| page != *imp.library_page);
        if hide {
            if imp.sidebar_before_hiding.get().is_none() {
                imp.sidebar_before_hiding
                    .set(Some(split_view.shows_sidebar()));
                split_view.set_show_sidebar(false);
            }
        } else if let Some(shown) = imp.sidebar_before_hiding.take() {
            split_view.set_show_sidebar(shown);
        }
    }

    /// Y and Ctrl+F: search the library from Home or the library page, in the search
    /// entry for now.
    pub(crate) fn start_search(&self) {
        if self.visible_dialog().is_some() {
            return;
        }
        let imp = self.imp();
        let page = imp.navigation_view.visible_page();
        if page.as_ref() == Some(imp.home_page.upcast_ref()) {
            self.show_library();
        } else if page.as_ref() != Some(&*imp.library_page) {
            return;
        }
        self.focus_search_entry();
    }

    /// Focuses the search entry, showing its top bar meanwhile in fullscreen mode.
    pub(crate) fn focus_search_entry(&self) {
        let imp = self.imp();
        if let Some(toolbar) = imp.library_page.child().and_downcast::<adw::ToolbarView>() {
            toolbar.set_reveal_top_bars(true);
        }
        imp.search_entry.grab_focus();
    }

    /// Reveals the top bars of the sidebar and of every page in the desktop window. In
    /// fullscreen mode they hide, except the library's while its search entry has focus.
    fn update_top_bars(&self) {
        let imp = self.imp();
        let reveal = !self.is_fullscreen();
        imp.sidebar_toolbar.set_reveal_top_bars(reveal);
        let searching = imp
            .search_entry
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_WITHIN);
        let pages = imp.navigation_view.navigation_stack();
        for page in pages.iter::<adw::NavigationPage>().flatten() {
            if let Some(toolbar) = page.child().and_downcast::<adw::ToolbarView>() {
                toolbar.set_reveal_top_bars(reveal || (page == *imp.library_page && searching));
            }
        }
    }

    /// Gives focus to the shown page: Home's card that had focus last, the library's grid
    /// on the cover the controller was last on, or else the first item that takes focus.
    pub(crate) fn focus_page(&self) {
        let imp = self.imp();
        let Some(page) = imp.navigation_view.visible_page() else {
            return;
        };
        if page == *imp.home_page.upcast_ref::<adw::NavigationPage>() {
            imp.home_page.focus_remembered();
            return;
        }
        let grid_shown = imp.content_stack.visible_child_name().as_deref() == Some("grid");
        if page == *imp.library_page
            && grid_shown
            && let Some(model) = imp.grid.model().filter(|model| model.n_items() > 0)
        {
            // A grid that does not have focus yet does not take it from `scroll_to`.
            imp.grid.grab_focus();
            self.focus_cover(imp.grid_cursor.get().min(model.n_items() - 1));
            return;
        }
        page.child_focus(gtk::DirectionType::TabForward);
    }

    /// Notes the cover at `position` as the one to come back to in the shown view.
    pub(crate) fn remember_cover(&self, position: u32) {
        let game = self
            .imp()
            .grid
            .model()
            .and_then(|model| model.item(position))
            .and_downcast::<GameObject>();
        if let Some(game) = game {
            let view = self.library().view();
            self.imp()
                .focus_memory
                .borrow_mut()
                .remember(view, game.id());
        }
    }

    /// The cover to come back to in the view now shown: the one last visited there.
    pub(crate) fn recall_cover(&self) -> u32 {
        let Some(model) = self.imp().grid.model() else {
            return 0;
        };
        let ids: Vec<_> = (0..model.n_items())
            .filter_map(|position| model.item(position).and_downcast::<GameObject>())
            .map(|game| game.id())
            .collect();
        let view = self.library().view();
        let position = self.imp().focus_memory.borrow().position(&view, &ids);
        position as u32
    }

    /// Plays a navigation sound in fullscreen mode, when the user has them on.
    pub(crate) fn play_sound(&self, sound: Sound) {
        if !self.is_fullscreen() || !self.settings().boolean("navigation-sounds") {
            return;
        }
        self.imp()
            .sounds
            .get_or_init(NavigationSounds::default)
            .play(sound);
    }
}

/// What a navigation sound answers.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Sound {
    Move,
    Select,
    Back,
}

/// Fullscreen mode's sounds: short tones made at start, played through GTK's media stream.
#[derive(Debug)]
pub struct NavigationSounds {
    tones: [glib::Bytes; 3],
    /// The sound playing, kept alive until the next one.
    playing: RefCell<Option<gtk::MediaFile>>,
}

impl Default for NavigationSounds {
    fn default() -> Self {
        Self {
            tones: [
                glib::Bytes::from_owned(tone_wav(660.0, 30)),
                glib::Bytes::from_owned(tone_wav(880.0, 60)),
                glib::Bytes::from_owned(tone_wav(440.0, 60)),
            ],
            // An empty stream loads GTK's media backend now: loading it on the first
            // sound stalls input for a moment.
            playing: RefCell::new(Some(gtk::MediaFile::new())),
        }
    }
}

impl NavigationSounds {
    fn play(&self, sound: Sound) {
        let tone = &self.tones[sound as usize];
        let stream = gio::MemoryInputStream::from_bytes(tone);
        let media = gtk::MediaFile::for_input_stream(&stream);
        media.connect_error_notify(|media| {
            if let Some(error) = media.error() {
                glib::g_warning!("pindu", "Could not play a navigation sound: {error}");
            }
        });
        media.play();
        self.playing.replace(Some(media));
    }
}
