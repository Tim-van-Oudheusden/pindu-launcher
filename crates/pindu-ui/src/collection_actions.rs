//! Collections in the main window: the sidebar row menu (rename, reorder, delete), the
//! "Add to Collection" menus of tiles and the desktop game page, and their undo toasts.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::model::{Collection, GameId};

use crate::models::GameObject;
use crate::window::PinduWindow;

/// The "Add to Collection" submenu and "Remove from" items for a game's menus.
pub fn collection_menu(game: &GameObject, collections: &[Collection]) -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append_submenu(
        Some(&gettext("_Add to Collection")),
        &add_menu(game, collections),
    );
    for item in remove_items(game, collections) {
        menu.append_item(&item);
    }
    menu
}

/// The collections the game is not in, then "New Collection…".
fn add_menu(game: &GameObject, collections: &[Collection]) -> gio::Menu {
    let id = game.id();
    let member_of = game.entry().collections.clone();
    let add = gio::Menu::new();
    for collection in collections.iter().filter(|c| !member_of.contains(&c.id)) {
        let item = gio::MenuItem::new(Some(&collection.name), None);
        item.set_action_and_target_value(
            Some("win.add-to-collection"),
            Some(&(id.as_str(), collection.id).to_variant()),
        );
        add.append_item(&item);
    }
    let new = gio::MenuItem::new(Some(&gettext("_New Collection…")), None);
    new.set_action_and_target_value(Some("win.new-collection"), Some(&id.as_str().to_variant()));
    let new_section = gio::Menu::new();
    new_section.append_item(&new);
    add.append_section(None, &new_section);
    add
}

/// "Remove from" items for the collections the game is in.
fn remove_items(game: &GameObject, collections: &[Collection]) -> Vec<gio::MenuItem> {
    let id = game.id();
    let member_of = game.entry().collections.clone();
    collections
        .iter()
        .filter(|c| member_of.contains(&c.id))
        .map(|collection| {
            let label =
                gettext("Remove from “{collection}”").replace("{collection}", &collection.name);
            let item = gio::MenuItem::new(Some(&label), None);
            item.set_action_and_target_value(
                Some("win.remove-from-collection"),
                Some(&(id.as_str(), collection.id).to_variant()),
            );
            item
        })
        .collect()
}

