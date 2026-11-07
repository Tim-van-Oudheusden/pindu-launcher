//! The Add Game and Edit Game dialog. Games added by hand get their type (native or
//! Windows), folder, program, title and options here, native games whether they run on
//! the host instead of in the Steam Linux Runtime, and Windows games their Proton build,
//! prefix and installer, with a note when umu-launcher is missing; every game can get its
//! own cover, banner and logo.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::manual::{self, ManualGame, WindowsOptions};
use pindu_core::model::{ArtLayout, ArtOrigin, GameId, Source};
use pindu_core::paths::{default_library_db_path, default_user_art_dir};
use pindu_core::pe::{self, Icon};
use pindu_core::proton::Defaults;
use pindu_core::store::Store;

use crate::file_service::{self, PickedImage};
use crate::library_service::LibraryService;
use crate::proton_choice::{FirstChoice, ProtonChoice};
use crate::proton_launch;
use crate::steamgriddb_picker::{self, PickerGame};
use crate::window::PinduWindow;

/// The Windows row of the Type list; the native one comes first.
const WINDOWS: u32 = 1;

/// What the dialog is for.
#[derive(Debug, Clone, Default)]
pub enum Mode {
    /// Adding a game by hand.
    #[default]
    Add,
    /// Editing a game; `manual` games also have their folder and program edited.
    Edit {
        /// The game being edited.
        id: GameId,
        /// Whether the game was added by hand.
        manual: bool,
    },
}

/// A pending change to the user's art in one layout.
#[derive(Debug, Clone)]
pub enum ArtChange {
    /// Replace the art with a picked image.
    Set(PickedImage),
    /// Remove the user's own image.
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
        pub kind_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub host_row: TemplateChild<adw::SwitchRow>,
        /// The selected Windows program's icon, at the start of the Program row while
        /// there is one (an empty prefix would still indent the row).
        pub program_icon: gtk::Image,
        #[template_child]
        pub windows_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub umu_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub umu_link: TemplateChild<gtk::LinkButton>,
        #[template_child]
        pub proton_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub prefix_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub reset_prefix_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub installer_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub installer_spinner: TemplateChild<adw::Spinner>,
        #[template_child]
        pub installer_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub cover_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub hero_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub logo_row: TemplateChild<adw::ActionRow>,
        pub mode: RefCell<Mode>,
        pub library: OnceCell<LibraryService>,
        /// The chosen game folder, as the host sees it (for native games run on the host).
        pub folder: RefCell<Option<PathBuf>>,
        /// The chosen game folder, as the sandbox sees it (for the games umu runs inside
        /// it), and for listing its programs.
        pub local_folder: RefCell<Option<PathBuf>>,
        /// The Proton row's builds.
        pub proton: OnceCell<Rc<ProtonChoice>>,
        /// The Windows game's prefix; `None` for a new one of its own.
        pub prefix: RefCell<Option<PathBuf>>,
        /// The icon of the selected Windows program.
        pub exe_icon: RefCell<Option<Icon>>,
        /// The title the dialog filled in itself, which a program's own title replaces.
        pub auto_title: RefCell<Option<String>>,
        /// Whether an installer is running; the game cannot be saved meanwhile.
        pub installing: Cell<bool>,
        pub art: RefCell<HashMap<ArtLayout, ArtChange>>,
        /// Layouts that already have art the user picked.
        pub user_art: RefCell<Vec<ArtLayout>>,
        pub remove_buttons: RefCell<HashMap<ArtLayout, gtk::Button>>,
        /// The title of the game being edited.
        pub game_title: RefCell<String>,
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

        #[template_callback]
        fn choose_prefix(&self) {
            self.obj().choose_prefix();
        }

        #[template_callback]
        fn reset_prefix(&self) {
            self.obj().set_prefix(None);
        }

        #[template_callback]
        fn run_installer(&self) {
            self.obj().run_installer();
        }
    }

    impl ObjectImpl for GameDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let dialog = self.obj();
            for layout in LAYOUTS {
                dialog.add_art_buttons(layout);
            }
            self.executable_row
                .connect_selected_item_notify(glib::clone!(
                    #[weak]
                    dialog,
                    move |_| {
                        dialog.read_program();
                        dialog.update_save_button();
                    }
                ));
            self.kind_row.connect_selected_notify(glib::clone!(
                #[weak]
                dialog,
                move |_| dialog.kind_changed()
            ));
            dialog.set_prefix(None);
            self.umu_row.set_visible(!proton_launch::umu_installed());
            self.umu_link.set_uri(proton_launch::UMU_INSTALL_URI);
            self.proton
                .set(ProtonChoice::new(
                    &self.proton_row,
                    FirstChoice::Default,
                    None,
                ))
                .expect("constructed once");
        }
    }
    impl WidgetImpl for GameDialog {}
    impl AdwDialogImpl for GameDialog {}
}

