//! The Add Game and Edit Game dialog. Games added by hand get their folder, program,
//! title and options here; every game can get its own cover, banner and logo.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::manual::{self, ManualGame};
use pindu_core::model::{ArtLayout, ArtOrigin, GameId, Source};
use pindu_core::paths::{default_library_db_path, default_user_art_dir};
use pindu_core::store::Store;

use crate::file_service::{self, PickedImage};
use crate::library_service::LibraryService;
use crate::window::PinduWindow;

/// What the dialog is for.
#[derive(Debug, Clone, Default)]
pub enum Mode {
    #[default]
    Add,
    /// Editing a game; `manual` games also have their folder and program edited.
    Edit { id: GameId, manual: bool },
}

/// A pending change to the user's art in one layout.
#[derive(Debug, Clone)]
pub enum ArtChange {
    Set(PickedImage),
    Remove,
}

const LAYOUTS: [ArtLayout; 3] = [ArtLayout::Cover, ArtLayout::Hero, ArtLayout::Logo];

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/game_dialog.ui")]
    pub struct GameDialog {
        #[template_child]
        pub save_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub game_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub folder_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub executable_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub executables: TemplateChild<gtk::StringList>,
        #[template_child]
        pub title_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub arguments_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub working_dir_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub cover_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub hero_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub logo_row: TemplateChild<adw::ActionRow>,
        pub mode: RefCell<Mode>,
        pub library: OnceCell<LibraryService>,
        /// The chosen game folder, as the host sees it.
        pub folder: RefCell<Option<PathBuf>>,
        pub art: RefCell<HashMap<ArtLayout, ArtChange>>,
        /// Layouts that already have art the user picked.
        pub user_art: RefCell<Vec<ArtLayout>>,
        pub remove_buttons: RefCell<HashMap<ArtLayout, gtk::Button>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GameDialog {
        const NAME: &'static str = "PinduGameDialog";
        type Type = super::GameDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[gtk::template_callbacks]
    impl GameDialog {
        #[template_callback]
        fn save(&self) {
            self.obj().save();
        }

        #[template_callback]
        fn choose_folder(&self) {
            self.obj().choose_folder();
        }

        #[template_callback]
        fn form_changed(&self) {
            self.obj().update_save_button();
        }
    }

    impl ObjectImpl for GameDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let dialog = self.obj();
            for layout in LAYOUTS {
                dialog.add_art_buttons(layout);
            }
            self.executable_row.connect_selected_notify(glib::clone!(
                #[weak]
                dialog,
                move |_| dialog.update_save_button()
            ));
        }
    }
    impl WidgetImpl for GameDialog {}
    impl AdwDialogImpl for GameDialog {}
}

