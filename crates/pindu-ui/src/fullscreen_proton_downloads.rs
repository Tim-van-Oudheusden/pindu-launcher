//! Quick Access's Proton level, for a console with a controller alone: the defaults for
//! new Windows games (the Proton build, MangoHud and GameMode, the same settings as
//! Preferences' Windows page), the builds downloaded into our tools folder, which A asks
//! to remove, and the GE-Proton and Proton-CachyOS releases, listed only when the player
//! checks, which A downloads. A download runs through `proton_download_service`, as the
//! Preferences' Proton page's do, so it is a task in Activity that can be cancelled there,
//! and A on its row stops it too. The level is [`PinduProtonLevel`], with its own
//! Blueprint (`data/ui/fullscreen_proton_downloads.blp`); Quick Access keeps the entry,
//! which sits in its shared list, and adds `win.check-proton-releases` to its host.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::proton::ProtonBuild;
use pindu_core::proton_downloads::{Fork, InstallState, Release, Step, install_state};

use crate::fullscreen_proton::ProtonChecks;
use crate::proton_choice::FirstChoice;
use crate::proton_download_service as service;
use crate::proton_downloads_page::{
    all_offline, fork_name, fork_subtitle, release_subtitle, show_progress,
};
use crate::proton_launch;

/// How often running installs are shown.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// The setting of the default Proton build, empty for the newest.
const DEFAULT_PROTON_KEY: &str = "default-proton";

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/fullscreen_proton_downloads.ui"
    )]
    pub struct PinduProtonLevel {
        #[template_child]
        pub quick_access_proton_default: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_proton_tools: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_default_mangohud: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub quick_access_default_gamemode: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub quick_access_proton_downloaded: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_proton_remove_error: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_proton_check: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_proton_check_row: TemplateChild<adw::ButtonRow>,
        #[template_child]
        pub quick_access_proton_status: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_proton_fork_1: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_proton_fork_note_1: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_proton_releases_1: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_proton_fork_2: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_proton_fork_note_2: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_proton_releases_2: TemplateChild<gtk::ListBox>,
        /// The default build's list.
        pub checks: OnceCell<Rc<ProtonChecks>>,
        /// The rows of the releases listed, of every fork.
        pub(crate) releases: RefCell<Vec<ReleaseRow>>,
        /// The folders in our tools folder.
        pub installed: RefCell<Vec<String>>,
        /// The installs last shown as running, to notice when one ends.
        pub shown_running: RefCell<Vec<String>>,
        pub polling: Cell<bool>,
        /// Whether a check for releases runs.
        pub checking: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduProtonLevel {
        const NAME: &'static str = "PinduProtonLevel";
        type Type = super::PinduProtonLevel;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduProtonLevel {}

    impl WidgetImpl for PinduProtonLevel {}

    impl BinImpl for PinduProtonLevel {}
}

glib::wrapper! {
    /// Quick Access's Proton level: the defaults for new Windows games, the downloaded
    /// builds and the releases to download. It is the `"proton"` page of Quick Access's
    /// stack.
    pub struct PinduProtonLevel(ObjectSubclass<imp::PinduProtonLevel>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PinduProtonLevel {
    /// Binds the defaults to `settings`, the ones Preferences changes, and follows the
    /// default build there.
    pub(crate) fn setup(&self, settings: &gio::Settings) {
        let imp = self.imp();
        settings
            .bind(
                "default-mangohud",
                &*imp.quick_access_default_mangohud,
                "active",
            )
            .build();
        settings
            .bind(
                "default-gamemode",
                &*imp.quick_access_default_gamemode,
                "active",
            )
            .build();
        let checks = ProtonChecks::new(
            &imp.quick_access_proton_default,
            FirstChoice::Newest,
            proton_launch::defaults().proton_version,
        );
        checks.connect_changed(glib::clone!(
            #[strong]
            settings,
            move |name| {
                let _ =
                    settings.set_string(DEFAULT_PROTON_KEY, name.as_deref().unwrap_or_default());
            }
        ));
        let weak = Rc::downgrade(&checks);
        settings.connect_changed(Some(DEFAULT_PROTON_KEY), move |settings, key| {
            if let Some(checks) = weak.upgrade() {
                let name = settings.string(key);
                checks.select((!name.is_empty()).then(|| name.to_string()));
            }
        });
        let _ = imp.checks.set(checks);
    }

    /// The level's lists, whose Up and Down go on to the next list at their ends.
    pub(crate) fn lists(&self) -> [gtk::ListBox; 6] {
        let imp = self.imp();
        [
            imp.quick_access_proton_default.get(),
            imp.quick_access_proton_tools.get(),
            imp.quick_access_proton_downloaded.get(),
            imp.quick_access_proton_check.get(),
            imp.quick_access_proton_releases_1.get(),
            imp.quick_access_proton_releases_2.get(),
        ]
    }

    /// Where focus goes on the level: the default build.
    pub(crate) fn focus_target(&self) -> Option<gtk::Widget> {
        self.imp()
            .checks
            .get()
            .and_then(|checks| checks.focus_target())
    }

    /// The level opens: the builds are read again, and running installs shown.
    pub(crate) fn show(&self) {
        let imp = self.imp();
        imp.quick_access_proton_remove_error.set_visible(false);
        if let Some(checks) = imp.checks.get() {
            checks.reload();
        }
        self.show_installed();
        self.show_states();
        self.poll();
    }

    /// Lists the releases of each fork; the only way the level goes online besides a
    /// download.
    pub(crate) fn check_releases(&self) {
        let imp = self.imp();
        // The row stays sensitive, so focus stays on it while it checks.
        if imp.checking.replace(true) {
            return;
        }
        imp.quick_access_proton_check_row
            .set_title(&gettext("Checking GitHub…"));
        imp.quick_access_proton_status.set_visible(false);
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let mut answers = Vec::new();
            for fork in Fork::ALL {
                answers.push((fork, service::releases(fork).await));
            }
            let Some(level) = weak.upgrade() else {
                return;
            };
            level.show_releases(answers);
        });
    }

    fn show_releases(&self, answers: Vec<(Fork, Result<Vec<Release>, String>)>) {
        let imp = self.imp();
        imp.checking.set(false);
        imp.quick_access_proton_check_row
            .set_title(&gettext("Check for Releases"));
        let status = &imp.quick_access_proton_status;
        if all_offline(&answers) {
            status.set_label(
                &gettext("Offline: {reason}").replace("{reason}", &service::offline_message()),
            );
            status.set_visible(true);
            return;
        }
        let shown = [
            (
                &imp.quick_access_proton_fork_1,
                &imp.quick_access_proton_fork_note_1,
                &imp.quick_access_proton_releases_1,
            ),
            (
                &imp.quick_access_proton_fork_2,
                &imp.quick_access_proton_fork_note_2,
                &imp.quick_access_proton_releases_2,
            ),
        ];
        let mut rows = Vec::new();
        for ((fork, answer), (heading, note, list)) in answers.into_iter().zip(shown) {
            heading.set_label(fork_name(fork));
            heading.set_visible(true);
            note.set_label(&fork_subtitle(fork, &answer));
            note.set_visible(true);
            list.remove_all();
            let releases = answer.unwrap_or_default();
            list.set_visible(!releases.is_empty());
            for release in releases {
                let row = ReleaseRow::new(release);
                let (weak, release) = (self.downgrade(), row.release.clone());
                row.row.connect_activated(move |_| {
                    if let Some(level) = weak.upgrade() {
                        level.choose_release(&release);
                    }
                });
                list.append(&row.row);
                rows.push(row);
            }
        }
        imp.releases.replace(rows);
        self.show_states();
    }

    /// A downloads an available release, and stops one that downloads.
    fn choose_release(&self, release: &Release) {
        let folder = release.folder();
        match service::step(folder) {
            Some(Step::Downloading { .. }) => service::stop(folder),
            Some(Step::Unpacking) => {}
            None if self.imp().installed.borrow().iter().any(|f| f == folder) => {}
            None => self.install(release.clone()),
        }
    }

    /// Downloads `release`; how it ends is posted to the notifications.
    fn install(&self, release: Release) {
        glib::spawn_future_local(async move {
            let _ = service::install(release).await;
        });
        self.show_states();
        self.poll();
    }

    /// Reads the builds in our tools folder and shows them.
    fn show_installed(&self) {
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let builds = service::installed().await;
            if let Some(level) = weak.upgrade() {
                level.fill_installed(builds);
            }
        });
    }

    fn fill_installed(&self, builds: Vec<ProtonBuild>) {
        let imp = self.imp();
        let list = &imp.quick_access_proton_downloaded;
        // Filling again takes focus away from a row that had it: it goes to the row now
        // in the same place.
        let focused = self
            .root()
            .and_then(|root| root.focus())
            .and_then(|focus| focus.ancestor(gtk::ListBoxRow::static_type()))
            .and_downcast::<gtk::ListBoxRow>()
            .filter(|row| row.parent().as_ref() == Some(list.upcast_ref()))
            .map(|row| row.index());
        list.remove_all();
        let mut folders = Vec::new();
        for build in builds {
            let folder = build
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            // The folder is shown only when the build's own name differs from it.
            let row = adw::ActionRow::builder()
                .title(&build.display_name)
                .subtitle(if build.display_name == folder {
                    ""
                } else {
                    &folder
                })
                .use_markup(false)
                .activatable(true)
                .build();
            row.add_suffix(&gtk::Image::from_icon_name("user-trash-symbolic"));
            let (weak, name, removed) =
                (self.downgrade(), build.display_name.clone(), folder.clone());
            row.connect_activated(move |_| {
                if let Some(level) = weak.upgrade() {
                    level.ask_remove(&name, &removed);
                }
            });
            list.append(&row);
            folders.push(folder);
        }
        if folders.is_empty() {
            list.append(
                &adw::ActionRow::builder()
                    .title(gettext("None Yet"))
                    .css_classes(["dim-label"])
                    .build(),
            );
        }
        imp.installed.replace(folders);
        if let Some(index) = focused {
            let last = list.last_child().and_downcast::<gtk::ListBoxRow>();
            if let Some(row) = list.row_at_index(index).or(last) {
                row.grab_focus();
            }
        }
        self.show_states();
    }

    fn ask_remove(&self, name: &str, folder: &str) {
        let ask = adw::AlertDialog::builder()
            .heading(gettext("Remove {build}?").replace("{build}", name))
            .body(gettext(
                "Games set to this build will not start until you choose another one.",
            ))
            .close_response("cancel")
            .default_response("cancel")
            .build();
        ask.add_responses(&[
            ("cancel", &gettext("_Cancel")),
            ("remove", &gettext("_Remove")),
        ]);
        ask.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        let (weak, folder) = (self.downgrade(), folder.to_owned());
        ask.connect_response(Some("remove"), move |_, _| {
            let (weak, folder) = (weak.clone(), folder.clone());
            glib::spawn_future_local(async move {
                let removed = service::remove(folder).await;
                let Some(level) = weak.upgrade() else {
                    return;
                };
                let error = &level.imp().quick_access_proton_remove_error;
                match removed {
                    Ok(()) => error.set_visible(false),
                    Err(reason) => {
                        error.set_label(
                            &gettext("The build was not removed: {error}")
                                .replace("{error}", &reason),
                        );
                        error.set_visible(true);
                    }
                }
                level.show_installed();
                if let Some(checks) = level.imp().checks.get() {
                    checks.reload();
                }
            });
        });
        ask.present(Some(self));
    }

    /// Shows running installs until none is left, and the builds again whenever one ends.
    fn poll(&self) {
        let imp = self.imp();
        if imp.polling.replace(true) {
            return;
        }
        imp.shown_running.replace(service::running());
        let weak = self.downgrade();
        glib::timeout_add_local(PROGRESS_INTERVAL, move || {
            let Some(level) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let imp = level.imp();
            let running = service::running();
            let ended = imp
                .shown_running
                .borrow()
                .iter()
                .any(|folder| !running.contains(folder));
            imp.shown_running.replace(running.clone());
            if ended {
                level.show_installed();
                if let Some(checks) = imp.checks.get() {
                    checks.reload();
                }
            }
            level.show_states();
            if running.is_empty() {
                imp.polling.set(false);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    fn show_states(&self) {
        let imp = self.imp();
        let installed = imp.installed.borrow();
        for row in imp.releases.borrow().iter() {
            let step = service::step(row.release.folder());
            row.show(install_state(&row.release, &installed, step));
        }
    }
}

/// A release in a fork's list: A downloads it, or stops its download.
#[derive(Debug)]
pub(crate) struct ReleaseRow {
    release: Release,
    row: adw::ActionRow,
    /// What A does: download, or stop.
    icon: gtk::Image,
    progress: gtk::ProgressBar,
    installed: gtk::Label,
}

impl ReleaseRow {
    fn new(release: Release) -> Self {
        let row = adw::ActionRow::builder()
            .title(&release.tag)
            .use_markup(false)
            .build();
        let progress = gtk::ProgressBar::builder()
            .valign(gtk::Align::Center)
            .width_request(120)
            .visible(false)
            .build();
        let installed = gtk::Label::builder()
            .label(gettext("Installed"))
            .css_classes(["dim-label"])
            .visible(false)
            .build();
        let icon = gtk::Image::new();
        row.add_suffix(&progress);
        row.add_suffix(&installed);
        row.add_suffix(&icon);
        Self {
            release,
            row,
            icon,
            progress,
            installed,
        }
    }

    fn show(&self, state: InstallState) {
        let downloading = matches!(state, InstallState::Installing(Step::Downloading { .. }));
        let available = state == InstallState::Available;
        self.row.set_activatable(available || downloading);
        self.icon.set_visible(available || downloading);
        self.icon.set_icon_name(Some(if downloading {
            "process-stop-symbolic"
        } else {
            "folder-download-symbolic"
        }));
        self.icon
            .update_property(&[gtk::accessible::Property::Label(&if downloading {
                gettext("Stop Download")
            } else {
                gettext("Download")
            })]);
        self.installed.set_visible(state == InstallState::Installed);
        self.progress
            .set_visible(matches!(state, InstallState::Installing(_)));
        self.row
            .set_subtitle(&release_subtitle(&self.release, state));
        show_progress(&self.progress, state);
    }
}
