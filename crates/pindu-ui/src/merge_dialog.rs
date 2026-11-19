//! The desktop window's Merge With dialog: a search over the other games, likely copies of
//! the game first (`pindu_core::game_groups::merge_candidates`), each row naming where the
//! game comes from. Choosing one hands it to the caller, which merges the two; fullscreen
//! mode lists the same games in its options sheet instead.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::ngettext;
use gtk::glib;
use pindu_core::game_groups::merge_candidates;
use pindu_core::model::{GameId, Source};

use crate::models::GameObject;
use crate::sources::source_name;

/// What the caller does with the chosen game.
type Chosen = Box<dyn FnOnce(GameId)>;

/// The game the dialog is for, the other games, and what to do with the one chosen.
#[derive(Default)]
pub struct Choice {
    game: RefCell<Option<GameObject>>,
    games: RefCell<Vec<GameObject>>,
    shown: RefCell<Vec<GameId>>,
    chosen: RefCell<Option<Chosen>>,
}

impl fmt::Debug for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Choice")
            .field("shown", &self.shown.borrow().len())
            .field("chosen", &self.chosen.borrow().is_none())
            .finish()
    }
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/merge_dialog.ui")]
    pub struct MergeDialog {
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub list: TemplateChild<gtk::ListBox>,
        pub choice: Choice,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MergeDialog {
        const NAME: &'static str = "PinduMergeDialog";
        type Type = super::MergeDialog;
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
    impl MergeDialog {
        #[template_callback]
        fn search_changed(&self) {
            self.obj().fill();
        }

        #[template_callback]
        fn choose_first(&self) {
            let first = self.choice.shown.borrow().first().cloned();
            if let Some(game) = first {
                self.obj().pick(game);
            }
        }

        #[template_callback]
        fn row_activated(&self, row: &gtk::ListBoxRow) {
            let game = usize::try_from(row.index())
                .ok()
                .and_then(|index| self.choice.shown.borrow().get(index).cloned());
            if let Some(game) = game {
                self.obj().pick(game);
            }
        }
    }

    impl ObjectImpl for MergeDialog {}
    impl WidgetImpl for MergeDialog {}
    impl AdwDialogImpl for MergeDialog {}
}

glib::wrapper! {
    /// Asks which game to merge a game with.
    pub struct MergeDialog(ObjectSubclass<imp::MergeDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MergeDialog {
    /// Asks, over `parent`, which of `games` to merge `game` with, and hands the chosen
    /// one's ID to `chosen`; nothing when the dialog is closed.
    pub fn choose(
        parent: &impl IsA<gtk::Widget>,
        game: &GameObject,
        games: Vec<GameObject>,
        chosen: impl FnOnce(GameId) + 'static,
    ) {
        let dialog: Self = glib::Object::new();
        let choice = &dialog.imp().choice;
        choice.game.replace(Some(game.clone()));
        choice.games.replace(games);
        choice.chosen.replace(Some(Box::new(chosen)));
        dialog.fill();
        dialog.present(Some(parent));
    }

    /// Lists the games that match the search.
    fn fill(&self) {
        let imp = self.imp();
        let choice = &imp.choice;
        let Some(game) = choice.game.borrow().clone() else {
            return;
        };
        let rows = candidate_rows(&game, &choice.games.borrow(), &imp.search_entry.text());
        imp.list.remove_all();
        let mut shown = Vec::with_capacity(rows.len());
        for (id, row) in rows {
            imp.list.append(&row);
            shown.push(id);
        }
        imp.stack
            .set_visible_child_name(if shown.is_empty() { "empty" } else { "games" });
        choice.shown.replace(shown);
    }

    /// Hands `game` to the caller and closes.
    fn pick(&self, game: GameId) {
        if let Some(chosen) = self.imp().choice.chosen.take() {
            chosen(game);
        }
        self.close();
    }
}

/// The rows of the games to merge `game` with among `games` (one per group) whose titles
/// match `query`, likely copies first, each with its ID.
pub(crate) fn candidate_rows(
    game: &GameObject,
    games: &[GameObject],
    query: &str,
) -> Vec<(GameId, adw::ActionRow)> {
    let entries: Vec<_> = games.iter().map(GameObject::entry).collect();
    let copies: HashMap<&GameId, usize> = entries
        .iter()
        .map(|entry| (&entry.game.id, entry.copies.len()))
        .collect();
    let target = game.game();
    merge_candidates(&target, entries.iter().map(|entry| &entry.game), query)
        .into_iter()
        .map(|candidate| {
            let copies = copies.get(&candidate.id).copied().unwrap_or(0);
            let row = candidate_row(&candidate.title, candidate.source(), copies);
            (candidate.id.clone(), row)
        })
        .collect()
}

/// A game to merge with: its title, and where it comes from or how many copies it has.
fn candidate_row(title: &str, source: Source, copies: usize) -> adw::ActionRow {
    let subtitle = if copies > 1 {
        ngettext("{n} copy", "{n} copies", copies as u32).replace("{n}", &copies.to_string())
    } else {
        source_name(source)
    };
    adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .use_markup(false)
        .activatable(true)
        .build()
}
