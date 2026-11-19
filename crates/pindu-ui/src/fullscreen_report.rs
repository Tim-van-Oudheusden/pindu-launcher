//! Quick Access's Diagnostics Report, how a console gets the debugging information About
//! carries ([`crate::diagnostics_service`]) out with a controller alone: the level,
//! [`PinduReportLevel`] with its own Blueprint (`data/ui/fullscreen_report.blp`), shows the
//! report in large monospace text a page at a time ([`pindu_core::report_screen`]) for a
//! phone to photograph, and each time it opens a copy is saved to the reports folder,
//! whose path it shows. LB and RB, or its Previous Page and Next Page, turn the pages, and
//! Up and Down scroll a page taller than the panel. Its Add a Game's Log leads to the games
//! with a log (the `"report-log"` page of Quick Access's stack); choosing one gathers the
//! report again with that game's newest log after it, and saves that copy. The main window
//! widens Quick Access while either level shows. Quick Access keeps the entry, which sits
//! in its shared list.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::diagnostics::LogChoice;
use pindu_core::gamepad::{Direction, Intent};
use pindu_core::model::GameId;
use pindu_core::panels::{Panel, QuickAccessLevel};
use pindu_core::report_screen::{PAGE_COLUMNS, PAGE_LINES, pages};
use pindu_core::{paths, reading};

use crate::diagnostics_service;
use crate::fullscreen::Sound;
use crate::quick_access::PinduQuickAccess;
use crate::window::PinduWindow;

/// Quick Access's width, as `window.blp` sets it: the most it takes, and its share of the
/// window.
const PANEL_WIDTH: (f64, f64) = (440.0, 0.4);

