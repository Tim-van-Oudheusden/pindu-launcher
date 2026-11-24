//! Library Health in the desktop window: a page of its own, opened from the main menu,
//! that shows the health service's report and follows it as it changes after a sync.
//! Every problem has a button for its next step: Preferences at its Sources page, the
//! `flatpak override` command to copy, or umu-launcher's install instructions.

use std::cell::RefCell;
use std::path::Path;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::glib;
use pindu_core::health::{HealthReport, Install, NextStep, Problem, SourceHealth};
use pindu_core::model::SourceFound;
use pindu_core::paths;
use pindu_core::sync_age::sync_age;

use crate::config::APP_ID;
use crate::health_service::{self, HealthService};
use crate::session_service;
use crate::sources::{source_icon, source_name, sync_age_text};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/health_page.ui")]
    pub struct HealthPage {
        #[template_child]
        pub page: TemplateChild<adw::PreferencesPage>,
        #[template_child]
        pub attention_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub sources_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub launching_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub artwork_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub library_group: TemplateChild<adw::PreferencesGroup>,
        /// The service whose report the page shows, and its `changed` handler.
        pub service: RefCell<Option<(HealthService, glib::SignalHandlerId)>>,
        /// The rows the report filled in, with their group, to take out on the next one.
        pub rows: RefCell<Vec<(adw::PreferencesGroup, gtk::Widget)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HealthPage {
        const NAME: &'static str = "PinduHealthPage";
        type Type = super::HealthPage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for HealthPage {
        fn dispose(&self) {
            if let Some((service, handler)) = self.service.take() {
                service.disconnect(handler);
            }
        }
    }

    impl WidgetImpl for HealthPage {}
    impl NavigationPageImpl for HealthPage {}
}

glib::wrapper! {
    /// The Library Health page.
    pub struct HealthPage(ObjectSubclass<imp::HealthPage>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl HealthPage {
    /// A page showing `service`'s report, which it asks to gather again now.
    pub fn new(service: &HealthService) -> Self {
        let page: Self = glib::Object::new();
        let handler = service.connect_changed(glib::clone!(
            #[weak]
            page,
            move |service| page.show(service.report().as_ref())
        ));
        page.show(service.report().as_ref());
        page.imp().service.replace(Some((service.clone(), handler)));
        service.refresh();
        page
    }

    /// Shows `report`, or that it is being gathered.
    fn show(&self, report: Option<&HealthReport>) {
        let imp = self.imp();
        for (group, row) in imp.rows.take() {
            group.remove(&row);
        }
        let Some(report) = report else {
            imp.page.set_description(&gettext("Checking the library…"));
            imp.attention_group.set_visible(false);
            return;
        };
        let problems: Vec<&Problem> = report.problems().collect();
        imp.page.set_description(&if problems.is_empty() {
            gettext("Nothing needs attention")
        } else {
            let count = u32::try_from(problems.len()).unwrap_or(u32::MAX);
            ngettext(
                "{count} thing needs attention",
                "{count} things need attention",
                count,
            )
            .replace("{count}", &count.to_string())
        });
        imp.attention_group.set_visible(!problems.is_empty());
        for problem in problems {
            self.add(&imp.attention_group, &self.problem_row(problem));
        }
        for source in &report.sources {
            self.add(&imp.sources_group, &source_row(source));
        }
        self.show_launching(report);
        let artwork = &report.artwork;
        self.add(
            &imp.artwork_group,
            &property_row(
                &gettext("SteamGridDB Key"),
                &if artwork.key_stored {
                    gettext("Stored in your keyring")
                } else {
                    gettext("Not stored; the launchers' own art shows")
                },
            ),
        );
        self.add(
            &imp.artwork_group,
            &property_row(
                &gettext("Artwork Cache"),
                &glib::format_size(artwork.cache_bytes),
            ),
        );
        let library = &report.library;
        let file = property_row(&gettext("Library File"), &home_relative(&library.file));
        file.set_subtitle_selectable(true);
        self.add(&imp.library_group, &file);
        self.add(
            &imp.library_group,
            &property_row(
                &gettext("Schema Version"),
                &library.schema_version.to_string(),
            ),
        );
        let backup = library
            .last_backup
            .and_then(|time| glib::DateTime::from_unix_local(time).ok())
            .and_then(|time| time.format("%x").ok())
            .map_or_else(|| gettext("Not recorded"), |date| date.to_string());
        self.add(
            &imp.library_group,
            &property_row(&gettext("Last Backup"), &backup),
        );
    }

    fn show_launching(&self, report: &HealthReport) {
        let group = &self.imp().launching_group;
        let launching = &report.launching;
        if let Some(host) = &launching.host_access {
            let row = property_row(
                &gettext("Start Games on This Computer"),
                &if host.allowed {
                    gettext("Allowed")
                } else {
                    gettext("Not allowed")
                },
            );
            self.add(group, &row);
        }
        self.add(
            group,
            &property_row(
                &gettext("umu-launcher"),
                &if launching.umu {
                    gettext("Installed")
                } else {
                    gettext("Not installed")
                },
            ),
        );
        let proton = if launching.proton_builds.is_empty() {
            gettext("None installed; umu downloads one the first time a game needs it")
        } else {
            launching.proton_builds.join(&gettext(", "))
        };
        self.add(group, &property_row(&gettext("Proton Builds"), &proton));
        for (title, available) in [
            (gettext("MangoHud"), launching.mangohud),
            (gettext("GameMode"), launching.gamemode),
        ] {
            let value = if available {
                gettext("Available")
            } else {
                gettext("Not available")
            };
            self.add(group, &property_row(&title, &value));
        }
        self.add(
            group,
            &property_row(
                &gettext("Gamescope Session"),
                &if launching.gamescope_session {
                    gettext("Yes")
                } else {
                    gettext("No")
                },
            ),
        );
    }

    /// A problem with the button that takes its next step.
    fn problem_row(&self, problem: &Problem) -> adw::ActionRow {
        let step = problem.next_step(APP_ID);
        let row = adw::ActionRow::builder()
            .title(health_service::problem_title(problem))
            .use_markup(false)
            .build();
        row.add_prefix(
            &gtk::Image::builder()
                .icon_name("dialog-warning-symbolic")
                .css_classes(["warning"])
                .build(),
        );
        let button = gtk::Button::builder()
            .label(health_service::next_step_label(&step))
            .valign(gtk::Align::Center)
            .build();
        match step {
            NextStep::HostOverride { command } => {
                row.set_subtitle(
                    &gettext("Run this command, then restart Pindu launcher: {command}")
                        .replace("{command}", &command),
                );
                row.set_subtitle_selectable(true);
                button.connect_clicked(glib::clone!(
                    #[weak(rename_to = page)]
                    self,
                    move |_| page.copy_command(&command)
                ));
            }
            step => button.set_action_name(Some(health_service::next_step_action(&step))),
        }
        row.add_suffix(&button);
        row
    }

    /// Copies the override command and says so.
    fn copy_command(&self, command: &str) {
        self.clipboard().set_text(command);
        if let Some(window) = self.root().and_downcast::<crate::window::PinduWindow>() {
            window.add_toast(adw::Toast::new(&gettext("Command copied")));
        }
    }

    fn add(&self, group: &adw::PreferencesGroup, row: &impl IsA<gtk::Widget>) {
        group.add(row);
        self.imp()
            .rows
            .borrow_mut()
            .push((group.clone(), row.clone().upcast()));
    }
}

/// One source: found or not, where and how it is installed, its games, its last sync and
/// what that sync could not read.
fn source_row(source: &SourceHealth) -> gtk::Widget {
    let subtitle = match (&source.found, source.enabled) {
        (_, false) => gettext("Not imported"),
        (SourceFound::NotFound, true) => gettext("Not found on this computer"),
        (SourceFound::ApplicationFolders, true) => gettext("Application folders"),
        (SourceFound::Folder(folder), true) => home_relative(folder),
    };
    let found = source.enabled && source.found != SourceFound::NotFound;
    if !found {
        let row = adw::ActionRow::builder()
            .title(source_name(source.source))
            .subtitle(subtitle)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(source_icon(source.source)));
        add_warning(&row, source);
        return row.upcast();
    }
    let row = adw::ExpanderRow::builder()
        .title(source_name(source.source))
        .subtitle(subtitle)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name(source_icon(source.source)));
    add_warning(&row, source);
    // The Desktop source reads every application folder, so it is installed as nothing.
    if let Some(install) = source.install {
        let install = match install {
            Install::Flatpak => gettext("Flatpak"),
            Install::Native => gettext("Native"),
        };
        row.add_row(&property_row(&gettext("Installed As"), &install));
    }
    let games = u32::try_from(source.games).unwrap_or(u32::MAX);
    row.add_row(&property_row(
        &gettext("Games"),
        &ngettext("{count} game", "{count} games", games).replace("{count}", &games.to_string()),
    ));
    row.add_row(&property_row(
        &gettext("Last Synced"),
        &sync_age_text(sync_age(source.last_sync, session_service::now())),
    ));
    for problem in &source.sync_problems {
        let problem_row = adw::ActionRow::builder()
            .title(problem)
            .use_markup(false)
            .title_selectable(true)
            .build();
        problem_row.add_prefix(
            &gtk::Image::builder()
                .icon_name("dialog-warning-symbolic")
                .css_classes(["warning"])
                .build(),
        );
        row.add_row(&problem_row);
    }
    row.upcast()
}

/// A warning icon on a source's row when it has problems; they are listed under Needs
/// Attention.
fn add_warning(row: &impl IsA<adw::PreferencesRow>, source: &SourceHealth) {
    if source.problems.is_empty() {
        return;
    }
    let icon = gtk::Image::builder()
        .icon_name("dialog-warning-symbolic")
        .tooltip_text(gettext("Needs attention"))
        .css_classes(["warning"])
        .build();
    if let Some(row) = row.dynamic_cast_ref::<adw::ExpanderRow>() {
        row.add_suffix(&icon);
    } else if let Some(row) = row.dynamic_cast_ref::<adw::ActionRow>() {
        row.add_suffix(&icon);
    }
}

/// A row naming a fact and its value.
fn property_row(title: &str, value: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(value)
        .use_markup(false)
        .css_classes(["property"])
        .build()
}

/// `path` with the home folder written as `~`.
fn home_relative(path: &Path) -> String {
    paths::home_relative(path, &glib::home_dir())
}
