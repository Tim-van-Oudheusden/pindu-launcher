//! The menu drawer, fullscreen mode's panel on the left, as a widget a window hosts:
//! [`PinduMenuDrawer`], with its own Blueprint (`data/ui/menu_drawer.blp`). It holds the
//! level stack: the main level with the Playing Now cards (`fullscreen_playing`) and the
//! entries, the collections, Controls (`fullscreen_bindings`) and Power
//! (`fullscreen_power`).
//!
//! Which level is open is the host's (`pindu_core::panels::Panels`): the host shows a
//! level with `show_level` and fills the entries with `fill`; the rows' `win.*` actions
//! are the host's. The main window hosts it on the left, where Start or Guide opens it over
//! the page. Over a running game in a `--steam` gamescope the overlay window
//! (`game_overlay`) hosts it too, where Guide opens it over the game and it leaves out
//! what would end the session (`set_over_game`).

use std::cell::OnceCell;

use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use gtk::prelude::*;
use pindu_core::bindings::{FUNCTIONS, function_name};
use pindu_core::gamepad::{Intent, Layout};
use pindu_core::gamescope::Xid;
use pindu_core::hints;
use pindu_core::model::Collection;
use pindu_core::panels::{DrawerEntry, DrawerLevel, drawer_entries};

use crate::fullscreen_controls::action_label;
use crate::fullscreen_panels::drawer_row;
use crate::fullscreen_playing::PlayingCards;
use crate::fullscreen_power::PinduPowerLevel;
use crate::hint_bar::{GLYPH_SIZE, glyph_label};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/menu_drawer.ui")]
    pub struct PinduMenuDrawer {
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub playing_cards: TemplateChild<gtk::Box>,
        #[template_child]
        pub windows_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub windows: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub collections_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub controls_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub exit_fullscreen_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub quit_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub power_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub collections: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub bindings: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub power_level: TemplateChild<PinduPowerLevel>,
        /// The Playing Now cards over the main level's entries.
        pub(crate) cards: OnceCell<PlayingCards>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduMenuDrawer {
        const NAME: &'static str = "PinduMenuDrawer";
        type Type = super::PinduMenuDrawer;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            PinduPowerLevel::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduMenuDrawer {
        fn constructed(&self) {
            self.parent_constructed();
            // The windows sit between the cards and the entries. Up from the entries goes
            // to the last window first; this runs before the cards' own handler, which
            // takes Up to the nearest card's Return.
            self.list.connect_keynav_failed(glib::clone!(
                #[weak(rename_to = windows)]
                self.windows,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, direction| {
                    let last = windows.is_visible().then(|| windows.last_child()).flatten();
                    match last {
                        Some(row) if direction == gtk::DirectionType::Up => {
                            row.grab_focus();
                            glib::Propagation::Stop
                        }
                        _ => glib::Propagation::Proceed,
                    }
                }
            ));
            let _ = self
                .cards
                .set(PlayingCards::new(&self.playing_cards, &self.list));
            // Up from the first window goes to the nearest card's Return, Down from the last
            // to the entries.
            self.windows.connect_keynav_failed(glib::clone!(
                #[weak(rename_to = cards)]
                self.playing_cards,
                #[weak(rename_to = list)]
                self.list,
                #[upgrade_or]
                glib::Propagation::Stop,
                move |_, direction| {
                    let target = match direction {
                        gtk::DirectionType::Up => cards
                            .last_child()
                            .as_ref()
                            .and_then(crate::fullscreen_playing::card_return),
                        gtk::DirectionType::Down => list.row_at_index(0).map(|row| row.upcast()),
                        _ => None,
                    };
                    if let Some(target) = target {
                        target.grab_focus();
                    }
                    glib::Propagation::Stop
                }
            ));
        }
    }

    impl WidgetImpl for PinduMenuDrawer {}

    impl BoxImpl for PinduMenuDrawer {}
}

glib::wrapper! {
    /// Fullscreen mode's menu drawer: the Playing Now cards, Home, Library, Collections,
    /// Controls, leaving fullscreen mode, Quit and Power, with Collections, Controls and
    /// Power as levels of their own.
    pub struct PinduMenuDrawer(ObjectSubclass<imp::PinduMenuDrawer>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl PinduMenuDrawer {
    /// The drawer's Playing Now cards.
    pub(crate) fn cards(&self) -> &PlayingCards {
        self.imp().cards.get().expect("made as the drawer is built")
    }

    /// Leaves out the entries that would leave fullscreen mode or end the session, for the
    /// drawer over a running game: Exit Fullscreen Mode, Quit and Power. Controls goes too:
    /// the game would take the buttons pressed to bind.
    pub(crate) fn set_over_game(&self) {
        let imp = self.imp();
        imp.controls_row.set_visible(false);
        imp.exit_fullscreen_row.set_visible(false);
        imp.quit_row.set_visible(false);
        imp.power_row.set_visible(false);
    }

    /// Shows the entries that apply: Collections while there are `collections`, whose
    /// level lists them, and Power while `power` says the system offers it.
    pub(crate) fn fill(&self, collections: &[Collection], power: bool) {
        let imp = self.imp();
        let entries = drawer_entries(false, !collections.is_empty(), power);
        imp.collections_row
            .set_visible(entries.contains(&DrawerEntry::Collections));
        imp.power_row
            .set_visible(entries.contains(&DrawerEntry::Power));
        imp.collections.remove_all();
        for collection in collections {
            let row = drawer_row("folder-symbolic", &collection.name);
            row.set_action_name(Some("win.drawer-collection"));
            row.set_action_target_value(Some(&collection.id.to_variant()));
            imp.collections.append(&row);
        }
    }

    /// Lists `windows`, the windows of the game in front with their titles, when it has
    /// more than one, each leading to `win.game-window` with its window; hides the list
    /// otherwise.
    pub(crate) fn show_windows(&self, windows: &[(Xid, Option<String>)]) {
        let imp = self.imp();
        imp.windows.remove_all();
        let shown = windows.len() > 1;
        imp.windows_title.set_visible(shown);
        imp.windows.set_visible(shown);
        if !shown {
            return;
        }
        for (xid, title) in windows {
            let untitled = gettext("Untitled Window");
            let row = drawer_row(
                "focus-windows-symbolic",
                title.as_deref().unwrap_or(&untitled),
            );
            row.set_action_name(Some("win.game-window"));
            row.set_action_target_value(Some(&xid.to_variant()));
            imp.windows.append(&row);
        }
    }

    /// Lists each function a button can be bound to with its button on `layout`, or
    /// "Press a button…" for the one `waiting` for a press, each leading to
    /// `win.bind-button` with its name, then Reset to Defaults (`win.reset-bindings`).
    /// Focus stays on the row it was on.
    pub(crate) fn fill_bindings(&self, layout: Layout, waiting: Option<Intent>) {
        let list = &self.imp().bindings;
        let focused = list
            .focus_child()
            .filter(|row| row.has_focus())
            .and_downcast::<gtk::ListBoxRow>()
            .map(|row| row.index());
        list.remove_all();
        for function in FUNCTIONS {
            let Some(name) = function_name(function) else {
                continue;
            };
            let content = gtk::Box::builder().spacing(16).build();
            content.append(
                &gtk::Label::builder()
                    .label(action_label(function))
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build(),
            );
            if waiting == Some(function) {
                // Translators: in the menu drawer's Controls, in place of a function's
                // button while it waits for the button to bind the function to.
                let waiting = gettext("Press a button…");
                let label = gtk::Label::builder()
                    .label(waiting)
                    .css_classes(["dim-label"])
                    .build();
                content.append(&label);
            } else {
                let button = layout.button(function);
                let glyph = gtk::Image::builder()
                    .icon_name(hints::glyph(layout.glyphs, button))
                    .pixel_size(GLYPH_SIZE)
                    .css_classes(["control-glyph"])
                    .build();
                let label = glyph_label(hints::glyph_name(layout.glyphs, button));
                glyph.update_property(&[gtk::accessible::Property::Label(&label)]);
                content.append(&glyph);
            }
            let row = gtk::ListBoxRow::builder().child(&content).build();
            row.set_action_name(Some("win.bind-button"));
            row.set_action_target_value(Some(&name.to_variant()));
            list.append(&row);
        }
        let reset = drawer_row("edit-undo-symbolic", &gettext("Reset to Defaults"));
        reset.set_action_name(Some("win.reset-bindings"));
        list.append(&reset);
        if let Some(row) = focused.and_then(|index| list.row_at_index(index)) {
            row.grab_focus();
        }
    }

    /// Shows `level`.
    pub(crate) fn show_level(&self, level: DrawerLevel) {
        self.imp().stack.set_visible_child_name(match level {
            DrawerLevel::Main => "main",
            DrawerLevel::Collections => "collections",
            DrawerLevel::Controls => "controls",
            DrawerLevel::Power => "power",
        });
    }

    /// Where focus goes as `level` shows, after `left` (the level focus comes from): the
    /// entry that led to a level just left, the first Playing Now card's Return, or the
    /// level's first entry. Power's own focus is its host's.
    pub(crate) fn focus_target(
        &self,
        level: DrawerLevel,
        left: Option<DrawerLevel>,
    ) -> Option<gtk::Widget> {
        let imp = self.imp();
        let first_row = |list: &gtk::ListBox| list.row_at_index(0).map(|row| row.upcast());
        match (level, left) {
            (DrawerLevel::Main, Some(DrawerLevel::Collections)) => {
                Some(imp.collections_row.get().upcast())
            }
            (DrawerLevel::Main, Some(DrawerLevel::Power)) => Some(imp.power_row.get().upcast()),
            (DrawerLevel::Main, Some(DrawerLevel::Controls)) => {
                Some(imp.controls_row.get().upcast())
            }
            (DrawerLevel::Main, _) if imp.playing_cards.is_visible() => imp
                .playing_cards
                .first_child()
                .as_ref()
                .and_then(crate::fullscreen_playing::card_return),
            (DrawerLevel::Main, _) => first_row(&imp.list),
            (DrawerLevel::Collections, _) => first_row(&imp.collections),
            (DrawerLevel::Controls, _) => first_row(&imp.bindings),
            (DrawerLevel::Power, _) => imp.power_level.focus_target(),
        }
    }

    /// The drawer's Power level.
    pub(crate) fn power_level(&self) -> &PinduPowerLevel {
        &self.imp().power_level
    }

    /// The drawer's Power entry.
    pub(crate) fn power_row(&self) -> &gtk::ListBoxRow {
        &self.imp().power_row
    }
}