/// Its width while the report shows, for longer lines in larger text.
const REPORT_WIDTH: (f64, f64) = (960.0, 0.5);

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/fullscreen_report.ui")]
    pub struct PinduReportLevel {
        #[template_child]
        pub report_counter: TemplateChild<gtk::Label>,
        #[template_child]
        pub report_scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub report_content: TemplateChild<gtk::Box>,
        #[template_child]
        pub report_spinner: TemplateChild<adw::Spinner>,
        #[template_child]
        pub report_text: TemplateChild<gtk::Label>,
        #[template_child]
        pub report_saved: TemplateChild<gtk::Label>,
        #[template_child]
        pub report_actions: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub report_previous: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub report_add_log: TemplateChild<gtk::ListBoxRow>,
        /// The report's pages, and the one shown.
        pub pages: RefCell<Vec<String>>,
        pub page: Cell<usize>,
        /// How often the level has opened, so that a report gathered for an earlier
        /// opening does not replace a newer one.
        pub openings: Cell<u64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduReportLevel {
        const NAME: &'static str = "PinduReportLevel";
        type Type = super::PinduReportLevel;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduReportLevel {}

    impl WidgetImpl for PinduReportLevel {}

    impl BinImpl for PinduReportLevel {}
}

glib::wrapper! {
    /// Quick Access's Diagnostics Report level: the report a page at a time, the page
    /// counter, where its copy was saved, and Previous Page and Next Page. It is the
    /// `"diagnostics-report"` page of Quick Access's stack.
    pub struct PinduReportLevel(ObjectSubclass<imp::PinduReportLevel>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PinduReportLevel {
    /// The level's lists, whose Up and Down go on past their ends.
    pub(crate) fn lists(&self) -> [gtk::ListBox; 1] {
        [self.imp().report_actions.get()]
    }

    /// Where focus goes on the level: the report, or Previous Page while it is gathered.
    pub(crate) fn focus_target(&self) -> Option<gtk::Widget> {
        let imp = self.imp();
        if imp.report_text.is_visible() {
            Some(imp.report_text.get().upcast())
        } else {
            Some(imp.report_previous.get().upcast())
        }
    }

    /// Add a Game's Log, where focus goes back to from the games with a log.
    pub(crate) fn add_log_row(&self) -> gtk::Widget {
        self.imp().report_add_log.get().upcast()
    }

    /// The report's text is the new `report`, shown from its first page; focus goes to it
    /// when it was waiting on Previous Page for it.
    pub(crate) fn show_report(&self, report: &str) {
        let imp = self.imp();
        imp.pages.replace(pages(report, PAGE_COLUMNS, PAGE_LINES));
        imp.page.set(0);
        imp.report_spinner.set_visible(false);
        imp.report_text.set_visible(true);
        self.show_page();
        if imp.report_previous.has_focus() {
            imp.report_text.grab_focus();
        }
    }

    /// Turns to the page after, or before, and says whether there was one.
    pub(crate) fn step_page(&self, forward: bool) -> bool {
        let imp = self.imp();
        let count = imp.pages.borrow().len();
        let page = imp.page.get();
        let next = if forward {
            (page + 1 < count).then_some(page + 1)
        } else {
            page.checked_sub(1)
        };
        let Some(next) = next else {
            return false;
        };
        imp.page.set(next);
        self.show_page();
        true
    }

    /// Up or Down while the report has focus: scrolls through a page taller than the panel
    /// shows, and says whether it did, rather than let focus move on
    /// (`pindu_core::reading`).
    pub(crate) fn scroll(&self, direction: Direction) -> bool {
        let imp = self.imp();
        if !imp.report_text.has_focus() {
            return false;
        }
        let Some(bounds) = imp.report_text.compute_bounds(&*imp.report_content) else {
            return false;
        };
        let adjustment = imp.report_scroller.vadjustment();
        let view = reading::View {
            top: adjustment.value(),
            height: adjustment.page_size(),
        };
        let text = reading::Span {
            top: f64::from(bounds.y()),
            bottom: f64::from(bounds.y() + bounds.height()),
        };
        match reading::step(view, text, direction, true) {
            reading::Step::Scroll(top) => {
                adjustment.set_value(top);
                true
            }
            reading::Step::Stay => true,
            reading::Step::MoveOn => false,
        }
    }

    /// The level opens: the spinner shows until the report is gathered, and the last
    /// report's page and path go. Gives the opening's number, for [`Self::is_opening`].
    fn open(&self) -> u64 {
        let imp = self.imp();
        let opening = imp.openings.get() + 1;
        imp.openings.set(opening);
        imp.pages.replace(Vec::new());
        imp.page.set(0);
        imp.report_counter.set_label("");
        imp.report_text.set_visible(false);
        imp.report_spinner.set_visible(true);
        imp.report_saved.set_visible(false);
        opening
    }

    /// Whether `opening` is the level's latest.
    fn is_opening(&self, opening: u64) -> bool {
        self.imp().openings.get() == opening
    }

    /// Says where the report's copy was saved, with the home folder as `~`, or why it was
    /// not.
    fn show_saved(&self, saved: Result<PathBuf, String>) {
        let label = match saved {
            Ok(path) => {
                let home = std::env::home_dir().unwrap_or_default();
                gettext("A copy is saved as {path}")
                    .replace("{path}", &paths::home_relative(&path, &home))
            }
            Err(error) => gettext("Could not save a copy: {error}").replace("{error}", &error),
        };
        let saved_label = &self.imp().report_saved;
        saved_label.set_label(&label);
        saved_label.set_visible(true);
    }

    /// Shows the current page from its top, with its counter.
    fn show_page(&self) {
        let imp = self.imp();
        let pages = imp.pages.borrow();
        let page = imp.page.get();
        imp.report_text
            .set_label(pages.get(page).map_or("", String::as_str));
        imp.report_counter.set_label(
            &gettext("Page {page} of {pages}")
                .replace("{page}", &(page + 1).to_string())
                .replace("{pages}", &pages.len().to_string()),
        );
        imp.report_scroller.vadjustment().set_value(0.0);
    }
}

impl PinduQuickAccess {
    /// Adds `win.diagnostics-report-previous` and `win.diagnostics-report-next`, which turn
    /// the report's pages, and `win.diagnostics-report-log`, which gathers the report again
    /// with the newest log of the game whose ID it is given.
    pub(crate) fn setup_diagnostics_report(&self, actions: &impl IsA<gio::ActionMap>) {
        for (name, forward) in [
            ("diagnostics-report-previous", false),
            ("diagnostics-report-next", true),
        ] {
            self.add_host_action(actions, name, None, move |quick_access, _| {
                quick_access.imp().report_level.step_page(forward);
            });
        }
        self.add_host_action(
            actions,
            "diagnostics-report-log",
            Some(glib::VariantTy::STRING),
            |quick_access, target| {
                let Some(id) = target
                    .and_then(glib::Variant::str)
                    .and_then(|id| id.parse::<GameId>().ok())
                else {
                    return;
                };
                quick_access.show_diagnostics_report(Some(id));
                quick_access.request_level(QuickAccessLevel::DiagnosticsReport);
            },
        );
    }

    /// The level opens: the report is gathered again, with `game`'s newest log after it
    /// when there is one, and shown, and a copy is saved.
    pub(crate) fn show_diagnostics_report(&self, game: Option<GameId>) {
        let Some(application) = self.application() else {
            return;
        };
        let level = self.imp().report_level.get();
        let opening = level.open();
        let steam_gamescope = application.gamescope_service().is_active();
        let own_session = application.runs_as_own_session();
        glib::spawn_future_local(async move {
            let report = diagnostics_service::debug_info(steam_gamescope, own_session, game).await;
            if !level.is_opening(opening) {
                return;
            }
            level.show_report(&report);
            let saved = diagnostics_service::save_report(report).await;
            if level.is_opening(opening) {
                level.show_saved(saved);
            }
        });
    }

    /// The games with a log open: they are listed, the game played with logs last first,
    /// and the first takes focus; or, when none has a log, how to get one shows.
    pub(crate) fn show_report_logs(&self) {
        let quick_access = self.downgrade();
        glib::spawn_future_local(async move {
            let choices = diagnostics_service::log_choices().await;
            let Some(quick_access) = quick_access.upgrade() else {
                return;
            };
            if quick_access.level() != Some(QuickAccessLevel::ReportLog) {
                return;
            }
            let imp = quick_access.imp();
            imp.report_logs.remove_all();
            for choice in &choices {
                imp.report_logs.append(&log_row(choice));
            }
            imp.report_logs.set_visible(!choices.is_empty());
            imp.report_logs_empty.set_visible(choices.is_empty());
            quick_access.focus_level(None);
        });
    }

    /// LB or RB while the report shows: turns its page, and says whether it did.
    pub(crate) fn step_report_page(&self, forward: bool) -> bool {
        self.level() == Some(QuickAccessLevel::DiagnosticsReport)
            && self.imp().report_level.step_page(forward)
    }

    /// Up or Down while the report shows: scrolls it (see [`PinduReportLevel::scroll`]).
    pub(crate) fn scroll_report(&self, direction: Direction) -> bool {
        self.level() == Some(QuickAccessLevel::DiagnosticsReport)
            && self.imp().report_level.scroll(direction)
    }
}

impl PinduWindow {
    /// LB and RB turn the report's pages while it shows, before the open panel takes them;
    /// says whether the intent was one of them.
    pub(crate) fn report_page_intent(&self, intent: Intent) -> bool {
        let forward = match intent {
            Intent::PreviousTab => false,
            Intent::NextTab => true,
            _ => return false,
        };
        if self.open_panel() != Some(Panel::QuickAccess(QuickAccessLevel::DiagnosticsReport)) {
            return false;
        }
        if self.imp().quick_access_panel.step_report_page(forward) {
            self.play_sound(Sound::Move);
        }
        true
    }

    /// Widens Quick Access while it shows `level`, if that is the report or the games with
    /// a log, and narrows it again after.
    pub(crate) fn fit_quick_access(&self, level: Option<QuickAccessLevel>) {
        let (width, fraction) = if matches!(
            level,
            Some(QuickAccessLevel::DiagnosticsReport | QuickAccessLevel::ReportLog)
        ) {
            REPORT_WIDTH
        } else {
            PANEL_WIDTH
        };
        let split_view = &self.imp().quick_access;
        split_view.set_max_sidebar_width(width);
        split_view.set_sidebar_width_fraction(fraction);
    }
}

/// A game with a log: its title and its newest log's name; choosing it adds that log to the
/// report.
fn log_row(choice: &LogChoice) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&choice.title)
        .subtitle(&choice.newest)
        .use_markup(false)
        .activatable(true)
        .build();
    row.set_action_name(Some("win.diagnostics-report-log"));
    row.set_action_target_value(Some(&choice.id.as_str().to_variant()));
    row
}
