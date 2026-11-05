//! Fullscreen mode's Home page, where the mode opens: the hero of the focused game over a
//! carousel of the pinned and recently played games (the "Jump back in" games), with a
//! Library tile at the end. Focus comes back to the card that last had it.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::focus::FocusMemory;
use pindu_core::library::ArtSet;
use pindu_core::model::GameId;

use crate::models::GameObject;
use crate::widgets::textures;
use crate::window::jump_card;

/// The size of the carousel's cards, the Library tile's as set in `home_page.blp`.
const CARD_SIZE: (i32, i32) = (320, 150);
/// Room kept beside the focused card when the carousel scrolls, as wide as its margins.
const SCROLL_MARGIN: f64 = 48.0;

/// What a card shows of its game, to rebuild the carousel only when that changes.
type CardKey = (GameId, String, Option<u8>, ArtSet);

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/home_page.ui")]
    pub struct HomePage {
        #[template_child]
        pub hero: TemplateChild<gtk::Picture>,
        #[template_child]
        pub logo: TemplateChild<gtk::Picture>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub carousel: TemplateChild<gtk::Box>,
        #[template_child]
        pub library_tile: TemplateChild<gtk::Button>,
        /// What the cards show, in carousel order.
        pub shown: RefCell<Vec<CardKey>>,
        /// The game cards, in carousel order, each with its game.
        pub cards: RefCell<Vec<(gtk::Button, GameObject)>>,
        /// The card that last had focus: its game, or `None` for the Library tile.
        pub focus_memory: RefCell<FocusMemory<(), Option<GameId>>>,
        /// Bumped for every hero shown, so art that loads late for an earlier one is dropped.
        pub hero_generation: Cell<u64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HomePage {
        const NAME: &'static str = "PinduHomePage";
        type Type = super::HomePage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for HomePage {
        fn constructed(&self) {
            self.parent_constructed();
            let page = self.obj();
            self.library_tile.connect_has_focus_notify(glib::clone!(
                #[weak]
                page,
                move |tile| {
                    if tile.has_focus() {
                        page.card_focused(tile, None);
                    }
                }
            ));
            // The page scrolls the carousel itself, keeping the lifted card's ring in view.
            if let Some(viewport) = self.carousel.parent().and_downcast::<gtk::Viewport>() {
                viewport.set_scroll_to_focus(false);
            }
            page.show_hero(None);
        }
    }

    impl WidgetImpl for HomePage {}

    impl NavigationPageImpl for HomePage {
        // Coming back to Home focuses the card that had focus when it was left.
        fn shown(&self) {
            self.parent_shown();
            self.obj().focus_remembered();
        }
    }
}

glib::wrapper! {
    pub struct HomePage(ObjectSubclass<imp::HomePage>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl HomePage {
    /// Shows `games` in the carousel, before the Library tile, rebuilding the cards only
    /// when what they show changed.
    pub fn set_games(&self, games: &[GameObject]) {
        let imp = self.imp();
        let keys: Vec<CardKey> = games
            .iter()
            .map(|game| {
                let entry = game.entry();
                (
                    entry.game.id.clone(),
                    entry.game.title.clone(),
                    entry.game.pin_order,
                    entry.art.clone(),
                )
            })
            .collect();
        if *imp.shown.borrow() == keys {
            return;
        }
        imp.shown.replace(keys);
        // Focus that went to the Library tile only because no games were shown yet goes
        // to the first game once they are.
        let first_games = imp.cards.borrow().is_empty() && imp.library_tile.is_focus();
        for (card, _) in imp.cards.take() {
            imp.carousel.remove(&card);
        }
        let mut cards = Vec::with_capacity(games.len());
        for game in games {
            let card = jump_card(game, CARD_SIZE);
            card.connect_has_focus_notify(glib::clone!(
                #[weak(rename_to = page)]
                self,
                #[weak]
                game,
                move |card| {
                    if card.has_focus() {
                        page.card_focused(card, Some(&game));
                    }
                }
            ));
            imp.carousel
                .insert_child_after(&card, cards.last().map(|(card, _)| card));
            cards.push((card, game.clone()));
        }
        imp.cards.replace(cards);
        if first_games && !games.is_empty() {
            imp.focus_memory
                .borrow_mut()
                .remember((), Some(games[0].id()));
            self.focus_remembered();
        } else if !self.carousel_has_focus() {
            // The hero follows the card that focus comes back to.
            self.show_hero(self.remembered().1.as_ref());
        }
    }

    /// Focuses the card that last had focus, wherever it moved to, or the first card when
    /// it is gone.
    pub fn focus_remembered(&self) {
        let (card, game) = self.remembered();
        card.grab_focus();
        self.show_hero(game.as_ref());
    }

    /// The card that last had focus, with its game (`None` for the Library tile).
    fn remembered(&self) -> (gtk::Button, Option<GameObject>) {
        let imp = self.imp();
        let cards = imp.cards.borrow();
        let items: Vec<Option<GameId>> = cards
            .iter()
            .map(|(_, game)| Some(game.id()))
            .chain([None])
            .collect();
        let position = imp.focus_memory.borrow().position(&(), &items);
        match cards.get(position) {
            Some((card, game)) => (card.clone(), Some(game.clone())),
            None => (imp.library_tile.get(), None),
        }
    }

    /// The game of the card with focus, if a game's card has it.
    pub fn focused_game(&self) -> Option<GameObject> {
        let cards = self.imp().cards.borrow();
        cards
            .iter()
            .find(|(card, _)| card.has_focus())
            .map(|(_, game)| game.clone())
    }

    fn carousel_has_focus(&self) -> bool {
        self.imp()
            .carousel
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_WITHIN)
    }

    /// Notes the focused card, scrolls it into view and shows its game in the hero.
    fn card_focused(&self, card: &gtk::Button, game: Option<&GameObject>) {
        let imp = self.imp();
        imp.focus_memory
            .borrow_mut()
            .remember((), game.map(GameObject::id));
        if let Some(viewport) = imp.carousel.parent()
            && let Some(adjustment) = viewport
                .downcast_ref::<gtk::Viewport>()
                .and_then(|viewport| viewport.hadjustment())
            && let Some(bounds) = card.compute_bounds(&viewport)
        {
            let start = f64::from(bounds.x()) + adjustment.value();
            let end = start + f64::from(bounds.width());
            adjustment.clamp_page(start - SCROLL_MARGIN, end + SCROLL_MARGIN);
        }
        self.show_hero(game);
    }

    /// Shows the game's hero art (or its cover) with its logo (or its title); the Library
    /// tile shows the title "Library" without art.
    fn show_hero(&self, game: Option<&GameObject>) {
        let imp = self.imp();
        let generation = imp.hero_generation.get() + 1;
        imp.hero_generation.set(generation);
        imp.hero.set_paintable(gtk::gdk::Paintable::NONE);
        imp.logo.set_paintable(gtk::gdk::Paintable::NONE);
        imp.logo.set_visible(false);
        imp.title.set_visible(true);
        let Some(game) = game else {
            imp.title.set_label(&gettext("Library"));
            return;
        };
        let (title, art, logo) = {
            let entry = game.entry();
            (
                entry.game.title.clone(),
                entry.art.hero.clone().or_else(|| entry.art.cover.clone()),
                entry.art.logo.clone(),
            )
        };
        imp.title.set_label(&title);
        // Art in memory shows at once, so moving along the carousel does not flicker.
        let art = art.filter(|path| match textures::cached(path) {
            Some(texture) => {
                imp.hero.set_paintable(Some(&texture));
                false
            }
            None => true,
        });
        let logo = logo.filter(|path| match textures::cached(path) {
            Some(texture) => {
                self.show_logo(&texture);
                false
            }
            None => true,
        });
        if art.is_none() && logo.is_none() {
            return;
        }
        let page = self.downgrade();
        glib::spawn_future_local(async move {
            if let Some(path) = art
                && let Ok(texture) = textures::load(&path).await
                && let Some(page) = page.upgrade()
                && page.imp().hero_generation.get() == generation
            {
                page.imp().hero.set_paintable(Some(&texture));
            }
            if let Some(path) = logo
                && let Ok(texture) = textures::load(&path).await
                && let Some(page) = page.upgrade()
                && page.imp().hero_generation.get() == generation
            {
                page.show_logo(&texture);
            }
        });
    }

    fn show_logo(&self, texture: &gtk::gdk::Texture) {
        let imp = self.imp();
        imp.logo.set_paintable(Some(texture));
        imp.logo.set_visible(true);
        imp.title.set_visible(false);
    }
}
