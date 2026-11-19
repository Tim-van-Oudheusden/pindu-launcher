//! Fullscreen mode's Add Game, which Quick Access's Add a Game opens: a dialog with
//! its own Blueprint (`data/ui/fullscreen_add_game.blp`) that a controller alone gets
//! through. It asks what kind of game to add (native, an installed Windows game, or a
//! Windows installer to run first), picks the game's folder or the installer with a picker
//! fullscreen mode draws itself, as a portal's file chooser cannot be driven with a
//! controller, then the game's program, and takes the title, prefilled from the program,
//! from the on-screen keyboard (`secret_dialog`). A Windows game's Proton build is picked
//! from the list Quick Access's Performance level shows (`fullscreen_proton`), the default
//! first, and the game runs through umu as on the desktop; arguments and the working
//! folder stay the desktop dialog's.
//!
//! Which step shows and where B goes back to is `pindu_core::add_game`; what the picker
//! lists is `pindu_core::file_picker`, read through `file_service`, so the picker shows
//! only what the launcher can already read. The game is stored as the desktop's Add Game
//! stores it (`game_dialog::save_to_store`).

use std::cell::{Cell, OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::add_game::{Flow, Route, Step};
use pindu_core::file_picker::{Accept, Entry, EntryKind, Place, PlaceKind};
use pindu_core::manual::{self, ManualGame, WindowsOptions};
use pindu_core::osk::Keyboard;
use pindu_core::paths::home_relative;
use pindu_core::pe::Icon;

use crate::file_service;
use crate::fullscreen_proton::ProtonChecks;
use crate::game_dialog::{Mode, save_to_store};
use crate::proton_choice::FirstChoice;
use crate::proton_launch;
use crate::secret_dialog::ask_secret;
use crate::window::PinduWindow;

/// What a row of the list does when it is activated.
#[derive(Debug, Clone)]
pub enum RowAction {
    /// Chooses the kind of game.
    Route(Route),
    /// Opens a place or a folder in the picker.
    Open(PathBuf),
    /// Takes the folder shown as the game's.
    UseFolder,
    /// Runs this installer.
    Installer(PathBuf),
    /// Takes this program, relative to the game's folder, as the game's.
    Program(String),
    /// Types the title.
    Title,
    /// Nothing: the row only tells something.
    Info,
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/fullscreen_add_game.ui")]
    pub struct PinduFullscreenAddGame {
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub heading: TemplateChild<gtk::Label>,
        #[template_child]
        pub description: TemplateChild<gtk::Label>,
        #[template_child]
        pub busy: TemplateChild<gtk::Box>,
        #[template_child]
        pub list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub empty: TemplateChild<gtk::Label>,
        #[template_child]
        pub proton_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub proton_builds: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub add_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub add_row: TemplateChild<adw::ButtonRow>,
        #[template_child]
        pub note: TemplateChild<gtk::Label>,
        /// The step that shows and the ones before it.
        pub flow: RefCell<Flow>,
        /// What each row of the list does, in the rows' order.
        pub actions: RefCell<Vec<RowAction>>,
        /// Counts the steps shown, so a listing that ends after the user moved on is
        /// dropped.
        pub shown: Cell<u64>,
        /// The game's folder, as the sandbox sees it, which its programs are relative to.
        pub folder: RefCell<Option<PathBuf>>,
        /// The programs found in the game's folder.
        pub programs: RefCell<Vec<PathBuf>>,
        /// The game's program, relative to its folder.
        pub program: RefCell<Option<String>>,
        /// The prefix an installer ran in, which the game keeps.
        pub prefix: RefCell<Option<PathBuf>>,
        /// The game's title, and whether the user typed it rather than took the program's.
        pub title: RefCell<String>,
        pub title_typed: Cell<bool>,
        /// A Windows program's own icon, which becomes the game's.
        pub icon: RefCell<Option<Icon>>,
        /// A Windows game's Proton build, listed the first time the last step shows.
        pub proton: OnceCell<Rc<ProtonChecks>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduFullscreenAddGame {
        const NAME: &'static str = "PinduFullscreenAddGame";
        type Type = super::PinduFullscreenAddGame;
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
    impl PinduFullscreenAddGame {
        #[template_callback]
        fn row_activated(&self, row: &gtk::ListBoxRow) {
            let action = usize::try_from(row.index())
                .ok()
                .and_then(|index| self.actions.borrow().get(index).cloned());
            if let Some(action) = action {
                self.obj().act(action);
            }
        }

        #[template_callback]
        fn add_activated(&self) {
            self.obj().add();
        }
    }

    impl ObjectImpl for PinduFullscreenAddGame {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().show_step();
        }
    }
    impl WidgetImpl for PinduFullscreenAddGame {}
    impl AdwDialogImpl for PinduFullscreenAddGame {}
}

glib::wrapper! {
    /// Fullscreen mode's Add Game.
    pub struct PinduFullscreenAddGame(ObjectSubclass<imp::PinduFullscreenAddGame>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl Default for PinduFullscreenAddGame {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl PinduFullscreenAddGame {
    /// Add Game at its first step, what kind of game to add.
    pub fn new() -> Self {
        Self::default()
    }

    /// B: back a step or up a folder, or nothing while the installer runs; `false` on
    /// the first step, which B closes.
    pub fn step_back(&self) -> bool {
        let imp = self.imp();
        let before = imp.flow.borrow().step().clone();
        if !imp.flow.borrow_mut().back() {
            return false;
        }
        if *imp.flow.borrow().step() != before {
            self.show_step();
        }
        true
    }

    /// Whether B goes back a step rather than closing, for the hints.
    pub fn goes_back(&self) -> bool {
        *self.imp().flow.borrow().step() != Step::Route
    }

    fn window(&self) -> Option<PinduWindow> {
        self.root().and_downcast::<PinduWindow>()
    }

    fn toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }

    fn route(&self) -> Route {
        self.imp().flow.borrow().route()
    }

    /// Does what an activated row says.
    fn act(&self, action: RowAction) {
        let imp = self.imp();
        match action {
            RowAction::Route(route) => {
                imp.prefix.replace(None);
                imp.flow.borrow_mut().choose_route(route);
                self.show_step();
            }
            RowAction::Open(folder) => {
                imp.flow.borrow_mut().open(&folder);
                self.show_step();
            }
            RowAction::UseFolder => self.use_folder(),
            RowAction::Installer(installer) => self.run_installer(installer),
            RowAction::Program(program) => self.choose_program(program),
            RowAction::Title => self.ask_title(),
            RowAction::Info => {}
        }
    }

    /// Shows the step the flow is at, and focuses its first row once it is listed.
    fn show_step(&self) {
        let imp = self.imp();
        let shown = imp.shown.get().wrapping_add(1);
        imp.shown.set(shown);
        imp.list.remove_all();
        imp.actions.borrow_mut().clear();
        imp.busy.set_visible(false);
        imp.list.set_visible(true);
        imp.proton_title.set_visible(false);
        imp.proton_builds.set_visible(false);
        imp.add_list.set_visible(false);
        imp.empty.set_label("");
        self.set_can_close(true);
        let step = imp.flow.borrow().step().clone();
        let route = self.route();
        let picking = matches!(step, Step::Places | Step::Folder(_));
        self.show_note(if picking {
            Some(gettext(
                "Only folders this launcher can already read are listed: the Games folder, \
                 your home folder as far as it may read it, and drives the system has mounted.",
            ))
        } else if matches!(step, Step::Route | Step::Details)
            && (step == Step::Route || route.windows())
            && !proton_launch::umu_installed()
        {
            Some(gettext(
                "umu-launcher is not installed. Windows games and their installers run \
                 through it.",
            ))
        } else {
            None
        });
        match step {
            Step::Route => {
                self.show_heading(&gettext("What Do You Want to Add?"), None);
                self.add_rows(route_rows());
            }
            Step::Places => {
                let heading = match route.accept() {
                    Accept::Folders => gettext("Choose the Game's Folder"),
                    Accept::WindowsPrograms => gettext("Choose the Game's Installer"),
                };
                self.show_heading(&heading, None);
                self.when_listed(shown, file_service::picker_places(), |dialog, places| {
                    dialog
                        .imp()
                        .empty
                        .set_label(&gettext("No folder this launcher can read was found"));
                    dialog.add_rows(places.into_iter().map(place_row).collect());
                });
            }
            Step::Folder(folder) => self.show_folder(shown, folder, route.accept()),
            Step::Installing => {
                self.show_heading(&gettext("Installing"), None);
                imp.busy.set_visible(true);
                imp.list.set_visible(false);
                self.set_can_close(false);
            }
            Step::Programs => {
                let folder = imp.folder.borrow().clone().unwrap_or_default();
                self.show_heading(
                    &gettext("Choose the Game's Program"),
                    Some(&shown_path(&folder)),
                );
                imp.empty
                    .set_label(&gettext("No programs found in this folder"));
                let rows = imp
                    .programs
                    .borrow()
                    .iter()
                    .map(|program| program_row(program, route))
                    .collect();
                self.add_rows(rows);
            }
            Step::Details => self.show_details(),
        }
        if let Some(window) = self.window() {
            window.update_hints();
        }
    }

    /// The picker in `folder`: Use This Folder when the step picks a folder, then its
    /// subfolders and the files the step takes.
    fn show_folder(&self, shown: u64, folder: PathBuf, accept: Accept) {
        let name = folder.file_name().map_or_else(
            || folder.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        self.show_heading(&name, Some(&shown_path(&folder)));
        if accept == Accept::Folders {
            let use_folder = adw::ButtonRow::builder()
                .title(gettext("Use This Folder"))
                .start_icon_name("object-select-symbolic")
                .css_classes(["suggested-action"])
                .build();
            self.add_rows(vec![(use_folder.upcast(), RowAction::UseFolder)]);
        }
        self.when_listed(
            shown,
            file_service::list_folder(folder, accept),
            move |dialog, listed| match listed {
                Ok(entries) => {
                    dialog.imp().empty.set_label(&match accept {
                        Accept::Folders => gettext("This folder has no folders in it"),
                        Accept::WindowsPrograms => {
                            gettext("This folder has no folders or Windows programs in it")
                        }
                    });
                    dialog.add_rows(entries.into_iter().map(entry_row).collect());
                }
                Err(error) => dialog.imp().empty.set_label(
                    &gettext("This folder cannot be read: {error}").replace("{error}", &error),
                ),
            },
        );
    }

    /// The title and the program, a Windows game's Proton build, and Add, which has the
    /// focus once there is a title.
    fn show_details(&self) {
        let imp = self.imp();
        let title = imp.title.borrow().clone();
        self.show_heading(&gettext("Add the Game"), None);
        let title_row = action_row(
            &gettext("Title"),
            Some(&title),
            "document-edit-symbolic",
            false,
        );
        let program = imp.program.borrow().clone().unwrap_or_default();
        let program_row = info_row(&gettext("Program"), &program);
        self.add_rows(vec![
            (title_row.upcast(), RowAction::Title),
            (program_row.upcast(), RowAction::Info),
        ]);
        if self.route().windows() {
            imp.proton
                .get_or_init(|| ProtonChecks::new(&imp.proton_builds, FirstChoice::Default, None));
            imp.proton_title.set_visible(true);
            imp.proton_builds.set_visible(true);
        }
        let has_title = !title.trim().is_empty();
        imp.add_row.set_sensitive(has_title);
        imp.add_list.set_visible(true);
        if has_title {
            imp.add_row.grab_focus();
        }
    }

    fn show_heading(&self, heading: &str, description: Option<&str>) {
        let imp = self.imp();
        imp.heading.set_label(heading);
        imp.description.set_label(description.unwrap_or_default());
        imp.description.set_visible(description.is_some());
    }

    fn show_note(&self, note: Option<String>) {
        let imp = self.imp();
        imp.note.set_visible(note.is_some());
        imp.note.set_label(&note.unwrap_or_default());
    }

    /// Runs `listing` and hands its result to `show`, unless another step showed since
    /// step `shown`.
    fn when_listed<T: 'static>(
        &self,
        shown: u64,
        listing: impl Future<Output = T> + 'static,
        show: impl FnOnce(&Self, T) + 'static,
    ) {
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let listed = listing.await;
            if let Some(dialog) = dialog.upgrade()
                && dialog.imp().shown.get() == shown
            {
                show(&dialog, listed);
            }
        });
    }

    /// Appends rows to the list, each doing what its action says, and focuses the first
    /// one that can take focus.
    fn add_rows(&self, rows: Vec<(gtk::Widget, RowAction)>) {
        let imp = self.imp();
        for (row, action) in rows {
            imp.list.append(&row);
            imp.actions.borrow_mut().push(action);
        }
        let actions = imp.actions.borrow();
        let target = actions
            .iter()
            .position(|action| !matches!(action, RowAction::Info));
        if let Some(row) = target.and_then(|index| imp.list.row_at_index(index as i32)) {
            row.grab_focus();
        }
    }

    /// The folder shown is the game's: its programs are looked for.
    fn use_folder(&self) {
        let imp = self.imp();
        let Some(folder) = imp.flow.borrow().folder().map(Path::to_owned) else {
            return;
        };
        let shown = imp.shown.get();
        let windows = self.route().windows();
        self.when_listed(
            shown,
            file_service::game_programs(folder.clone(), windows),
            move |dialog, programs| {
                let imp = dialog.imp();
                imp.folder.replace(Some(folder));
                imp.programs.replace(programs);
                imp.flow.borrow_mut().use_folder();
                dialog.show_step();
            },
        );
    }

    /// Takes the game's program, and its own title and icon for a Windows game, unless
    /// the user typed a title.
    fn choose_program(&self, program: String) {
        let imp = self.imp();
        let Some(folder) = imp.folder.borrow().clone() else {
            return;
        };
        let windows = self.route().windows();
        let shown = imp.shown.get();
        let path = folder.join(&program);
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let info = if windows {
                file_service::windows_program(path).await
            } else {
                Default::default()
            };
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            let imp = dialog.imp();
            if imp.shown.get() != shown {
                return;
            }
            if !imp.title_typed.get() {
                imp.title.replace(manual::program_title(
                    Path::new(&program),
                    info.title.as_deref(),
                ));
            }
            imp.icon.replace(info.icon);
            imp.program.replace(Some(program));
            imp.flow.borrow_mut().choose_program();
            dialog.show_step();
        });
    }

    /// Asks for the title with the on-screen keyboard, starting from the one shown.
    fn ask_title(&self) {
        let dialog = self.downgrade();
        let asked = ask_secret(
            self,
            &gettext("Title"),
            &gettext("The game's name, as the library shows it."),
            Keyboard::text(),
            move |text| {
                let (Some(dialog), Some(text)) = (dialog.upgrade(), text) else {
                    return;
                };
                let title = text.trim();
                if title.is_empty() {
                    return;
                }
                let imp = dialog.imp();
                imp.title.replace(title.to_owned());
                imp.title_typed.set(true);
                dialog.show_step();
            },
        );
        if let Some(asked) = asked {
            asked.show_text(&self.imp().title.borrow());
        }
    }

    /// Runs the installer through umu in a new prefix with the default Proton build, then
    /// lists the programs it installed; when it installed none, the game's folder is picked
    /// by hand and keeps the prefix.
    fn run_installer(&self, installer: PathBuf) {
        let Some(window) = self.window() else {
            return;
        };
        let Some(app) = window.application() else {
            return;
        };
        let imp = self.imp();
        let name = installer
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let prefix = manual::new_prefix(&proton_launch::defaults().prefixes, &name);
        imp.flow.borrow_mut().start_installer();
        self.show_step();
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let before = file_service::prefix_programs(prefix.clone()).await;
            let toast = {
                let dialog = dialog.clone();
                move |toast: adw::Toast| {
                    if let Some(dialog) = dialog.upgrade() {
                        dialog.imp().toast_overlay.add_toast(toast);
                    }
                }
            };
            let ran = proton_launch::run_installer(
                &app,
                Some(window.upcast()),
                installer,
                None,
                prefix.clone(),
                toast,
            )
            .await;
            let installed = file_service::installed_programs(prefix.clone(), before).await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            let imp = dialog.imp();
            if ran {
                imp.prefix.replace(Some(prefix));
            }
            match installed {
                Some((folder, programs)) => {
                    imp.folder.replace(Some(folder));
                    imp.programs.replace(programs);
                    imp.flow.borrow_mut().installed(true);
                    dialog.toast(&gettext("Installed. Pick the game's program."));
                }
                None if ran => {
                    imp.flow.borrow_mut().installed(false);
                    dialog.toast(&gettext(
                        "The installer added no programs. Choose the game's folder instead.",
                    ));
                }
                None => imp.flow.borrow_mut().installer_failed(),
            }
            dialog.show_step();
        });
    }

    /// Stores the game as the desktop's Add Game does, then closes.
    fn add(&self) {
        let imp = self.imp();
        let (Some(folder), Some(program)) =
            (imp.folder.borrow().clone(), imp.program.borrow().clone())
        else {
            return;
        };
        let title = imp.title.borrow().trim().to_owned();
        if title.is_empty() {
            return;
        }
        let windows = self.route().windows();
        let form = ManualGame {
            title: title.clone(),
            folder,
            executable: program,
            arguments: String::new(),
            working_dir: String::new(),
            host: false,
            windows: windows.then(|| WindowsOptions {
                proton_version: imp.proton.get().and_then(|checks| checks.selected()),
                prefix: imp.prefix.borrow().clone(),
            }),
        };
        let icon = if windows {
            imp.icon.borrow().clone()
        } else {
            None
        };
        let defaults = proton_launch::defaults();
        self.set_sensitive(false);
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let saved = gio::spawn_blocking(move || {
                save_to_store(Mode::Add, Some(form), icon, Vec::new(), defaults)
            })
            .await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            dialog.set_sensitive(true);
            match saved {
                Ok(Ok(_)) => {
                    if let Some(window) = dialog.window() {
                        window.library_service().reload();
                        window.add_toast(adw::Toast::new(
                            &gettext("Added “{title}”").replace("{title}", &title),
                        ));
                    }
                    dialog.close();
                }
                Ok(Err(error)) => dialog.toast(&error),
                Err(_) => dialog.toast(&gettext("The game could not be added")),
            }
        });
    }
}

