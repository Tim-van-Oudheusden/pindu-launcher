//! The one-time summary of what the library's first sync found
//! (`pindu_core::health::first_sync_summary`): the launchers found, the games imported
//! and what needs attention, pointing at Library Health. Both shells show it as an Adwaita
//! alert, which a controller reads and answers alone in fullscreen mode; Open Library
//! Health opens the desktop window's page, or in fullscreen mode Quick Access at its
//! Library Health level. Either answer is the last time it shows
//! (`first-sync-summary-shown`).

use adw::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::glib;
use pindu_core::health::FirstSyncSummary;
use pindu_core::panels::{Panel, QuickAccessLevel};

use crate::health_service::problem_title;
use crate::sources::source_list;
use crate::window::PinduWindow;

/// The GSettings key that records the summary was dismissed.
pub const SHOWN_KEY: &str = "first-sync-summary-shown";

impl PinduWindow {
    /// Shows `summary` over the shell the window is in, and records its dismissal.
    pub fn show_first_sync_summary(&self, summary: &FirstSyncSummary) {
        let attention = !summary.problems.is_empty();
        let dialog = adw::AlertDialog::builder()
            .heading(gettext("Your Library Is Ready"))
            .body(summary_body(summary))
            .close_response("close")
            .default_response(if attention { "health" } else { "close" })
            .build();
        dialog.add_responses(&[
            ("close", &gettext("_Close")),
            ("health", &gettext("_Open Library Health")),
        ]);
        if attention {
            dialog.set_response_appearance("health", adw::ResponseAppearance::Suggested);
        }
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| {
                    if let Err(error) = window.settings().set_boolean(SHOWN_KEY, true) {
                        glib::g_warning!("pindu", "Could not note the first sync summary: {error}");
                    }
                    if response == "health" {
                        window.open_library_health();
                    }
                }
            ),
        );
        dialog.present(Some(self));
    }

    /// Library Health in the shell the window is in: Quick Access's level in fullscreen
    /// mode, the page of its own in the desktop window.
    fn open_library_health(&self) {
        if self.is_fullscreen() {
            self.show_panel(Panel::QuickAccess(QuickAccessLevel::LibraryHealth));
        } else {
            WidgetExt::activate_action(self, "win.library-health", None)
                .expect("the window has win.library-health");
        }
    }
}

/// The summary in sentences: the launchers found and the games imported, then what needs
/// attention, one problem a line.
fn summary_body(summary: &FirstSyncSummary) -> String {
    let launchers = if summary.launchers.is_empty() {
        gettext("No launchers were found on this computer.")
    } else {
        // Translators: {launchers} names launchers, such as "Steam and Lutris".
        gettext("Found {launchers}.").replace("{launchers}", &source_list(&summary.launchers))
    };
    let games = match summary.games {
        0 => gettext("No games were imported."),
        count => ngettext(
            "Imported one game.",
            "Imported {count} games.",
            count as u32,
        )
        .replace("{count}", &count.to_string()),
    };
    let attention = match summary.problems.len() {
        0 => gettext("Nothing needs attention."),
        count => {
            let heading = ngettext(
                "One thing needs attention:",
                "{count} things need attention:",
                count as u32,
            )
            .replace("{count}", &count.to_string());
            let lines: Vec<String> = summary
                .problems
                .iter()
                .map(|problem| format!("• {}", problem_title(problem)))
                .collect();
            format!("{heading}\n{}", lines.join("\n"))
        }
    };
    format!("{launchers} {games}\n\n{attention}")
}
