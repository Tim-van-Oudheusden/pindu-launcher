//! The Proton page of Preferences: the builds downloaded into our tools folder, and the
//! GE-Proton and Proton-CachyOS releases to install, listed only when the user checks.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::proton::ProtonBuild;
use pindu_core::proton_downloads::{Fork, InstallState, Release, Step, install_state};

use crate::proton_choice::ProtonChoice;
use crate::proton_download_service as service;

/// How often running installs are shown.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// The fork's name, as its makers write it.
fn fork_name(fork: Fork) -> &'static str {
    match fork {
        Fork::GeProton => "GE-Proton",
        Fork::CachyOs => "Proton-CachyOS",
    }
}

/// The widgets of the page, from the Preferences template, and what they show.
#[derive(Debug)]
pub struct ProtonDownloadsPage {
    dialog: glib::WeakRef<adw::PreferencesDialog>,
    downloaded_group: adw::PreferencesGroup,
    releases_group: adw::PreferencesGroup,
    status_row: adw::ActionRow,
    check_button: gtk::Button,
    /// The rows of the downloaded builds, to replace when they change.
    downloaded_rows: RefCell<Vec<adw::ActionRow>>,
    /// One expander per fork, made at the first check.
    forks: RefCell<Vec<(Fork, adw::ExpanderRow, Vec<ReleaseRow>)>>,
    /// The folders in our tools folder.
    installed: RefCell<Vec<String>>,
    /// The installs last shown as running, to notice when one ends.
    shown_running: RefCell<Vec<String>>,
    polling: Cell<bool>,
    /// The Proton row of the Windows page, read again when a build comes or goes.
    proton_choice: Rc<ProtonChoice>,
}

impl ProtonDownloadsPage {
    /// Shows the downloaded builds; `proton_choice` is read again after a build was
    /// installed or removed.
    pub fn new(
        dialog: &adw::PreferencesDialog,
        downloaded_group: &adw::PreferencesGroup,
        releases_group: &adw::PreferencesGroup,
        status_row: &adw::ActionRow,
        check_button: &gtk::Button,
        proton_choice: Rc<ProtonChoice>,
    ) -> Rc<Self> {
        let page = Rc::new(Self {
            dialog: dialog.downgrade(),
            downloaded_group: downloaded_group.clone(),
            releases_group: releases_group.clone(),
            status_row: status_row.clone(),
            check_button: check_button.clone(),
            downloaded_rows: RefCell::default(),
            forks: RefCell::default(),
            installed: RefCell::default(),
            shown_running: RefCell::new(service::running()),
            polling: Cell::new(false),
            proton_choice,
        });
        page.show_installed();
        page.poll();
        page
    }

