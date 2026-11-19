//! Quick Access's Duplicates: the Duplicates entry's level, which lists the duplicate
//! suggestions and then the groups the sync made (see `pindu_core::duplicates::review`),
//! worded as on the desktop's Duplicates page (`duplicate_review`). A suggestion opens to
//! Group, which groups the pair, and Not the Same Game, which remembers the pair so it is
//! never suggested again; a group the sync made opens to Undo, which splits it and keeps
//! the pair apart. Everything is chosen with a controller alone. The list is read again as
//! the level opens, after each sync and after each choice.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::duplicates::Review;
use pindu_core::panels::QuickAccessLevel;

use crate::application::PinduApplication;
use crate::duplicate_review::{group_subtitle, group_title, suggestion_subtitle, suggestion_title};
use crate::library_service::LibraryService;
use crate::quick_access::PinduQuickAccess;

impl PinduQuickAccess {
    /// Reads the review again after each sync while the level shows: a sync may group
    /// copies or bring new ones.
    pub(crate) fn setup_duplicates(&self, application: &PinduApplication) {
        let quick_access = self.downgrade();
        application.sync_service().connect_finished(move |_| {
            if let Some(quick_access) = quick_access.upgrade()
                && quick_access.level() == Some(QuickAccessLevel::Duplicates)
            {
                quick_access.show_duplicates();
            }
        });
    }

    /// Fills the Duplicates list from the review, keeping focus on the row it was on (or
    /// the one now in its place), or giving it to the first row when the level has just
    /// opened.
    pub(crate) fn show_duplicates(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let library = application.library_service().clone();
        let quick_access = self.downgrade();
        glib::spawn_future_local(async move {
            let review = library.duplicate_review().await;
            let Some(quick_access) = quick_access.upgrade() else {
                return;
            };
            if quick_access.level() != Some(QuickAccessLevel::Duplicates) {
                return;
            }
            quick_access.fill_duplicates(&library, review.as_ref());
        });
    }

    fn fill_duplicates(&self, library: &LibraryService, review: Option<&Review>) {
        let imp = self.imp();
        let list = imp.duplicates.get();
        let focused = self
            .focus_widget()
            .filter(|focus| focus.is_ancestor(&list))
            .and_then(|focus| {
                // The top row, not one of the rows an expander opens to.
                std::iter::successors(Some(focus), gtk::Widget::parent)
                    .filter_map(|widget| widget.downcast::<gtk::ListBoxRow>().ok())
                    .find(|row| row.parent().as_ref() == Some(list.upcast_ref()))
            })
            .map_or(0, |row| row.index());
        list.remove_all();
        if let Some(review) = review {
            for suggestion in &review.suggestions {
                let row = adw::ExpanderRow::builder()
                    .title(suggestion_title(review, suggestion))
                    .subtitle(suggestion_subtitle(suggestion))
                    .use_markup(false)
                    .build();
                let (first, second) = (suggestion.first.clone(), suggestion.second.clone());
                row.add_row(&self.choice_row(&gettext("Group"), false, library, {
                    let games = vec![first.clone(), second.clone()];
                    move |library| {
                        let games = games.clone();
                        async move {
                            library.merge_games(games).await;
                        }
                    }
                }));
                row.add_row(&self.choice_row(
                    &gettext("Not the Same Game"),
                    true,
                    library,
                    move |library| {
                        let (first, second) = (first.clone(), second.clone());
                        async move { library.dismiss_duplicate(&first, &second).await }
                    },
                ));
                list.append(&row);
            }
            for group in &review.automatic {
                let row = adw::ExpanderRow::builder()
                    .title(group_title(review, group))
                    .subtitle(group_subtitle(group))
                    .use_markup(false)
                    .build();
                let Some(member) = group.members.first().cloned() else {
                    continue;
                };
                row.add_row(&self.choice_row(
                    &gettext("Undo Grouping"),
                    true,
                    library,
                    move |library| {
                        let member = member.clone();
                        async move {
                            library.split_group(&member).await;
                        }
                    },
                ));
                list.append(&row);
            }
        }
        let empty = list.row_at_index(0).is_none();
        list.set_visible(!empty);
        imp.duplicates_empty.set_visible(empty);
        let row = list.row_at_index(focused).or_else(|| {
            focused
                .checked_sub(1)
                .and_then(|index| list.row_at_index(index))
        });
        if let Some(row) = row {
            row.grab_focus();
        }
    }

    /// A row a suggestion or a group opens to: `title`, which runs `choose` and then
    /// reads the review again.
    fn choice_row<F, Fut>(
        &self,
        title: &str,
        destructive: bool,
        library: &LibraryService,
        choose: F,
    ) -> adw::ButtonRow
    where
        F: Fn(LibraryService) -> Fut + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        let row = adw::ButtonRow::builder().title(title).build();
        if destructive {
            row.add_css_class("destructive-action");
        }
        let quick_access = self.downgrade();
        let library = library.clone();
        row.connect_activated(move |row| {
            row.set_sensitive(false);
            let choice = choose(library.clone());
            let quick_access = quick_access.clone();
            glib::spawn_future_local(async move {
                choice.await;
                if let Some(quick_access) = quick_access.upgrade() {
                    quick_access.show_duplicates();
                }
            });
        });
        row
    }
}
