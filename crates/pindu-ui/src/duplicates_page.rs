//! Duplicates in the desktop window: a page of its own, opened from the main menu, that
//! lists the duplicate suggestions with their reason and confidence and the groups the
//! sync made (see `pindu_core::duplicates::review`), worded as in fullscreen mode
//! (`duplicate_review`). A suggestion's Group groups the pair and its Not the Same Game
//! remembers the pair so it is never suggested again; a group's Undo splits it and keeps
//! the pair apart. The page reads the review again as it shows, after each sync and after
//! each choice.

use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::duplicates::Review;

use crate::duplicate_review::{group_subtitle, group_title, suggestion_subtitle, suggestion_title};
use crate::library_service::LibraryService;
use crate::sync_service::SyncService;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/duplicates_page.ui")]
    pub struct DuplicatesPage {
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub suggestions_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub automatic_group: TemplateChild<adw::PreferencesGroup>,
        /// The service the page reads and writes the review through.
        pub library: RefCell<Option<LibraryService>>,
        /// The sync service and its `finished` handler.
        pub sync: RefCell<Option<(SyncService, glib::SignalHandlerId)>>,
        /// The rows the review filled in, with their group, to take out on the next one.
        pub rows: RefCell<Vec<(adw::PreferencesGroup, adw::ActionRow)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DuplicatesPage {
        const NAME: &'static str = "PinduDuplicatesPage";
        type Type = super::DuplicatesPage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for DuplicatesPage {
        fn dispose(&self) {
            if let Some((service, handler)) = self.sync.take() {
                service.disconnect(handler);
            }
        }
    }

    impl WidgetImpl for DuplicatesPage {}

    impl NavigationPageImpl for DuplicatesPage {
        // Back from a game page, where copies may have been grouped or split.
        fn shown(&self) {
            self.parent_shown();
            self.obj().refresh();
        }
    }
}

glib::wrapper! {
    /// The Duplicates page.
    pub struct DuplicatesPage(ObjectSubclass<imp::DuplicatesPage>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl DuplicatesPage {
    /// A page reviewing the library's duplicates through `library`, read again after each
    /// of `sync`'s syncs.
    pub fn new(library: &LibraryService, sync: &SyncService) -> Self {
        let page: Self = glib::Object::new();
        let imp = page.imp();
        imp.library.replace(Some(library.clone()));
        let handler = sync.connect_finished(glib::clone!(
            #[weak]
            page,
            move |_| page.refresh()
        ));
        imp.sync.replace(Some((sync.clone(), handler)));
        page
    }

    /// Reads the review and shows it.
    fn refresh(&self) {
        let Some(library) = self.imp().library.borrow().clone() else {
            return;
        };
        let page = self.downgrade();
        glib::spawn_future_local(async move {
            let review = library.duplicate_review().await;
            if let Some(page) = page.upgrade() {
                page.show(&library, review.as_ref());
            }
        });
    }

    /// Shows `review`; nothing to review shows the empty state.
    fn show(&self, library: &LibraryService, review: Option<&Review>) {
        let imp = self.imp();
        for (group, row) in imp.rows.take() {
            group.remove(&row);
        }
        let review =
            review.filter(|review| !review.suggestions.is_empty() || !review.automatic.is_empty());
        let Some(review) = review else {
            imp.stack.set_visible_child_name("empty");
            return;
        };
        imp.stack.set_visible_child_name("review");
        imp.suggestions_group
            .set_visible(!review.suggestions.is_empty());
        imp.automatic_group
            .set_visible(!review.automatic.is_empty());
        let mut rows = Vec::new();
        for suggestion in &review.suggestions {
            let row = review_row(
                &suggestion_title(review, suggestion),
                &suggestion_subtitle(suggestion),
            );
            let (first, second) = (suggestion.first.clone(), suggestion.second.clone());
            row.add_suffix(
                &self.choice_button(&gettext("Not the Same Game"), false, library, {
                    let (first, second) = (first.clone(), second.clone());
                    move |library| {
                        let (first, second) = (first.clone(), second.clone());
                        async move { library.dismiss_duplicate(&first, &second).await }
                    }
                }),
            );
            let games = vec![first, second];
            row.add_suffix(
                &self.choice_button(&gettext("Group"), true, library, move |library| {
                    let games = games.clone();
                    async move {
                        library.merge_games(games).await;
                    }
                }),
            );
            imp.suggestions_group.add(&row);
            rows.push((imp.suggestions_group.get(), row));
        }
        for group in &review.automatic {
            let row = review_row(&group_title(review, group), &group_subtitle(group));
            let Some(member) = group.members.first().cloned() else {
                continue;
            };
            row.add_suffix(
                &self.choice_button(&gettext("Undo"), false, library, move |library| {
                    let member = member.clone();
                    async move {
                        library.split_group(&member).await;
                    }
                }),
            );
            imp.automatic_group.add(&row);
            rows.push((imp.automatic_group.get(), row));
        }
        imp.rows.replace(rows);
    }

    /// A row's button: `label`, which runs `choose` and then reads the review again.
    fn choice_button<F, Fut>(
        &self,
        label: &str,
        suggested: bool,
        library: &LibraryService,
        choose: F,
    ) -> gtk::Button
    where
        F: Fn(LibraryService) -> Fut + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        let button = gtk::Button::builder()
            .label(label)
            .valign(gtk::Align::Center)
            .build();
        if suggested {
            button.add_css_class("suggested-action");
        }
        let page = self.downgrade();
        let library = library.clone();
        button.connect_clicked(move |button| {
            if let Some(row) = button.ancestor(adw::ActionRow::static_type()) {
                row.set_sensitive(false);
            }
            let choice = choose(library.clone());
            let page = page.clone();
            glib::spawn_future_local(async move {
                choice.await;
                if let Some(page) = page.upgrade() {
                    page.refresh();
                }
            });
        });
        button
    }
}

/// A suggestion's or a group's row, with its title and subtitle.
fn review_row(title: &str, subtitle: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .use_markup(false)
        .build()
}