impl PinduWindow {
    /// The `win.add-game-fullscreen` action of Quick Access's Add a Game.
    pub(crate) fn setup_fullscreen_add_game(&self) {
        let open = gio::ActionEntry::builder("add-game-fullscreen")
            .activate(|window: &Self, _, _| window.open_fullscreen_add_game())
            .build();
        self.add_action_entries([open]);
    }

    /// Closes the panels and opens Add Game.
    fn open_fullscreen_add_game(&self) {
        self.close_panel();
        if self.visible_dialog().is_none() {
            PinduFullscreenAddGame::new().present(Some(self));
        }
    }
}

/// A path as shown, with the home folder as `~`.
fn shown_path(path: &Path) -> String {
    home_relative(path, &std::env::home_dir().unwrap_or_default())
}

/// The kinds of game to add.
fn route_rows() -> Vec<(gtk::Widget, RowAction)> {
    [
        (
            Route::Native,
            gettext("Native Game"),
            gettext("A Linux game: pick its folder, then its program"),
            "application-x-executable-symbolic",
        ),
        (
            Route::Windows,
            gettext("Windows Game"),
            gettext(
                "An installed Windows game, run through Proton: pick its folder, then its program",
            ),
            "applications-games-symbolic",
        ),
        (
            Route::Installer,
            gettext("Windows Installer"),
            gettext(
                "Run a Windows game's installer through Proton, then pick the program it installed",
            ),
            "system-software-install-symbolic",
        ),
    ]
    .into_iter()
    .map(|(route, title, subtitle, icon)| {
        (
            action_row(&title, Some(&subtitle), icon, true).upcast(),
            RowAction::Route(route),
        )
    })
    .collect()
}

