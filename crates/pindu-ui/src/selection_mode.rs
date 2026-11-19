//! Selection mode in the desktop library: the header bar's Select button turns it on,
//! activating a game in the grid or the list then selects it, and the action bar below
//! favourites, hides or files every selected game at once, with a toast to undo it, or
//! merges two or more into one game with several copies (`group_actions`).
//! Escape, an action or fullscreen mode turns it off again.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gdk, gio, glib};
use pindu_core::bulk::{BulkAction, Offers};
use pindu_core::model::GameId;

use crate::models::GameObject;
use crate::window::PinduWindow;

impl PinduWindow {
    /// The Select button, Escape, the action bar and the `win.bulk-*` actions it uses.
    pub(crate) fn setup_selection_mode(&self) {
        let imp = self.imp();
        imp.select_button.connect_toggled(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |button| window.imp().selection.set_active(button.is_active())
        ));
        imp.selection.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.show_selection()
        ));
        // A reload after an action or its undo changes what the action bar offers.
        self.library().connect_counts_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                if window.imp().selection.is_active() {
                    window.show_selection();
                }
            }
        ));

        imp.select_all_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                let model = window.library().model();
                let shown = (0..model.n_items())
                    .filter_map(|position| model.item(position).and_downcast::<GameObject>())
                    .map(|game| game.id());
                window.imp().selection.select_all(shown);
            }
        ));
        imp.favourite_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.apply_bulk(window.offers().favourite)
        ));
        imp.hide_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.apply_bulk(window.offers().hide)
        ));
        imp.merge_selection_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.merge_selected()
        ));
        let add = gio::ActionEntry::builder("bulk-add-to-collection")
            .parameter_type(Some(glib::VariantTy::INT64))
            .activate(|window: &Self, _, target| {
                if let Some(collection) = target.and_then(i64::from_variant) {
                    window.apply_bulk(BulkAction::AddToCollection(collection));
                }
            })
            .build();
        let remove = gio::ActionEntry::builder("bulk-remove-from-collection")
            .parameter_type(Some(glib::VariantTy::INT64))
            .activate(|window: &Self, _, target| {
                if let Some(collection) = target.and_then(i64::from_variant) {
                    window.apply_bulk(BulkAction::RemoveFromCollection(collection));
                }
            })
            .build();
        self.add_action_entries([add, remove]);

        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                let selection = &window.imp().selection;
                if key != gdk::Key::Escape || !selection.is_active() {
                    return glib::Propagation::Proceed;
                }
                selection.set_active(false);
                glib::Propagation::Stop
            }
        ));
        imp.library_page.add_controller(keys);

        self.connect_fullscreened_notify(|window| {
            let imp = window.imp();
            let fullscreen = window.is_fullscreen();
            imp.select_button.set_visible(!fullscreen);
            if fullscreen {
                imp.selection.set_active(false);
            }
        });
        self.show_selection();
    }

    /// Activating a game in the grid or the list: in selection mode it selects or
    /// deselects the game, otherwise it opens the game's page.
    pub(crate) fn activate_game(&self, id: &GameId) {
        let selection = &self.imp().selection;
        if selection.is_active() {
            selection.toggle(id);
        } else {
            self.show_details(id);
        }
    }

    /// The selected games the library still has.
    fn selected_games(&self) -> Vec<GameObject> {
        self.imp()
            .selection
            .games()
            .iter()
            .filter_map(|id| self.library().game_object(id))
            .collect()
    }

    /// What the action bar offers for the selected games.
    fn offers(&self) -> Offers {
        let games = self.selected_games();
        let entries: Vec<_> = games.iter().map(GameObject::entry).collect();
        Offers::of(
            entries
                .iter()
                .map(|entry| (&entry.game, entry.collections.as_slice())),
            // Saved filters pick their games themselves.
            &self.library().manual_collections(),
        )
    }

    /// The Select button and the action bar as selection mode and its games are.
    fn show_selection(&self) {
        let imp = self.imp();
        let active = imp.selection.is_active();
        imp.select_button.set_active(active);
        imp.selection_bar.set_revealed(active);
        let count = self.selected_games().len();
        let some = count > 0;
        imp.selection_count.set_label(
            &ngettext(
                "{count} game selected",
                "{count} games selected",
                count_u32(count),
            )
            .replace("{count}", &count.to_string()),
        );
        let offers = self.offers();
        imp.favourite_button
            .set_label(&if offers.favourite == BulkAction::Unfavourite {
                gettext("_Unfavourite")
            } else {
                gettext("_Favourite")
            });
        imp.hide_button
            .set_label(&if offers.hide == BulkAction::Unhide {
                gettext("_Unhide")
            } else {
                gettext("_Hide")
            });
        imp.favourite_button.set_sensitive(some);
        imp.hide_button.set_sensitive(some);
        imp.merge_selection_button.set_sensitive(count > 1);

        let collections = self.library().manual_collections();
        let name = |id: &i64| {
            collections
                .iter()
                .find(|collection| collection.id == *id)
                .map(|collection| collection.name.clone())
                .unwrap_or_default()
        };
        let (add, remove) = (gio::Menu::new(), gio::Menu::new());
        for id in &offers.add_to {
            let label = gettext("Add to “{collection}”").replace("{collection}", &name(id));
            let item = gio::MenuItem::new(Some(&label), None);
            item.set_action_and_target_value(
                Some("win.bulk-add-to-collection"),
                Some(&id.to_variant()),
            );
            add.append_item(&item);
        }
        for id in &offers.remove_from {
            let label = gettext("Remove from “{collection}”").replace("{collection}", &name(id));
            let item = gio::MenuItem::new(Some(&label), None);
            item.set_action_and_target_value(
                Some("win.bulk-remove-from-collection"),
                Some(&id.to_variant()),
            );
            remove.append_item(&item);
        }
        let menu = gio::Menu::new();
        menu.append_section(None, &add);
        menu.append_section(None, &remove);
        imp.collection_button.set_menu_model(Some(&menu));
        imp.collection_button
            .set_sensitive(some && !collections.is_empty());
    }

    /// Applies `action` to the selected games and leaves selection mode, with a toast
    /// that undoes it for the games it changed.
    fn apply_bulk(&self, action: BulkAction) {
        let games: Vec<GameId> = self.selected_games().iter().map(GameObject::id).collect();
        if games.is_empty() {
            return;
        }
        self.imp().selection.set_active(false);
        let collection = match action {
            BulkAction::AddToCollection(id) | BulkAction::RemoveFromCollection(id) => self
                .library()
                .collections()
                .into_iter()
                .find(|collection| collection.id == id)
                .map(|collection| collection.name)
                .unwrap_or_default(),
            _ => String::new(),
        };
        let (window, service) = (self.downgrade(), self.library_service());
        glib::spawn_future_local(async move {
            let Some(changed) = service.apply_bulk(games, action).await else {
                return;
            };
            let Some(window) = window.upgrade() else {
                return;
            };
            if changed.is_empty() {
                return;
            }
            let toast = adw::Toast::builder()
                .title(done_title(action, changed.len(), &collection))
                .button_label(gettext("_Undo"))
                .use_markup(false)
                .build();
            toast.connect_button_clicked(move |_| {
                let (service, changed) = (service.clone(), changed.clone());
                glib::spawn_future_local(async move {
                    service.apply_bulk(changed, action.undone()).await;
                });
            });
            window.add_toast(toast);
        });
    }

    /// Merges the selected games into one game with several copies and leaves selection
    /// mode; a split from the game's menu undoes it.
    fn merge_selected(&self) {
        let games: Vec<GameId> = self.selected_games().iter().map(GameObject::id).collect();
        if games.len() < 2 {
            return;
        }
        self.imp().selection.set_active(false);
        self.merge_selection(games, |count| {
            ngettext(
                "{count} game merged into one",
                "{count} games merged into one",
                count_u32(count),
            )
            .replace("{count}", &count.to_string())
        });
    }
}

/// The toast after `action` changed `count` games, naming `collection` for the
/// collection actions.
fn done_title(action: BulkAction, count: usize, collection: &str) -> String {
    let n = count_u32(count);
    let title = match action {
        BulkAction::Favourite => ngettext(
            "{count} game added to Favourites",
            "{count} games added to Favourites",
            n,
        ),
        BulkAction::Unfavourite => ngettext(
            "{count} game removed from Favourites",
            "{count} games removed from Favourites",
            n,
        ),
        BulkAction::Hide => ngettext("{count} game hidden", "{count} games hidden", n),
        BulkAction::Unhide => ngettext("{count} game unhidden", "{count} games unhidden", n),
        BulkAction::AddToCollection(_) => ngettext(
            "{count} game added to “{collection}”",
            "{count} games added to “{collection}”",
            n,
        ),
        BulkAction::RemoveFromCollection(_) => ngettext(
            "{count} game removed from “{collection}”",
            "{count} games removed from “{collection}”",
            n,
        ),
    };
    title
        .replace("{count}", &count.to_string())
        .replace("{collection}", collection)
}

/// A game count as ngettext takes it.
fn count_u32(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}