impl PinduWindow {
    pub(crate) fn setup_collection_actions(&self) {
        let rename = gio::ActionEntry::builder("rename-collection")
            .activate(|window: &Self, _, _| {
                if let Some(id) = window.imp().menu_collection.get() {
                    window.rename_collection(id);
                }
            })
            .build();
        let up = gio::ActionEntry::builder("move-collection-up")
            .activate(|window: &Self, _, _| window.move_collection(-1))
            .build();
        let down = gio::ActionEntry::builder("move-collection-down")
            .activate(|window: &Self, _, _| window.move_collection(1))
            .build();
        let delete = gio::ActionEntry::builder("delete-collection")
            .activate(|window: &Self, _, _| {
                if let Some(id) = window.imp().menu_collection.get() {
                    window.delete_collection(id);
                }
            })
            .build();
        let pair = glib::VariantTy::new("(sx)").expect("valid type");
        let add = gio::ActionEntry::builder("add-to-collection")
            .parameter_type(Some(pair))
            .activate(|window: &Self, _, target| {
                if let Some((game, collection)) = game_and_collection(target) {
                    window.set_in_collection(&game, collection, true);
                }
            })
            .build();
        let remove = gio::ActionEntry::builder("remove-from-collection")
            .parameter_type(Some(pair))
            .activate(|window: &Self, _, target| {
                if let Some((game, collection)) = game_and_collection(target) {
                    window.set_in_collection(&game, collection, false);
                }
            })
            .build();
        let new = gio::ActionEntry::builder("new-collection")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|window: &Self, _, target| {
                let game = target.and_then(|t| t.str()).and_then(|id| id.parse().ok());
                window.new_collection(game);
            })
            .build();
        self.add_action_entries([rename, up, down, delete, add, remove, new]);
    }

    fn collection_name(&self, id: i64) -> String {
        self.library()
            .collections()
            .into_iter()
            .find(|collection| collection.id == id)
            .map(|collection| collection.name)
            .unwrap_or_default()
    }

    /// Asks for a name, then creates the collection, with `game` in it if given.
    pub(crate) fn new_collection(&self, game: Option<GameId>) {
        let window = self.downgrade();
        self.ask_collection_name(
            gettext("New Collection"),
            gettext("_Create"),
            "",
            move |name| {
                let Some(window) = window.upgrade() else {
                    return;
                };
                let game = game.clone();
                glib::spawn_future_local(async move {
                    let service = window.library_service();
                    if let Some(collection) = service.create_collection(&name, game.as_ref()).await
                    {
                        window.add_toast(adw::Toast::new(
                            &gettext("Created “{collection}”")
                                .replace("{collection}", &collection.name),
                        ));
                    }
                });
            },
        );
    }

    fn rename_collection(&self, id: i64) {
        let window = self.downgrade();
        let current = self.collection_name(id);
        self.ask_collection_name(
            gettext("Rename Collection"),
            gettext("_Rename"),
            &current,
            move |name| {
                if let Some(window) = window.upgrade() {
                    window.library_service().rename_collection(id, &name);
                }
            },
        );
    }

    fn move_collection(&self, step: isize) {
        if let Some(id) = self.imp().menu_collection.get() {
            self.library_service().move_collection(id, step);
        }
    }

    /// Deletes a collection (its games stay), with a toast to undo it.
    fn delete_collection(&self, id: i64) {
        let window = self.downgrade();
        glib::spawn_future_local(async move {
            let Some(window) = window.upgrade() else {
                return;
            };
            let service = window.library_service();
            let Some((collection, members)) = service.delete_collection(id).await else {
                return;
            };
            let toast = adw::Toast::builder()
                .title(gettext("Deleted “{collection}”").replace("{collection}", &collection.name))
                .button_label(gettext("_Undo"))
                .use_markup(false)
                .build();
            let restore = std::cell::Cell::new(Some((collection, members)));
            toast.connect_button_clicked(move |_| {
                if let Some((collection, members)) = restore.take() {
                    service.restore_collection(collection, members);
                }
            });
            window.add_toast(toast);
        });
    }

    /// Adds a game to a collection or takes it out, with a toast to undo it.
    fn set_in_collection(&self, game: &GameId, collection: i64, member: bool) {
        let service = self.library_service();
        service.set_in_collection(collection, game, member);
        let name = self.collection_name(collection);
        let title = if member {
            gettext("Added to “{collection}”")
        } else {
            gettext("Removed from “{collection}”")
        };
        let toast = adw::Toast::builder()
            .title(title.replace("{collection}", &name))
            .button_label(gettext("_Undo"))
            .use_markup(false)
            .build();
        let game = game.clone();
        toast
            .connect_button_clicked(move |_| service.set_in_collection(collection, &game, !member));
        self.add_toast(toast);
    }

    /// A small dialog with a name entry; `done` gets the trimmed, non-empty name.
    fn ask_collection_name(
        &self,
        heading: String,
        confirm: String,
        initial: &str,
        done: impl Fn(String) + 'static,
    ) {
        let entry = gtk::Entry::builder()
            .text(initial)
            .activates_default(true)
            .placeholder_text(gettext("Name"))
            .build();
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .extra_child(&entry)
            .close_response("cancel")
            .default_response("confirm")
            .focus_widget(&entry)
            .build();
        dialog.add_responses(&[("cancel", &gettext("_Cancel")), ("confirm", &confirm)]);
        dialog.set_response_appearance("confirm", adw::ResponseAppearance::Suggested);
        dialog.set_response_enabled("confirm", !initial.trim().is_empty());
        entry.connect_changed(glib::clone!(
            #[weak]
            dialog,
            move |entry| dialog.set_response_enabled("confirm", !entry.text().trim().is_empty())
        ));
        dialog.connect_response(Some("confirm"), move |_, _| {
            let name = entry.text().trim().to_owned();
            if !name.is_empty() {
                done(name);
            }
        });
        dialog.present(Some(self));
    }
}

fn game_and_collection(target: Option<&glib::Variant>) -> Option<(GameId, i64)> {
    let (game, collection) = target?.get::<(String, i64)>()?;
    Some((game.parse().ok()?, collection))
}