glib::wrapper! {
    /// The Add Game and Edit Game dialog.
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
        imp.game_title.replace(title.to_owned());
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
        imp.host_row.set_active(form.host);
        if let Some(windows) = &form.windows {
            self.proton().select(windows.proton_version.clone());
            self.set_prefix(windows.prefix.clone());
            imp.kind_row.set_selected(WINDOWS);
        }
        // The stored folder is the one the game's runner sees, and the one to list.
        self.set_folder(
            form.folder.clone(),
            Some(form.folder.clone()),
            Some(form.executable.clone()),
        );
    }

    fn is_windows(&self) -> bool {
        self.imp().kind_row.selected() == WINDOWS
    }

    /// Native and Windows games list different programs and have different options.
    fn kind_changed(&self) {
        let imp = self.imp();
        imp.windows_group.set_visible(self.is_windows());
        imp.host_row.set_visible(!self.is_windows());
        let local = imp.local_folder.borrow().clone();
        if let Some(local) = local {
            self.list_programs(local, self.selected_executable());
        }
        self.read_program();
        self.update_save_button();
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
                dialog.set_auto_title(&name.to_string_lossy());
            }
            dialog.set_folder(host, folder.path(), None);
        });
    }

    /// Records the folder as the host and the sandbox see it, and lists its programs;
    /// `keep` is the program to keep selected.
    fn set_folder(&self, host: PathBuf, local: Option<PathBuf>, keep: Option<String>) {
        let imp = self.imp();
        imp.folder_row.set_subtitle(&host.to_string_lossy());
        imp.folder.replace(Some(host));
        imp.local_folder.replace(local.clone());
        match local {
            Some(local) => self.list_programs(local, keep),
            None => self.show_programs(Vec::new(), keep),
        }
    }

    /// Lists the programs in a folder: `.exe` files for Windows games, files with an
    /// execute bit for native ones.
    fn list_programs(&self, folder: PathBuf, keep: Option<String>) {
        let windows = self.is_windows();
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let found = gio::spawn_blocking(move || {
                if windows {
                    manual::find_windows_executables(&folder)
                } else {
                    manual::find_executables(&folder)
                }
            })
            .await
            .unwrap_or_default();
            if let Some(dialog) = dialog.upgrade() {
                dialog.show_programs(found, keep);
            }
        });
    }

    fn show_programs(&self, found: Vec<PathBuf>, keep: Option<String>) {
        let imp = self.imp();
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
        self.update_save_button();
    }

    /// A title the dialog fills in itself, until the user types their own.
    fn set_auto_title(&self, title: &str) {
        let imp = self.imp();
        imp.auto_title.replace(Some(title.to_owned()));
        imp.title_row.set_text(title);
    }

    /// Reads the selected Windows program's own title and icon, off the main thread: the
    /// title replaces one the dialog filled in, the icon becomes the game's.
    fn read_program(&self) {
        let imp = self.imp();
        imp.exe_icon.replace(None);
        if imp.program_icon.parent().is_some() {
            imp.executable_row.remove(&imp.program_icon);
        }
        let folder = imp.local_folder.borrow().clone();
        let (true, Some(folder), Some(program)) =
            (self.is_windows(), folder, self.selected_executable())
        else {
            return;
        };
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let path = folder.join(&program);
            let info = gio::spawn_blocking(move || pe::read_exe(&path))
                .await
                .unwrap_or_default();
            let texture = match &info.icon {
                Some(icon) => crate::widgets::textures::decode_bytes(&icon.bytes)
                    .await
                    .inspect_err(|error| {
                        glib::g_debug!("pindu", "Could not read the icon of {program}: {error}")
                    })
                    .ok(),
                None => None,
            };
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            if !dialog.is_windows() || dialog.selected_executable().as_ref() != Some(&program) {
                return;
            }
            let imp = dialog.imp();
            if let Some(title) = &info.title {
                let current = imp.title_row.text();
                if current.is_empty()
                    || imp.auto_title.borrow().as_deref() == Some(current.as_str())
                {
                    dialog.set_auto_title(title);
                }
            }
            if let Some(texture) = texture {
                imp.program_icon.set_paintable(Some(&texture));
                imp.program_icon.set_pixel_size(32);
                if imp.program_icon.parent().is_none() {
                    imp.executable_row.add_prefix(&imp.program_icon);
                }
                imp.exe_icon.replace(info.icon);
            }
        });
    }

    fn proton(&self) -> &ProtonChoice {
        self.imp().proton.get().expect("constructed")
    }

    fn selected_proton(&self) -> Option<String> {
        self.proton().selected()
    }

    /// Shows the prefix the game gets: the one chosen, or a new one of its own.
    fn set_prefix(&self, prefix: Option<PathBuf>) {
        let imp = self.imp();
        imp.prefix_row.set_subtitle(&match &prefix {
            Some(prefix) => prefix.to_string_lossy().into_owned(),
            None => gettext("A new folder in {folder}").replace(
                "{folder}",
                &proton_launch::defaults().prefixes.to_string_lossy(),
            ),
        });
        imp.reset_prefix_button.set_visible(prefix.is_some());
        imp.prefix.replace(prefix);
    }

    fn choose_prefix(&self) {
        let chooser = gtk::FileDialog::builder()
            .title(gettext("Choose the Game's Prefix"))
            .modal(true)
            .initial_folder(&gio::File::for_path(proton_launch::defaults().prefixes))
            .build();
        let root = self.root().and_downcast::<gtk::Window>();
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(folder) = chooser.select_folder_future(root.as_ref()).await else {
                return;
            };
            // umu runs inside the sandbox, so the path the sandbox sees is the one kept.
            if let (Some(dialog), Some(path)) = (dialog.upgrade(), folder.path()) {
                dialog.set_prefix(Some(path));
            }
        });
    }

    /// Runs an installer the user picks in the game's prefix, then lists the programs it
    /// installed, in the folder they share, for the user to pick the game's.
    fn run_installer(&self) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Windows Programs")));
        filter.add_suffix("exe");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let chooser = gtk::FileDialog::builder()
            .title(gettext("Choose the Game's Installer"))
            .modal(true)
            .filters(&filters)
            .build();
        let root = self.root().and_downcast::<gtk::Window>();
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(file) = chooser.open_future(root.as_ref()).await else {
                return;
            };
            let (Some(dialog), Some(installer)) = (dialog.upgrade(), file.path()) else {
                return;
            };
            let Some(app) = root.as_ref().and_then(|root| root.application()) else {
                return;
            };
            let imp = dialog.imp();
            // The installer and the game share the prefix, so it is fixed now.
            let prefix = imp.prefix.borrow().clone().unwrap_or_else(|| {
                manual::new_prefix(
                    &proton_launch::defaults().prefixes,
                    imp.title_row.text().trim(),
                )
            });
            dialog.set_prefix(Some(prefix.clone()));
            dialog.set_installing(true);
            let before = {
                let prefix = prefix.clone();
                gio::spawn_blocking(move || manual::prefix_programs(&prefix))
                    .await
                    .unwrap_or_default()
            };
            let toast = {
                let dialog = dialog.downgrade();
                move |toast: adw::Toast| {
                    if let Some(dialog) = dialog.upgrade() {
                        dialog.imp().toast_overlay.add_toast(toast);
                    }
                }
            };
            let ran = proton_launch::run_installer(
                &app,
                root,
                installer,
                dialog.selected_proton(),
                prefix.clone(),
                toast,
            )
            .await;
            let installed = gio::spawn_blocking(move || {
                manual::installed_programs(&before, &manual::prefix_programs(&prefix))
            })
            .await
            .ok()
            .flatten();
            dialog.set_installing(false);
            match installed {
                Some((folder, programs)) => {
                    if dialog.imp().title_row.text().is_empty()
                        && let Some(name) = folder.file_name()
                    {
                        dialog.set_auto_title(&name.to_string_lossy());
                    }
                    // Prefixes are in the games folder, which host and sandbox see alike.
                    let imp = dialog.imp();
                    imp.folder_row.set_subtitle(&folder.to_string_lossy());
                    imp.folder.replace(Some(folder.clone()));
                    imp.local_folder.replace(Some(folder));
                    dialog.show_programs(programs, None);
                    dialog.toast(&gettext("Installed. Pick the game's program."));
                }
                None if ran => {
                    dialog.toast(&gettext(
                        "The installer added no programs. Choose the game's folder instead.",
                    ));
                }
                None => {}
            }
        });
    }

    /// While an installer runs, the row says so and cannot start another.
    fn set_installing(&self, installing: bool) {
        let imp = self.imp();
        imp.installing.set(installing);
        imp.installer_spinner.set_visible(installing);
        imp.installer_button.set_sensitive(!installing);
        imp.kind_row.set_sensitive(!installing);
        self.update_save_button();
        imp.installer_row.set_subtitle(&if installing {
            gettext("Installing… Finish the installer to go on.")
        } else {
            gettext("Run the game's installer in its prefix, then pick the program it installed")
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
        let windows = self.is_windows();
        let host = !windows && imp.host_row.is_active();
        let folder = if host {
            imp.folder.borrow().clone()?
        } else {
            imp.local_folder.borrow().clone()?
        };
        Some(ManualGame {
            title: imp.title_row.text().to_string(),
            folder,
            executable: self.selected_executable()?,
            arguments: imp.arguments_row.text().to_string(),
            working_dir: imp.working_dir_row.text().to_string(),
            host,
            windows: windows.then(|| WindowsOptions {
                proton_version: self.selected_proton(),
                prefix: imp.prefix.borrow().clone(),
            }),
        })
    }

    fn is_ready(&self) -> bool {
        let imp = self.imp();
        match &*imp.mode.borrow() {
            Mode::Add | Mode::Edit { manual: true, .. } => self
                .form()
                .is_some_and(|form| !form.title.trim().is_empty()),
            Mode::Edit { manual: false, .. } => !imp.art.borrow().is_empty(),
        }
    }

    fn update_save_button(&self) {
        let imp = self.imp();
        imp.save_button
            .set_sensitive(!imp.installing.get() && self.is_ready());
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
        let from_grid = button("system-search-symbolic", gettext("Choose From SteamGridDB"));
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
        from_grid.connect_clicked(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| dialog.pick_from_steamgriddb(layout)
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

    /// Opens the SteamGridDB picker for the game: by Steam app ID for Steam games, else
    /// by the title.
    fn pick_from_steamgriddb(&self, layout: ArtLayout) {
        let imp = self.imp();
        let steam_app_id = match &*imp.mode.borrow() {
            Mode::Edit { id, .. } if id.source() == Source::Steam => id.native_id().parse().ok(),
            _ => None,
        };
        let title = match imp.title_row.text().trim() {
            "" => imp.game_title.borrow().clone(),
            title => title.to_owned(),
        };
        let game = PickerGame {
            steam_app_id,
            title,
        };
        let dialog = self.downgrade();
        steamgriddb_picker::pick(self, layout, game, move |image| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.set_art(layout, ArtChange::Set(image));
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
        // A program's icon is only kept for Windows games, whose programs carry one.
        let icon = form
            .as_ref()
            .and_then(|form| form.windows.as_ref())
            .and_then(|_| imp.exe_icon.borrow().clone());
        let title = imp.title_row.text().to_string();
        imp.save_button.set_sensitive(false);
        let defaults = proton_launch::defaults();
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let saved =
                gio::spawn_blocking(move || save_to_store(mode, form, icon, art, defaults)).await;
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
    icon: Option<Icon>,
    art: Vec<(ArtLayout, ArtChange)>,
    defaults: Defaults,
) -> Result<bool, String> {
    let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let (id, added) = match (mode, form) {
        (Mode::Add, Some(form)) => (
            manual::add_game(&store, &form, &defaults, now).map_err(|e| e.to_string())?,
            true,
        ),
        (Mode::Edit { id, manual: true }, Some(form)) => {
            manual::update_game(&store, &id, &form, &defaults.prefixes)
                .map_err(|e| e.to_string())?;
            (id, false)
        }
        (Mode::Edit { id, manual: false }, _) => (id, false),
        (_, None) => return Err(gettext("Choose a folder and a program first")),
    };
    let art_dir = default_user_art_dir();
    if let Some(icon) = icon {
        manual::set_exe_icon(&store, &art_dir, &id, &icon).map_err(|error| error.to_string())?;
    }
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
