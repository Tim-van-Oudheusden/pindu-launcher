//! Fullscreen mode's on-screen keyboard for the search field: Y opens it in a panel above
//! the hint bar, and while it is open the controller types into the search entry, which
//! keeps focus. The grid and what each button does are `pindu_core::osk`; this draws the
//! keys, marks the cursor's, and applies what the keyboard says to the search entry.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::gamepad::Intent;
use pindu_core::hints::Typing;
use pindu_core::osk::{self, Key, Keyboard, Layer, Outcome};

use crate::fullscreen::Sound;
use crate::window::PinduWindow;

/// The style class that marks the key under the cursor.
pub(crate) const CURSOR_CLASS: &str = "cursor";

impl PinduWindow {
    /// Lays out the keys, and closes the keyboard once the search entry loses focus or the
    /// window leaves fullscreen mode.
    pub(crate) fn setup_search_keyboard(&self) {
        let imp = self.imp();
        let mut keys = Vec::new();
        for (row, cells) in osk::rows().enumerate() {
            let mut labels = Vec::new();
            for cell in cells {
                let label = gtk::Label::builder()
                    .label(key_label(cell.key, false))
                    .css_classes(["osk-key"])
                    .build();
                imp.keyboard_grid.attach(
                    &label,
                    cell.column as i32,
                    row as i32,
                    cell.width as i32,
                    1,
                );
                labels.push(label);
            }
            keys.push(labels);
        }
        imp.keyboard_keys.replace(keys);

        self.connect_focus_widget_notify(|window| {
            let typing = window
                .imp()
                .search_entry
                .state_flags()
                .contains(gtk::StateFlags::FOCUS_WITHIN);
            if !typing {
                window.close_search_keyboard();
            }
        });
        // The keyboard is fullscreen mode's: closed, its revealer still asks for the
        // keyboard's full width (704 px), more than a narrow desktop window has.
        imp.keyboard_revealer.set_visible(self.is_fullscreen());
        self.connect_fullscreened_notify(|window| {
            let fullscreen = window.is_fullscreen();
            if !fullscreen {
                window.close_search_keyboard();
            }
            window.imp().keyboard_revealer.set_visible(fullscreen);
        });
        // B's hint turns from Delete into Close as the field empties, however it does.
        imp.search_entry.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                if window.imp().search_keyboard.get().is_some() {
                    window.update_hints();
                }
            }
        ));
    }

    /// Opens the keyboard on its first letter, over the focused search entry.
    pub(crate) fn open_search_keyboard(&self) {
        let imp = self.imp();
        imp.search_keyboard.set(Some(Keyboard::default()));
        imp.keyboard_revealer.set_reveal_child(true);
        self.show_keyboard_cursor();
        self.update_hints();
    }

    fn close_search_keyboard(&self) {
        let imp = self.imp();
        if imp.search_keyboard.take().is_none() {
            return;
        }
        imp.keyboard_revealer.set_reveal_child(false);
        self.update_hints();
    }

    /// Hands `intent` to the keyboard while it is open, which takes every button then;
    /// `false` when it is closed.
    pub(crate) fn search_keyboard_intent(&self, intent: Intent) -> bool {
        let imp = self.imp();
        let Some(mut keyboard) = imp.search_keyboard.get() else {
            return false;
        };
        let entry = &imp.search_entry;
        let outcome = keyboard.handle(intent, &entry.text());
        imp.search_keyboard.set(Some(keyboard));
        match outcome {
            Outcome::Moved => {
                self.show_keyboard_cursor();
                self.play_sound(Sound::Move);
            }
            Outcome::Edit(text) => {
                entry.set_text(&text);
                entry.set_position(-1);
                self.play_sound(Sound::Select);
            }
            Outcome::Close => {
                self.close_search_keyboard();
                self.focus_page();
                self.play_sound(Sound::Back);
            }
            Outcome::Confirm => {
                // Search now rather than after the entry's typing delay, so focus lands
                // on the first result.
                entry.emit_by_name::<()>("search-changed", &[]);
                self.close_search_keyboard();
                self.focus_page();
                self.play_sound(Sound::Select);
            }
            // The search keyboard has one layer and never hides the text.
            Outcome::Layer | Outcome::Reveal | Outcome::Ignored => {}
        }
        self.update_hints();
        true
    }

    /// The open keyboard's cursor key and whether the search field is empty, for the hints.
    pub(crate) fn search_keyboard_typing(&self) -> Option<Typing> {
        let imp = self.imp();
        imp.search_keyboard.get().map(|keyboard| Typing {
            key: keyboard.key(),
            empty: imp.search_entry.text().is_empty(),
        })
    }

    /// Marks the key under the cursor, and tells screen readers which key it is.
    fn show_keyboard_cursor(&self) {
        let imp = self.imp();
        let Some(keyboard) = imp.search_keyboard.get() else {
            return;
        };
        let cursor = keyboard.position();
        for (row, labels) in imp.keyboard_keys.borrow().iter().enumerate() {
            for (index, label) in labels.iter().enumerate() {
                if (row, index) == cursor {
                    label.add_css_class(CURSOR_CLASS);
                } else {
                    label.remove_css_class(CURSOR_CLASS);
                }
            }
        }
        crate::announcement::announce(
            self,
            &key_name(keyboard.key(), keyboard.hidden()),
            gtk::AccessibleAnnouncementPriority::Medium,
        );
    }
}

/// What a key shows, on a keyboard that hides the text or not.
pub(crate) fn key_label(key: Key, hidden: bool) -> String {
    match key {
        Key::Char(character) => character.to_string(),
        Key::Space => gettext("Space"),
        Key::Backspace => gettext("Delete"),
        Key::Done => gettext("Done"),
        Key::Layer(Layer::Upper) => "ABC".to_owned(),
        Key::Layer(Layer::Symbols) => "#+=".to_owned(),
        Key::Layer(Layer::Lower | Layer::Search) => "abc".to_owned(),
        Key::Reveal if hidden => gettext("Show"),
        Key::Reveal => gettext("Hide"),
    }
}

/// What screen readers say for a key: its label, or the layer a layer key switches to.
pub(crate) fn key_name(key: Key, hidden: bool) -> String {
    match key {
        Key::Layer(Layer::Upper) => gettext("Capitals"),
        Key::Layer(Layer::Symbols) => gettext("Symbols"),
        Key::Layer(Layer::Lower | Layer::Search) => gettext("Lowercase"),
        key => key_label(key, hidden),
    }
}