glib::wrapper! {
    pub struct GameDialog(ObjectSubclass<imp::GameDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl GameDialog {
    /// A dialog that adds a game by hand.
    pub fn add(library: &LibraryService) -> Self {
        let dialog: Self = glib::Object::new();
        let imp = dialog.imp();
        imp.library.set(library.clone()).expect("new dialog");
        dialog.set_title(&gettext("Add Game"));
        imp.save_button.set_label(&gettext("_Add"));
        dialog.show_art_state();
        dialog
    }

    /// A dialog that edits a game: everything for a game added by hand, its art for others.
    pub fn edit(library: &LibraryService, id: &GameId, title: &str) -> Self {
        let dialog: Self = glib::Object::new();
        let imp = dialog.imp();
        imp.library.set(library.clone()).expect("new dialog");
        let manual = id.source() == Source::Manual;
        imp.mode.replace(Mode::Edit {
            id: id.clone(),
            manual,
        });
        dialog.set_title(&gettext("Edit “{title}”").replace("{title}", title));
        imp.save_button.set_label(&gettext("_Save"));
        imp.game_group.set_visible(manual);
        dialog.load(id.clone(), manual);
        dialog
    }

    fn library(&self) -> &LibraryService {
        self.imp().library.get().expect("set at construction")
    }

    fn toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }

    /// Reads the stored form and which layouts have user art, off the main thread.
    fn load(&self, id: GameId, manual: bool) {
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let read = gio::spawn_blocking(move || {
                let store = Store::open(&default_library_db_path()).map_err(|e| e.to_string())?;
                let form = if manual {
                    manual::read_game(&store, &id).map_err(|e| e.to_string())?
                } else {
                    None
                };
                let user_art = store
                    .artwork(&id)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .filter(|art| art.origin == ArtOrigin::User)
                    .map(|art| art.layout)
                    .collect::<Vec<_>>();
                Ok::<_, String>((form, user_art))
            })
            .await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            match read {
                Ok(Ok((form, user_art))) => {
                    dialog.imp().user_art.replace(user_art);
                    if let Some(form) = form {
                        dialog.fill(&form);
                    }
                    dialog.show_art_state();
                }
                Ok(Err(error)) => dialog.toast(&error),
                Err(_) => {}
            }
        });
    }

    /// Shows a stored game's form.
    fn fill(&self, form: &ManualGame) {
        let imp = self.imp();
        imp.title_row.set_text(&form.title);
        imp.arguments_row.set_text(&form.arguments);
        imp.working_dir_row.set_text(&form.working_dir);
        self.set_folder(form.folder.clone(), Some(form.executable.clone()));
    }

    fn choose_folder(&self) {
        let chooser = gtk::FileDialog::builder()
            .title(gettext("Choose the Game's Folder"))
            .modal(true)
            .build();
        let root = self.root().and_downcast::<gtk::Window>();
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(folder) = chooser.select_folder_future(root.as_ref()).await else {
                return;
            };
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            let Some(host) = file_service::host_path(&folder).await else {
                dialog.toast(&gettext("This folder cannot be used"));
                return;
            };
            if dialog.imp().title_row.text().is_empty()
                && let Some(name) = host.file_name()
            {
                dialog.imp().title_row.set_text(&name.to_string_lossy());
            }
            dialog.set_folder_scanning(host, folder.path());
        });
    }

    /// Records the folder and lists its programs; `keep` is the program to keep selected.
    fn set_folder(&self, host: PathBuf, keep: Option<String>) {
        let local = Some(host.clone());
        self.set_folder_with(host, local, keep);
    }

    fn set_folder_scanning(&self, host: PathBuf, local: Option<PathBuf>) {
        self.set_folder_with(host, local, None);
    }

    fn set_folder_with(&self, host: PathBuf, local: Option<PathBuf>, keep: Option<String>) {
        let imp = self.imp();
        imp.folder_row.set_subtitle(&host.to_string_lossy());
        imp.folder.replace(Some(host));
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let found = match local {
                Some(local) => gio::spawn_blocking(move || manual::find_executables(&local))
                    .await
                    .unwrap_or_default(),
                None => Vec::new(),
            };
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            let imp = dialog.imp();
            let mut names: Vec<String> = found
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            if let Some(keep) = &keep
                && !names.contains(keep)
            {
                names.insert(0, keep.clone());
            }
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            imp.executables.splice(0, imp.executables.n_items(), &refs);
            let selected = keep
                .and_then(|keep| names.iter().position(|name| *name == keep))
                .unwrap_or(0);
            imp.executable_row.set_selected(selected as u32);
            imp.executable_row.set_sensitive(!names.is_empty());
            imp.executable_row.set_subtitle(&if names.is_empty() {
                gettext("No programs found in this folder")
            } else {
                String::new()
            });
            dialog.update_save_button();
        });
    }

    fn selected_executable(&self) -> Option<String> {
        let imp = self.imp();
        imp.executable_row
            .selected_item()
            .and_downcast::<gtk::StringObject>()
            .map(|item| item.string().to_string())
    }

    fn form(&self) -> Option<ManualGame> {
        let imp = self.imp();
        Some(ManualGame {
            title: imp.title_row.text().to_string(),
            folder: imp.folder.borrow().clone()?,
            executable: self.selected_executable()?,
            arguments: imp.arguments_row.text().to_string(),
            working_dir: imp.working_dir_row.text().to_string(),
        })
    }

    fn update_save_button(&self) {
        let imp = self.imp();
        let ready = match &*imp.mode.borrow() {
            Mode::Add | Mode::Edit { manual: true, .. } => self
                .form()
                .is_some_and(|form| !form.title.trim().is_empty()),
            Mode::Edit { manual: false, .. } => !imp.art.borrow().is_empty(),
        };
        imp.save_button.set_sensitive(ready);
    }

    fn art_row(&self, layout: ArtLayout) -> adw::ActionRow {
        let imp = self.imp();
        match layout {
            ArtLayout::Cover => imp.cover_row.get(),
            ArtLayout::Hero => imp.hero_row.get(),
            _ => imp.logo_row.get(),
        }
    }

    /// Adds "from file", "from web address" and "remove" buttons to an art row.
    fn add_art_buttons(&self, layout: ArtLayout) {
        let row = self.art_row(layout);
        let button = |icon: &str, tooltip: String| {
            let button = gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(tooltip)
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            row.add_suffix(&button);
            button
        };
        let from_file = button("document-open-symbolic", gettext("Choose an Image File"));
        let from_web = button("web-browser-symbolic", gettext("Use a Web Address"));
        let remove = button("user-trash-symbolic", gettext("Remove Your Image"));
        from_file.connect_clicked(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| dialog.pick_art_file(layout)
        ));
        from_web.connect_clicked(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| dialog.ask_art_address(layout)
        ));
        remove.connect_clicked(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| dialog.set_art(layout, ArtChange::Remove)
        ));
        self.imp()
            .remove_buttons
            .borrow_mut()
            .insert(layout, remove);
    }

    fn pick_art_file(&self, layout: ArtLayout) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Images")));
        filter.add_mime_type("image/*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let chooser = gtk::FileDialog::builder()
            .title(gettext("Choose an Image"))
            .modal(true)
            .filters(&filters)
            .build();
        let root = self.root().and_downcast::<gtk::Window>();
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(file) = chooser.open_future(root.as_ref()).await else {
                return;
            };
            let read = file_service::read_image(&file).await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            match read {
                Ok(image) => dialog.set_art(layout, ArtChange::Set(image)),
                Err(error) => dialog.toast(&error),
            }
        });
    }

    fn ask_art_address(&self, layout: ArtLayout) {
        let entry = gtk::Entry::builder()
            .placeholder_text("https://")
            .activates_default(true)
            .input_purpose(gtk::InputPurpose::Url)
            .build();
        let ask = adw::AlertDialog::builder()
            .heading(gettext("Use a Web Address"))
            .body(gettext(
                "The image is downloaded once and kept with your library.",
            ))
            .extra_child(&entry)
            .close_response("cancel")
            .default_response("use")
            .focus_widget(&entry)
            .build();
        ask.add_responses(&[("cancel", &gettext("_Cancel")), ("use", &gettext("_Use"))]);
        ask.set_response_appearance("use", adw::ResponseAppearance::Suggested);
        let dialog = self.downgrade();
        ask.connect_response(Some("use"), move |_, _| {
            let address = entry.text().to_string();
            let dialog = dialog.clone();
            glib::spawn_future_local(async move {
                let fetched = file_service::fetch_image(&address).await;
                let Some(dialog) = dialog.upgrade() else {
                    return;
                };
                match fetched {
                    Ok(image) => dialog.set_art(layout, ArtChange::Set(image)),
                    Err(error) => dialog.toast(
                        &gettext("Could not use the address: {error}").replace("{error}", &error),
                    ),
                }
            });
        });
        ask.present(Some(self));
    }

    fn set_art(&self, layout: ArtLayout, change: ArtChange) {
        self.imp().art.borrow_mut().insert(layout, change);
        self.show_art_state();
        self.update_save_button();
    }

    /// Each art row says what will happen to it.
    fn show_art_state(&self) {
        let imp = self.imp();
        let art = imp.art.borrow();
        let user_art = imp.user_art.borrow();
        let adding = matches!(
            *imp.mode.borrow(),
            Mode::Add | Mode::Edit { manual: true, .. }
        );
        for layout in LAYOUTS {
            let has_user_art = user_art.contains(&layout);
            let (subtitle, removable) = match art.get(&layout) {
                Some(ArtChange::Set(image)) => (
                    gettext("New image ({extension})").replace("{extension}", &image.extension),
                    true,
                ),
                Some(ArtChange::Remove) => (gettext("Your image will be removed"), false),
                None if has_user_art => (gettext("Your image"), true),
                None if adding => (gettext("None"), false),
                None => (gettext("From the launcher, if it has one"), false),
            };
            self.art_row(layout).set_subtitle(&subtitle);
            if let Some(remove) = imp.remove_buttons.borrow().get(&layout) {
                remove.set_visible(removable);
            }
        }
    }

    /// Writes the game and art changes off the main thread, then closes.
    fn save(&self) {
        let imp = self.imp();
        let mode = imp.mode.borrow().clone();
        let form = self.form();
        let art: Vec<(ArtLayout, ArtChange)> = imp.art.borrow().clone().into_iter().collect();
        let title = imp.title_row.text().to_string();
        imp.save_button.set_sensitive(false);
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let saved = gio::spawn_blocking(move || save_to_store(mode, form, art)).await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            match saved {
                Ok(Ok(added)) => {
                    dialog.library().reload();
                    if added && let Some(window) = dialog.root().and_downcast::<PinduWindow>() {
                        window.add_toast(adw::Toast::new(
                            &gettext("Added “{title}”").replace("{title}", &title),
                        ));
                    }
                    dialog.close();
                }
                Ok(Err(error)) => {
                    dialog.toast(&error);
                    dialog.update_save_button();
                }
                Err(_) => dialog.update_save_button(),
            }
        });
    }
}

/// Stores the dialog's changes; returns whether a game was added.
fn save_to_store(
    mode: Mode,
    form: Option<ManualGame>,
    art: Vec<(ArtLayout, ArtChange)>,
) -> Result<bool, String> {
    let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let (id, added) = match (mode, form) {
        (Mode::Add, Some(form)) => (
            manual::add_game(&store, &form, now).map_err(|e| e.to_string())?,
            true,
        ),
        (Mode::Edit { id, manual: true }, Some(form)) => {
            manual::update_game(&store, &id, &form).map_err(|e| e.to_string())?;
            (id, false)
        }
        (Mode::Edit { id, manual: false }, _) => (id, false),
        (_, None) => return Err(gettext("Choose a folder and a program first")),
    };
    let art_dir = default_user_art_dir();
    for (layout, change) in art {
        match change {
            ArtChange::Set(image) => manual::set_user_art(
                &store,
                &art_dir,
                &id,
                layout,
                &image.extension,
                &image.bytes,
            )
            .map(drop),
            ArtChange::Remove => manual::remove_user_art(&store, &id, layout),
        }
        .map_err(|error| error.to_string())?;
    }
    Ok(added)
}