    /// Lists the releases of each fork; the only way the page goes online besides an
    /// install.
    pub fn check_releases(self: &Rc<Self>) {
        self.check_button.set_sensitive(false);
        self.status_row.set_title(&gettext("Checking GitHub…"));
        self.status_row.set_subtitle("");
        self.status_row.set_visible(true);
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let mut answers = Vec::new();
            for fork in Fork::ALL {
                answers.push((fork, service::releases(fork).await));
            }
            let Some(page) = weak.upgrade() else {
                return;
            };
            page.check_button.set_sensitive(true);
            if answers.iter().all(|(_, answer)| {
                answer
                    .as_ref()
                    .is_err_and(|error| *error == service::offline_message())
            }) {
                page.status_row.set_title(&gettext("Offline"));
                page.status_row.set_subtitle(&service::offline_message());
                return;
            }
            page.status_row.set_visible(false);
            for (fork, answer) in answers {
                page.show_fork(fork, answer);
            }
        });
    }

    fn show_fork(self: &Rc<Self>, fork: Fork, answer: Result<Vec<Release>, String>) {
        let mut forks = self.forks.borrow_mut();
        let index = match forks.iter().position(|(listed, ..)| *listed == fork) {
            Some(index) => index,
            None => {
                let expander = adw::ExpanderRow::builder().title(fork_name(fork)).build();
                self.releases_group.add(&expander);
                forks.push((fork, expander, Vec::new()));
                forks.len() - 1
            }
        };
        let (_, expander, rows) = &mut forks[index];
        for row in rows.drain(..) {
            expander.remove(&row.row);
        }
        match answer {
            Ok(releases) => {
                expander.set_subtitle(
                    &gettext("{count} newest releases from {repository}")
                        .replace("{count}", &releases.len().to_string())
                        .replace("{repository}", fork.repository()),
                );
                for release in releases {
                    let row = ReleaseRow::new(release, Rc::downgrade(self));
                    expander.add_row(&row.row);
                    rows.push(row);
                }
                expander.set_expanded(true);
            }
            Err(error) => {
                expander.set_subtitle(&error);
                expander.set_expanded(false);
            }
        }
        drop(forks);
        self.show_states();
    }

    /// Reads the builds in our tools folder and shows them.
    fn show_installed(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let builds = service::installed().await;
            if let Some(page) = weak.upgrade() {
                page.fill_installed(builds);
            }
        });
    }

    fn fill_installed(self: &Rc<Self>, builds: Vec<ProtonBuild>) {
        for row in self.downloaded_rows.borrow_mut().drain(..) {
            self.downloaded_group.remove(&row);
        }
        let mut folders = Vec::new();
        let mut rows = Vec::new();
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
                .build();
            let remove = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .tooltip_text(gettext("Remove"))
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            let (weak, name, removed) = (
                Rc::downgrade(self),
                build.display_name.clone(),
                folder.clone(),
            );
            remove.connect_clicked(move |_| {
                if let Some(page) = weak.upgrade() {
                    page.ask_remove(&name, &removed);
                }
            });
            row.add_suffix(&remove);
            folders.push(folder);
            rows.push(row);
        }
        if rows.is_empty() {
            rows.push(
                adw::ActionRow::builder()
                    .title(gettext("None Yet"))
                    .css_classes(["dim-label"])
                    .build(),
            );
        }
        for row in &rows {
            self.downloaded_group.add(row);
        }
        self.downloaded_rows.replace(rows);
        self.installed.replace(folders);
        self.show_states();
    }

    fn ask_remove(self: &Rc<Self>, name: &str, folder: &str) {
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
        let (weak, folder) = (Rc::downgrade(self), folder.to_owned());
        ask.connect_response(Some("remove"), move |_, _| {
            let (weak, folder) = (weak.clone(), folder.clone());
            glib::spawn_future_local(async move {
                let removed = service::remove(folder).await;
                let Some(page) = weak.upgrade() else {
                    return;
                };
                if let Err(error) = removed {
                    page.toast(
                        &gettext("The build was not removed: {error}").replace("{error}", &error),
                    );
                }
                page.show_installed();
                page.proton_choice.reload();
            });
        });
        if let Some(dialog) = self.dialog.upgrade() {
            ask.present(Some(&dialog));
        }
    }

    fn install(self: &Rc<Self>, release: Release) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let name = release.tag.clone();
            let installed = service::install(release).await;
            if let (Some(page), Err(error)) = (weak.upgrade(), installed) {
                page.toast(
                    &gettext("{build} was not installed: {error}")
                        .replace("{build}", &name)
                        .replace("{error}", &error),
                );
            }
        });
        self.show_states();
        self.poll();
    }

    /// Shows running installs until none is left, and the builds again whenever one ends.
    fn poll(self: &Rc<Self>) {
        if self.polling.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(PROGRESS_INTERVAL, move || {
            let Some(page) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let running = service::running();
            let ended = page
                .shown_running
                .borrow()
                .iter()
                .any(|folder| !running.contains(folder));
            page.shown_running.replace(running.clone());
            if ended {
                page.show_installed();
                page.proton_choice.reload();
            }
            page.show_states();
            if running.is_empty() {
                page.polling.set(false);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    fn show_states(&self) {
        let installed = self.installed.borrow();
        for (_, _, rows) in self.forks.borrow().iter() {
            for row in rows {
                let step = service::step(row.release.folder());
                row.show(install_state(&row.release, &installed, step));
            }
        }
    }

    fn toast(&self, text: &str) {
        if let Some(dialog) = self.dialog.upgrade() {
            dialog.add_toast(adw::Toast::new(text));
        }
    }
}

/// A release in a fork's list, with its install button and progress.
#[derive(Debug)]
struct ReleaseRow {
    release: Release,
    row: adw::ActionRow,
    install: gtk::Button,
    stop: gtk::Button,
    progress: gtk::ProgressBar,
    installed: gtk::Label,
}

impl ReleaseRow {
    fn new(release: Release, page: Weak<ProtonDownloadsPage>) -> Self {
        let row = adw::ActionRow::builder().title(&release.tag).build();
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
        let install = gtk::Button::builder()
            .label(gettext("_Install"))
            .use_underline(true)
            .valign(gtk::Align::Center)
            .build();
        let stop = gtk::Button::builder()
            .icon_name("process-stop-symbolic")
            .tooltip_text(gettext("Stop Download"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .visible(false)
            .build();
        row.add_suffix(&progress);
        row.add_suffix(&installed);
        row.add_suffix(&stop);
        row.add_suffix(&install);
        let chosen = release.clone();
        install.connect_clicked(move |_| {
            if let Some(page) = page.upgrade() {
                page.install(chosen.clone());
            }
        });
        let folder = release.folder().to_owned();
        stop.connect_clicked(move |_| service::stop(&folder));
        Self {
            release,
            row,
            install,
            stop,
            progress,
            installed,
        }
    }

    fn show(&self, state: InstallState) {
        let size = glib::format_size(self.release.tarball.size);
        let published = glib::DateTime::from_iso8601(&self.release.published_at, None)
            .and_then(|date| date.to_local())
            .and_then(|date| date.format("%-d %B %Y"))
            .map_or_else(
                |_| self.release.published_at.clone(),
                |date| date.to_string(),
            );
        let about = gettext("{size}, published {date}")
            .replace("{size}", &size)
            .replace("{date}", &published);
        let installing = matches!(state, InstallState::Installing(_));
        self.install.set_visible(state == InstallState::Available);
        self.installed.set_visible(state == InstallState::Installed);
        self.progress.set_visible(installing);
        self.stop.set_visible(matches!(
            state,
            InstallState::Installing(Step::Downloading { .. })
        ));
        match state {
            InstallState::Installing(step @ Step::Downloading { received, total }) => {
                self.row.set_subtitle(
                    &gettext("Downloading {received} of {total}")
                        .replace("{received}", &glib::format_size(received))
                        .replace("{total}", &glib::format_size(total)),
                );
                match step.fraction() {
                    Some(fraction) => self.progress.set_fraction(fraction),
                    None => self.progress.pulse(),
                }
            }
            InstallState::Installing(Step::Unpacking) => {
                self.row.set_subtitle(&gettext("Unpacking…"));
                self.progress.pulse();
            }
            InstallState::Available | InstallState::Installed => self.row.set_subtitle(&about),
        }
    }
}