/// A place the picker starts from.
fn place_row(place: Place) -> (gtk::Widget, RowAction) {
    let (title, icon) = match place.kind {
        PlaceKind::Games => (gettext("Games"), "folder-symbolic"),
        PlaceKind::Home => (gettext("Home Folder"), "user-home-symbolic"),
        PlaceKind::Drive => (place.name.clone(), "drive-removable-media-symbolic"),
    };
    let row = action_row(&title, Some(&shown_path(&place.path)), icon, true);
    (row.upcast(), RowAction::Open(place.path))
}

/// A subfolder to open, or an installer to run.
fn entry_row(entry: Entry) -> (gtk::Widget, RowAction) {
    match entry.kind {
        EntryKind::Folder => (
            action_row(&entry.name, None, "folder-symbolic", true).upcast(),
            RowAction::Open(entry.path),
        ),
        EntryKind::File => (
            action_row(
                &entry.name,
                Some(&gettext("Run this installer")),
                "system-software-install-symbolic",
                false,
            )
            .upcast(),
            RowAction::Installer(entry.path),
        ),
    }
}

/// A program found in the game's folder.
fn program_row(program: &Path, route: Route) -> (gtk::Widget, RowAction) {
    let icon = if route.windows() {
        "applications-games-symbolic"
    } else {
        "application-x-executable-symbolic"
    };
    let name = program.to_string_lossy().into_owned();
    (
        action_row(&name, None, icon, false).upcast(),
        RowAction::Program(name),
    )
}

/// A row A activates, with an icon, and an arrow when it leads on to a list.
fn action_row(title: &str, subtitle: Option<&str>, icon: &str, leads_on: bool) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .use_markup(false)
        .activatable(true)
        .build();
    if let Some(subtitle) = subtitle {
        row.set_subtitle(subtitle);
    }
    row.add_prefix(
        &gtk::Image::builder()
            .icon_name(icon)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build(),
    );
    if leads_on {
        row.add_suffix(
            &gtk::Image::builder()
                .icon_name("go-next-symbolic")
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build(),
        );
    }
    row
}

/// A row that only tells something, which focus passes by.
fn info_row(title: &str, value: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(value)
        .use_markup(false)
        .activatable(false)
        .focusable(false)
        .css_classes(["property"])
        .build()
}
