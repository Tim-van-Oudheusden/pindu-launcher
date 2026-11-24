//! Quick Access's Library Health (`pindu_core::health`): its entry, subtitled with how many
//! problems need attention, and its level with the same report the desktop's Library
//! Health page shows, in its four sections. The level is [`PinduHealthLevel`], with its own
//! Blueprint (`data/ui/fullscreen_health.blp`); Quick Access keeps the entry, which sits in
//! its shared list, and adds `win.health-next-step` to its host. Both show
//! [`HealthService`]'s report; neither gathers it.
//!
//! Every row takes focus, so the report is read with a controller alone. A problem's row
//! takes its next step on A where fullscreen mode can take it, and otherwise says where it
//! is taken, as the system panel's rows do (`docs/system-panel.md`): never as an error.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};
use pindu_core::health::{
    FullscreenSession, HealthReport, Install, NextStep, Problem, SourceHealth, StepReach, Tool,
};
use pindu_core::model::SourceFound;
use pindu_core::panels::QuickAccessLevel;
use pindu_core::sync_age::sync_age;

use crate::application::PinduApplication;
use crate::config::APP_ID;
use crate::health_service::{self, HealthService};
use crate::proton_launch::UMU_INSTALL_URI;
use crate::quick_access::PinduQuickAccess;
use crate::session_service;
use crate::sources::{source_name, sync_age_text};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/fullscreen_health.ui")]
    pub struct PinduHealthLevel {
        #[template_child]
        pub quick_access_health_summary: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_health_sources_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_health_sources: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_health_launching_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_health_launching: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_health_artwork_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_health_artwork: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_health_library_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_health_library: TemplateChild<gtk::ListBox>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduHealthLevel {
        const NAME: &'static str = "PinduHealthLevel";
        type Type = super::PinduHealthLevel;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduHealthLevel {}

    impl WidgetImpl for PinduHealthLevel {}

    impl BinImpl for PinduHealthLevel {}
}

glib::wrapper! {
    /// Quick Access's Library Health level: the report's Sources, Launching, Artwork and
    /// Library Data, each problem with its next step. It is the `"library-health"` page of
    /// Quick Access's stack.
    pub struct PinduHealthLevel(ObjectSubclass<imp::PinduHealthLevel>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PinduQuickAccess {
    /// Adds `win.health-next-step`, which closes the panel and activates the application
    /// action a problem's next step names, and shows the entry as the report is and as it
    /// changes.
    pub(crate) fn setup_health(
        &self,
        application: &PinduApplication,
        actions: &impl IsA<gio::ActionMap>,
    ) {
        self.add_host_action(
            actions,
            "health-next-step",
            Some(glib::VariantTy::STRING),
            |quick_access, target| {
                let Some(action) = target.and_then(|target| target.str()) else {
                    return;
                };
                let Some(name) = action.strip_prefix("app.") else {
                    return;
                };
                quick_access.request_close();
                if let Some(application) = quick_access.application() {
                    application.activate_action(name, None);
                }
            },
        );
        let service = application.health_service();
        service.connect_changed(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |service| quick_access.show_health_report(service.report().as_ref())
        ));
        self.show_health_report(service.report().as_ref());
    }

    /// The level opens: it shows the report as last gathered, and gathers it again.
    pub(crate) fn show_health(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let service = application.health_service();
        self.imp()
            .health_level
            .fill(service.report().as_ref(), self.health_session(service));
        service.refresh();
    }

    /// Where the level shows the report: over a game or not, in a gamescope session or not.
    fn health_session(&self, service: &HealthService) -> FullscreenSession {
        FullscreenSession {
            over_game: self.over_game(),
            gamescope_session: service
                .report()
                .is_some_and(|report| report.launching.gamescope_session),
        }
    }

    /// Shows the entry's subtitle and icon, and fills the level again while it is open,
    /// keeping focus on it.
    fn show_health_report(&self, report: Option<&HealthReport>) {
        let imp = self.imp();
        let summary = summary(report);
        imp.health_subtitle.set_visible(report.is_some());
        imp.health_subtitle.set_label(&summary);
        let problems = report.map_or(0, |report| report.problems().count());
        imp.health_icon.set_icon_name(Some(if problems > 0 {
            "dialog-warning-symbolic"
        } else {
            "emblem-ok-symbolic"
        }));
        if self.level() != Some(QuickAccessLevel::LibraryHealth) {
            return;
        }
        let Some(application) = self.application() else {
            return;
        };
        let level = &imp.health_level;
        let had_focus = level.has_focus();
        level.fill(report, self.health_session(application.health_service()));
        if had_focus || !level.has_focus() {
            self.focus_level(None);
        }
    }
}

impl PinduHealthLevel {
    /// The level's lists, whose Up and Down go on to the next list at their ends.
    pub(crate) fn lists(&self) -> [gtk::ListBox; 4] {
        let imp = self.imp();
        [
            imp.quick_access_health_sources.get(),
            imp.quick_access_health_launching.get(),
            imp.quick_access_health_artwork.get(),
            imp.quick_access_health_library.get(),
        ]
    }

    /// Where focus goes on the level: its first row.
    pub(crate) fn focus_target(&self) -> Option<gtk::Widget> {
        self.lists()
            .iter()
            .find_map(|list| list.row_at_index(0))
            .map(|row| row.upcast())
    }

    /// Whether focus is on one of the level's rows, which filling it again takes away.
    pub(crate) fn has_focus(&self) -> bool {
        self.root()
            .and_then(|root| gtk::prelude::RootExt::focus(&root))
            .is_some_and(|focus| self.lists().iter().any(|list| focus.is_ancestor(list)))
    }

    /// The level's rows for `report`, or only that it is being read while there is none
    /// yet. `session` decides which next steps the rows take themselves.
    pub(crate) fn fill(&self, report: Option<&HealthReport>, session: FullscreenSession) {
        let imp = self.imp();
        imp.quick_access_health_summary.set_label(&summary(report));
        for list in self.lists() {
            list.remove_all();
        }
        let titles = [
            &imp.quick_access_health_sources_title,
            &imp.quick_access_health_launching_title,
            &imp.quick_access_health_artwork_title,
            &imp.quick_access_health_library_title,
        ];
        for (title, list) in titles.into_iter().zip(self.lists()) {
            title.set_visible(report.is_some());
            list.set_visible(report.is_some());
        }
        let Some(report) = report else {
            return;
        };

        let sources = &imp.quick_access_health_sources;
        for source in &report.sources {
            sources.append(&source_row(source));
            for problem in &source.problems {
                sources.append(&problem_row(problem, session));
            }
        }
        let has_sources = !report.sources.is_empty();
        imp.quick_access_health_sources_title
            .set_visible(has_sources);
        sources.set_visible(has_sources);

        let launching = &imp.quick_access_health_launching;
        let launch = &report.launching;
        if let Some(host) = &launch.host_access {
            launching.append(&fact_row(
                &gettext("Start Games on This Computer"),
                &if host.allowed {
                    gettext("Allowed")
                } else {
                    gettext("Not allowed")
                },
            ));
            if let Some(problem) = &host.problem {
                launching.append(&problem_row(problem, session));
            }
        }
        launching.append(&fact_row(
            "umu-launcher",
            &if launch.umu {
                gettext("Installed")
            } else {
                gettext("Not installed")
            },
        ));
        if let Some(problem) = &launch.umu_problem {
            launching.append(&problem_row(problem, session));
        }
        launching.append(&fact_row(
            &gettext("Proton Builds"),
            &if launch.proton_builds.is_empty() {
                gettext("None installed; umu downloads one when a game needs it")
            } else {
                launch.proton_builds.join(", ")
            },
        ));
        launching.append(&fact_row(
            "MangoHud",
            &if launch.mangohud {
                gettext("Installed")
            } else {
                gettext("Not installed")
            },
        ));
        launching.append(&fact_row(
            "GameMode",
            &if launch.gamemode {
                gettext("Available")
            } else {
                gettext("Not available")
            },
        ));
        launching.append(&fact_row(
            &gettext("Session"),
            &if launch.gamescope_session {
                gettext("Gamescope session")
            } else {
                gettext("Desktop session")
            },
        ));

        let artwork = &imp.quick_access_health_artwork;
        artwork.append(&fact_row(
            &gettext("SteamGridDB Key"),
            &if report.artwork.key_stored {
                gettext("Stored")
            } else {
                gettext("Not stored; the launchers’ own art shows")
            },
        ));
        artwork.append(&fact_row(
            &gettext("Artwork Cache"),
            &glib::format_size(report.artwork.cache_bytes),
        ));

        let library = &imp.quick_access_health_library;
        library.append(&fact_row(
            &gettext("Library File"),
            &report.library.file.display().to_string(),
        ));
        library.append(&fact_row(
            &gettext("Schema Version"),
            &report.library.schema_version.to_string(),
        ));
        library.append(&fact_row(
            &gettext("Last Backup"),
            &sync_age_text(sync_age(report.library.last_backup, session_service::now())),
        ));
    }
}

/// How many problems need attention, or that the report is being read.
fn summary(report: Option<&HealthReport>) -> String {
    let Some(report) = report else {
        return gettext("Checking the library…");
    };
    match u32::try_from(report.problems().count()).unwrap_or(u32::MAX) {
        0 => gettext("Nothing needs attention"),
        count => ngettext(
            "{count} problem needs attention",
            "{count} problems need attention",
            count,
        )
        .replace("{count}", &count.to_string()),
    }
}

/// A row stating one fact: `title`, with `value` under it.
fn fact_row(title: &str, value: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(value)
        .use_markup(false)
        .css_classes(["property"])
        .build()
}

/// A source's row: whether it is on and found, then its games, how it is installed, where
/// and when it last synced.
fn source_row(source: &SourceHealth) -> adw::ActionRow {
    let games = u32::try_from(source.games).unwrap_or(u32::MAX);
    let games =
        ngettext("{count} game", "{count} games", games).replace("{count}", &games.to_string());
    let synced = || {
        gettext("Last synced: {when}").replace(
            "{when}",
            &sync_age_text(sync_age(source.last_sync, session_service::now())),
        )
    };
    let subtitle = if !source.enabled {
        gettext("Turned off")
    } else {
        match &source.found {
            SourceFound::Folder(folder) => {
                let install = match source.install {
                    Some(Install::Flatpak) => gettext("Flatpak"),
                    _ => gettext("Native"),
                };
                format!("{games}, {install}\n{}\n{}", folder.display(), synced())
            }
            SourceFound::ApplicationFolders => {
                format!("{games}\n{}\n{}", gettext("Application folders"), synced())
            }
            SourceFound::NotFound => gettext("Not found on this computer"),
        }
    };
    adw::ActionRow::builder()
        .title(source_name(source.source))
        .subtitle(subtitle)
        .use_markup(false)
        .build()
}

/// A problem's row: what is wrong, and its next step, which A takes where fullscreen mode
/// can take it; elsewhere the row says where it is taken.
fn problem_row(problem: &Problem, session: FullscreenSession) -> adw::ActionRow {
    let step = problem.next_step(APP_ID);
    // A opens Preferences at its Permissions group, which shows the command to copy; the
    // desktop page's Copy Command has no clipboard to paste from in a controller session.
    let label = match &step {
        NextStep::HostOverride { .. } => gettext("Open Permissions"),
        step => health_service::next_step_label(step),
    };
    let reach = step.reach_in_fullscreen(session);
    let subtitle = match (reach, &step) {
        (StepReach::AfterTheGame, _) => {
            // Translators: {step} is what to do about a problem, such as opening Preferences.
            gettext("{step}, once the game has ended").replace("{step}", &label)
        }
        (StepReach::OnAnotherDevice, NextStep::HowToInstall(Tool::Umu)) => {
            // Translators: {address} is a web address.
            gettext("Read how to install it on another device: {address}")
                .replace("{address}", UMU_INSTALL_URI)
        }
        _ => label,
    };
    let row = adw::ActionRow::builder()
        .title(health_service::problem_title(problem))
        .subtitle(subtitle)
        .use_markup(false)
        .build();
    row.add_prefix(
        &gtk::Image::builder()
            .icon_name("dialog-warning-symbolic")
            .accessible_role(gtk::AccessibleRole::Presentation)
            .css_classes(["warning"])
            .build(),
    );
    if reach == StepReach::Here {
        row.set_activatable(true);
        row.set_action_name(Some("win.health-next-step"));
        row.set_action_target_value(Some(&health_service::next_step_action(&step).to_variant()));
        row.add_suffix(
            &gtk::Image::builder()
                .icon_name("go-next-symbolic")
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build(),
        );
    }
    row
}
