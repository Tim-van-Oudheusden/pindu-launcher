//! Fullscreen mode's Home page, where the mode opens, in two screens. The first lists
//! Home's sections (`pindu_core::sections`), a tile each, then the Library tile, over
//! Home's background, with the focused section's name and game count. Activating a section
//! (`win.home-section`) opens the second screen: its games, with the focused game's hero
//! behind them. B goes back to the first screen, on the section it came from. Each screen
//! comes back to the tile or card that last had focus. The page carries `.home-sections`
//! or `.home-games` for designs, which can choose the tiles' shape and size
//! (`pindu_core::home`) and crossfade the hero (`.shown`, `.no-art`). Behind the first
//! screen plays Home's background (`.home-background`), as `home::background_playback`
//! says for where the window is. Quick Access's Blur and Dim name their strength on the
//! page (`blur-*`, `dim-*`, `home::background_classes`), which blurs the background and
//! the hero and darkens them under `.home-dim`.

use std::cell::{Cell, RefCell};
use std::path::Path;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::glib;
use pindu_core::focus::FocusMemory;
use pindu_core::home::{self, BackgroundContext, CardArt, Detail, Playback, Strength};
use pindu_core::library::{ArtSet, HistoryGroup};
use pindu_core::model::GameId;
use pindu_core::sections::{Section, SectionId};
use pindu_core::themes::{HomeLayout, ThemeLayout, TileShape};

use crate::config::{PKGDATADIR, RESOURCE_BASE_PATH};
use crate::game_page::{date, playtime};
use crate::models::GameObject;
use crate::sources::{source_icon, source_name};
use crate::widgets::{CoverArt, textures};
use crate::window::jump_card_frame;

/// A `.detail` item of Home's details row: an icon and the value, with what it is as its
/// tooltip and description. The wording is the game page's.
fn detail_item(detail: Detail) -> gtk::Box {
    let (icon, value, caption) = match detail {
        Detail::Playtime(secs) => (
            "media-playback-start-symbolic",
            playtime(secs),
            gettext("Playtime"),
        ),
        Detail::LastPlayed { group, at } => {
            let value = match group {
                HistoryGroup::Today => gettext("Today"),
                HistoryGroup::ThisWeek => gettext("This Week"),
                HistoryGroup::Earlier => date(at),
            };
            (
                "document-open-recent-symbolic",
                value,
                gettext("Last Played"),
            )
        }
        Detail::Source(source) => (
            source_icon(source),
            source_name(source),
            gettext("Launcher"),
        ),
        Detail::Games(count) => ("view-grid-symbolic", game_count(count), gettext("Games")),
    };
    let item = gtk::Box::builder()
        .spacing(6)
        .tooltip_text(&caption)
        .css_classes(["detail"])
        .build();
    item.append(
        &gtk::Image::builder()
            .icon_name(icon)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build(),
    );
    item.append(&gtk::Label::new(Some(&value)));
    item.update_property(&[gtk::accessible::Property::Description(&caption)]);
    item
}

/// How many games a section holds, as its details row says it.
fn game_count(count: u32) -> String {
    ngettext("{count} game", "{count} games", count).replace("{count}", &count.to_string())
}

/// A section's name: the user's for a collection, else Home's own.
fn section_title(section: &Section) -> String {
    match (section.id, &section.title) {
        (_, Some(title)) => title.clone(),
        (SectionId::RecentlyPlayed, None) => gettext("Recently Played"),
        (SectionId::Favourites, None) => gettext("Favourites"),
        (SectionId::Installed, None) => gettext("Installed"),
        (SectionId::Source(source), None) => source_name(source),
        (SectionId::Collection(_), None) => String::new(),
    }
}

/// A section's tile (`.home-section-tile`), which opens it through `win.home-section`:
/// its art (`data/home/<name>.svg`) filling the tile, or for a collection, which has
/// none, an icon over its name (`.home-collection-tile`). The tile keeps its size
/// whatever the art's: the art is an overlay, which does not size it.
fn section_tile(section: &Section, title: &str, (width, height): (i32, i32)) -> gtk::Button {
    let frame = gtk::Overlay::builder()
        .child(
            &gtk::Box::builder()
                .width_request(width)
                .height_request(height)
                .build(),
        )
        .build();
    let tile = gtk::Button::builder()
        .child(&frame)
        .overflow(gtk::Overflow::Hidden)
        .tooltip_text(title)
        .action_name("win.home-section")
        .action_target(&section.id.name().to_variant())
        .css_classes(["card", "home-section-tile"])
        .build();
    if let SectionId::Collection(_) = section.id {
        tile.add_css_class("home-collection-tile");
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .valign(gtk::Align::Center)
            .margin_start(12)
            .margin_end(12)
            .build();
        content.append(
            &gtk::Image::builder()
                .icon_name("folder-symbolic")
                .pixel_size(48)
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build(),
        );
        content.append(
            &gtk::Label::builder()
                .label(title)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["heading"])
                .build(),
        );
        frame.add_overlay(&content);
    } else {
        let resource = format!("{RESOURCE_BASE_PATH}/home/{}.svg", section.id.name());
        let art = gtk::Picture::for_resource(&resource);
        art.set_content_fit(gtk::ContentFit::Cover);
        art.set_can_shrink(true);
        art.set_accessible_role(gtk::AccessibleRole::Presentation);
        frame.add_overlay(&art);
    }
    tile.update_property(&[gtk::accessible::Property::Label(title)]);
    tile
}

/// Room kept beside the focused card when the carousel scrolls, as wide as its margins;
/// padding a design gives the carousel, for a focused card it scales up, adds to it.
const SCROLL_MARGIN: f64 = 48.0;

/// What a card shows of its game, to rebuild the cards only when that changes.
type CardKey = (GameId, String, Option<u8>, ArtSet);

/// What a section's tile shows, to rebuild the tiles only when that changes.
type TileKey = (SectionId, String);

/// What has focus on Home, which the title, details and hero show.
#[derive(Debug, Clone)]
enum Focused {
    Section(SectionId),
    Game(GameObject),
    Library,
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/home_page.ui")]
    pub struct HomePage {
        #[template_child]
        pub background: TemplateChild<gtk::Picture>,
        /// Home's background video, loaded the first time the window is fullscreen.
        pub background_media: RefCell<Option<gtk::MediaFile>>,
        /// Where the window is, as the window last said (`set_background_context`).
        pub background_context: Cell<BackgroundContext>,
        #[template_child]
        pub hero_a: TemplateChild<gtk::Picture>,
        #[template_child]
        pub hero_b: TemplateChild<gtk::Picture>,
        /// Whether `hero_b`, not `hero_a`, holds the latest art shown.
        pub hero_b_latest: Cell<bool>,
        #[template_child]
        pub logo: TemplateChild<gtk::Picture>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub home: TemplateChild<gtk::Overlay>,
        #[template_child]
        pub content: TemplateChild<gtk::Box>,
        #[template_child]
        pub info: TemplateChild<gtk::Box>,
        #[template_child]
        pub details: TemplateChild<gtk::Box>,
        #[template_child]
        pub carousel_row: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub carousel: TemplateChild<gtk::Box>,
        #[template_child]
        pub library_tile: TemplateChild<gtk::Button>,
        #[template_child]
        pub library_tile_frame: TemplateChild<gtk::Box>,
        /// Home's sections, in order, each with its games.
        pub sections: RefCell<Vec<(Section, Vec<GameObject>)>>,
        /// The section whose games the second screen shows; `None` on the first screen.
        pub open: Cell<Option<SectionId>>,
        /// What the section tiles show, in carousel order.
        pub tiles_shown: RefCell<Vec<TileKey>>,
        /// The section tiles in the carousel, each with its section.
        pub tiles: RefCell<Vec<(gtk::Button, SectionId)>>,
        /// What the game cards show, in carousel order.
        pub shown: RefCell<Vec<CardKey>>,
        /// The game cards in the carousel, each with its game.
        pub cards: RefCell<Vec<(gtk::Button, GameObject)>>,
        /// The tile that last had focus on the first screen: its section, or `None` for
        /// the Library tile.
        pub section_focus: RefCell<FocusMemory<(), Option<SectionId>>>,
        /// The card that last had focus in each section's games.
        pub game_focus: RefCell<FocusMemory<SectionId, GameId>>,
        /// Bumped for every hero shown, so art that loads late for an earlier one is dropped.
        pub hero_generation: Cell<u64>,
        /// The layout of the design in force.
        pub layout: Cell<ThemeLayout>,
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
                        page.card_focused(tile, Focused::Library);
                    }
                }
            ));
            // The page scrolls the carousel itself, keeping the lifted card's ring in view.
            if let Some(viewport) = self.carousel.parent().and_downcast::<gtk::Viewport>() {
                viewport.set_scroll_to_focus(false);
            }
            page.show_home_layout(HomeLayout::default());
            page.show_tile_shape(TileShape::default(), None);
            page.show_screen_class();
            page.show_focused(&Focused::Library);
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
    /// Fullscreen mode's Home page.
    pub struct HomePage(ObjectSubclass<imp::HomePage>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl HomePage {
    /// Follows the layout of the design in force: where the carousel sits, the shape and
    /// size its tiles take, and whether the focused item's details show.
    pub fn set_layout(&self, layout: ThemeLayout) {
        let imp = self.imp();
        let old = imp.layout.replace(layout);
        if old.home != layout.home {
            self.show_home_layout(layout.home);
        }
        if old.home_details != layout.home_details
            && let Some((_, focused)) = self.remembered()
        {
            self.show_details(&focused);
        }
        if (old.home_tile, old.home_tile_size) != (layout.home_tile, layout.home_tile_size) {
            self.show_tile_shape(layout.home_tile, layout.home_tile_size);
            let had_focus = self.carousel_has_focus();
            imp.tiles_shown.replace(Vec::new());
            imp.shown.replace(Vec::new());
            self.refresh();
            if had_focus {
                self.focus_remembered();
            }
        }
    }

    /// Names Home's background `blur` and `dim` on the page for `style.css` and designs.
    pub fn set_background_look(&self, blur: Strength, dim: Strength) {
        let page = &self.imp().home;
        for class in home::BACKGROUND_CLASSES {
            page.remove_css_class(class);
        }
        for class in home::background_classes(blur, dim) {
            page.add_css_class(class);
        }
    }

    /// Puts the carousel at the bottom under the logo or title, or near the top above
    /// them, and names the layout on the page (`home-layout-hero-bottom`,
    /// `home-layout-row-top`) for designs. Moving the rows keeps focus where it is.
    fn show_home_layout(&self, layout: HomeLayout) {
        let imp = self.imp();
        for other in HomeLayout::ALL {
            imp.home
                .remove_css_class(&format!("home-layout-{}", other.name()));
        }
        imp.home
            .add_css_class(&format!("home-layout-{}", layout.name()));
        let content = &imp.content;
        match layout {
            HomeLayout::HeroBottom => {
                content.reorder_child_after(&*imp.carousel_row, Some(&*imp.info));
                content.set_valign(gtk::Align::End);
                content.set_margin_top(0);
                content.set_margin_bottom(24);
            }
            HomeLayout::RowTop => {
                content.reorder_child_after(&*imp.info, Some(&*imp.carousel_row));
                content.set_valign(gtk::Align::Start);
                content.set_margin_top(24);
                content.set_margin_bottom(0);
            }
        }
    }

    /// Sizes the Library tile for `shape` with its shorter side `size` pixels, if set, and
    /// names the shape on the carousel (`home-tile-landscape`, `home-tile-square`,
    /// `home-tile-portrait`) for designs.
    fn show_tile_shape(&self, shape: TileShape, size: Option<i32>) {
        let imp = self.imp();
        for other in TileShape::ALL {
            imp.carousel
                .remove_css_class(&format!("home-tile-{}", other.name()));
        }
        imp.carousel
            .add_css_class(&format!("home-tile-{}", shape.name()));
        let (width, height) = home::card_size(shape, size);
        imp.library_tile_frame.set_size_request(width, height);
    }

    /// Names the screen shown on the page for designs: `.home-sections` for the first,
    /// `.home-games` for a section's games.
    fn show_screen_class(&self) {
        let home = &self.imp().home;
        let games = self.imp().open.get().is_some();
        home.remove_css_class(if games { "home-sections" } else { "home-games" });
        home.add_css_class(if games { "home-games" } else { "home-sections" });
        self.show_background_playback();
    }

    /// Follows where the window is for Home's background. `first_screen` says whether
    /// Home is the shown page; the page itself knows whether its first screen shows.
    pub fn set_background_context(&self, context: BackgroundContext) {
        let imp = self.imp();
        imp.background_context.set(context);
        if context.fullscreen && imp.background_media.borrow().is_none() {
            self.load_background();
        }
        self.show_background_playback();
    }

    /// Loads Home's background (`home/background-*.webm` in the app's data, in the size
    /// `home::background_video` picks for the window's screen), muted and looped. A file
    /// that is missing or does not play is logged and leaves the black of
    /// `.home-background`.
    fn load_background(&self) {
        let media = gtk::MediaFile::new();
        // Looped by hand: with `loop` set, GTK's GStreamer player on GNOME 51 stops on the
        // last frame instead of starting again, and never says it ended.
        media.connect_ended_notify(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |media| {
                if media.is_ended() {
                    media.seek(0);
                    page.show_background_playback();
                }
            }
        ));
        media.connect_error_notify(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |media| {
                if let Some(error) = media.error() {
                    glib::g_warning!("pindu", "Could not play Home's background: {error}");
                    page.imp()
                        .background
                        .set_paintable(gtk::gdk::Paintable::NONE);
                }
            }
        ));
        // Holding the first frame needs a stream that can seek, which it is once prepared.
        media.connect_prepared_notify(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.show_background_playback()
        ));
        let imp = self.imp();
        imp.background.set_paintable(Some(&media));
        imp.background_media.replace(Some(media.clone()));
        if self.is_mapped() {
            self.open_background(&media);
        } else {
            // Fullscreen mode can start before the window is on a screen: the size is
            // known once Home shows, so the file is chosen then, once.
            let handler = std::rc::Rc::new(RefCell::new(None));
            let id = self.connect_map(glib::clone!(
                #[strong]
                handler,
                #[weak]
                media,
                move |page| {
                    page.open_background(&media);
                    if let Some(id) = handler.take() {
                        page.disconnect(id);
                    }
                }
            ));
            handler.replace(Some(id));
        }
    }

    /// Opens Home's background in the size `home::background_video` picks for the screen
    /// the window is on, muted.
    fn open_background(&self, media: &gtk::MediaFile) {
        media.set_filename(Some(
            Path::new(PKGDATADIR)
                .join("home")
                .join(home::background_video(self.screen_pixels())),
        ));
        // After the file: GStreamer's player, which takes the mute, comes with it.
        media.set_muted(true);
    }

    /// The size in device pixels of the screen the window is on, once it is shown.
    fn screen_pixels(&self) -> Option<(i32, i32)> {
        let surface = self.native()?.surface()?;
        let monitor = surface.display().monitor_at_surface(&surface)?;
        let geometry = monitor.geometry();
        let scale = monitor.scale();
        Some((
            (f64::from(geometry.width()) * scale).round() as i32,
            (f64::from(geometry.height()) * scale).round() as i32,
        ))
    }

    /// Plays, pauses or holds Home's background on its first frame, as
    /// `home::background_playback` says for where the window is and the screen shown.
    fn show_background_playback(&self) {
        let imp = self.imp();
        let Some(media) = imp.background_media.borrow().clone() else {
            return;
        };
        let context = imp.background_context.get();
        let context = BackgroundContext {
            first_screen: context.first_screen && imp.open.get().is_none(),
            ..context
        };
        match home::background_playback(context) {
            Playback::Play => media.play(),
            Playback::Pause => media.pause(),
            Playback::FirstFrame => {
                media.pause();
                if media.is_seekable() && media.timestamp() != 0 {
                    media.seek(0);
                }
            }
        }
    }

    /// Takes Home's sections, each with its games, and shows them on the screen shown. A
    /// section whose games show and that is gone now goes back to the first screen.
    pub fn set_sections(&self, sections: Vec<(Section, Vec<GameObject>)>) {
        self.imp().sections.replace(sections);
        self.refresh();
    }

    /// Shows the shown screen's tiles or cards again, after the sections or the tiles'
    /// shape changed.
    fn refresh(&self) {
        let imp = self.imp();
        let Some(open) = imp.open.get() else {
            self.show_tiles();
            return;
        };
        let games = imp
            .sections
            .borrow()
            .iter()
            .find(|(section, _)| section.id == open)
            .map(|(_, games)| games.clone());
        match games {
            Some(games) if !games.is_empty() => self.show_cards(&games),
            _ => self.show_sections(),
        }
    }

    /// Whether the second screen, a section's games, shows.
    pub fn shows_section(&self) -> bool {
        self.imp().open.get().is_some()
    }

    /// Shows the first screen, focused on the tile that last had focus there: the
    /// section whose games showed, when coming back from them.
    pub fn show_sections(&self) {
        let imp = self.imp();
        if imp.open.take().is_none() {
            return;
        }
        for (card, _) in imp.cards.take() {
            imp.carousel.remove(&card);
        }
        imp.shown.replace(Vec::new());
        imp.library_tile.set_visible(true);
        self.show_screen_class();
        self.show_tiles();
        self.focus_remembered();
    }

    /// Shows the second screen with the games of the section named `name`
    /// (`SectionId::name`), focused on the one that last had focus there; `false` when
    /// Home has no such section.
    pub fn show_section(&self, name: &str) -> bool {
        let imp = self.imp();
        let Some(id) = SectionId::from_name(name) else {
            return false;
        };
        let games = imp
            .sections
            .borrow()
            .iter()
            .find(|(section, _)| section.id == id)
            .map(|(_, games)| games.clone());
        let Some(games) = games.filter(|games| !games.is_empty()) else {
            return false;
        };
        imp.section_focus.borrow_mut().remember((), Some(id));
        for (tile, _) in imp.tiles.take() {
            imp.carousel.remove(&tile);
        }
        imp.tiles_shown.replace(Vec::new());
        for (card, _) in imp.cards.take() {
            imp.carousel.remove(&card);
        }
        imp.shown.replace(Vec::new());
        imp.open.set(Some(id));
        imp.library_tile.set_visible(false);
        self.show_screen_class();
        self.show_cards(&games);
        self.focus_remembered();
        true
    }

    /// Shows a tile for each section, before the Library tile, rebuilding them only when
    /// what they show changed.
    fn show_tiles(&self) {
        let imp = self.imp();
        let sections = imp.sections.borrow().clone();
        let keys: Vec<TileKey> = sections
            .iter()
            .map(|(section, _)| (section.id, section_title(section)))
            .collect();
        if *imp.tiles_shown.borrow() != keys {
            // Focus that went to the Library tile only because no sections were shown yet
            // goes to the first section once they are.
            let first_tiles = imp.tiles.borrow().is_empty() && imp.library_tile.is_focus();
            for (tile, _) in imp.tiles.take() {
                imp.carousel.remove(&tile);
            }
            let layout = imp.layout.get();
            let size = home::card_size(layout.home_tile, layout.home_tile_size);
            let mut tiles: Vec<(gtk::Button, SectionId)> = Vec::with_capacity(sections.len());
            for ((section, _), (_, title)) in sections.iter().zip(&keys) {
                let tile = section_tile(section, title, size);
                let id = section.id;
                tile.connect_has_focus_notify(glib::clone!(
                    #[weak(rename_to = page)]
                    self,
                    move |tile| {
                        if tile.has_focus() {
                            page.card_focused(tile, Focused::Section(id));
                        }
                    }
                ));
                imp.carousel
                    .insert_child_after(&tile, tiles.last().map(|(tile, _)| tile));
                tiles.push((tile, id));
            }
            imp.tiles.replace(tiles);
            imp.tiles_shown.replace(keys);
            if first_tiles && let Some((section, _)) = sections.first() {
                imp.section_focus
                    .borrow_mut()
                    .remember((), Some(section.id));
                self.focus_remembered();
                return;
            }
        }
        if !self.carousel_has_focus()
            && let Some((_, focused)) = self.remembered()
        {
            // The title and details follow the tile that focus comes back to.
            self.show_focused(&focused);
        }
    }

    /// Shows the cards of `games`, the open section's, rebuilding them only when what
    /// they show changed.
    fn show_cards(&self, games: &[GameObject]) {
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
        let had_focus = self.carousel_has_focus();
        imp.shown.replace(keys);
        for (card, _) in imp.cards.take() {
            imp.carousel.remove(&card);
        }
        let layout = imp.layout.get();
        let shape = layout.home_tile;
        let size = home::card_size(shape, layout.home_tile_size);
        let mut cards = Vec::with_capacity(games.len());
        for game in games {
            let (card, picture) = jump_card_frame(game, size);
            let art = home::card_art(shape, &game.entry().art);
            show_card_art(&picture, art, size, card.scale_factor());
            card.connect_has_focus_notify(glib::clone!(
                #[weak(rename_to = page)]
                self,
                #[weak]
                game,
                move |card| {
                    if card.has_focus() {
                        page.card_focused(card, Focused::Game(game));
                    }
                }
            ));
            imp.carousel
                .insert_child_after(&card, cards.last().map(|(card, _)| card));
            cards.push((card, game.clone()));
        }
        imp.cards.replace(cards);
        if had_focus {
            // The focused card was rebuilt: focus its game's new card.
            self.focus_remembered();
        } else if let Some((_, focused)) = self.remembered() {
            // The hero follows the card that focus comes back to.
            self.show_focused(&focused);
        }
    }

    /// Focuses the tile or card that last had focus on the screen shown, wherever it
    /// moved to, or the first one when it is gone.
    pub fn focus_remembered(&self) {
        if let Some((card, focused)) = self.remembered() {
            card.grab_focus();
            self.show_focused(&focused);
        }
    }

    /// The tile or card that last had focus on the screen shown, with what it shows.
    fn remembered(&self) -> Option<(gtk::Button, Focused)> {
        let imp = self.imp();
        match imp.open.get() {
            None => {
                let tiles = imp.tiles.borrow();
                let items: Vec<Option<SectionId>> = tiles
                    .iter()
                    .map(|(_, id)| Some(*id))
                    .chain([None])
                    .collect();
                let position = imp.section_focus.borrow().position(&(), &items);
                Some(match tiles.get(position) {
                    Some((tile, id)) => (tile.clone(), Focused::Section(*id)),
                    None => (imp.library_tile.get(), Focused::Library),
                })
            }
            Some(open) => {
                let cards = imp.cards.borrow();
                let items: Vec<GameId> = cards.iter().map(|(_, game)| game.id()).collect();
                let position = imp.game_focus.borrow().position(&open, &items);
                cards
                    .get(position)
                    .map(|(card, game)| (card.clone(), Focused::Game(game.clone())))
            }
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

    /// Notes the focused tile or card, scrolls it into view and shows what it stands for.
    fn card_focused(&self, card: &gtk::Button, focused: Focused) {
        let imp = self.imp();
        match (&focused, imp.open.get()) {
            (Focused::Game(game), Some(open)) => {
                imp.game_focus.borrow_mut().remember(open, game.id());
            }
            (Focused::Section(id), None) => imp.section_focus.borrow_mut().remember((), Some(*id)),
            (Focused::Library, None) => imp.section_focus.borrow_mut().remember((), None),
            _ => {}
        }
        if let Some(viewport) = imp.carousel.parent()
            && let Some(adjustment) = viewport
                .downcast_ref::<gtk::Viewport>()
                .and_then(|viewport| viewport.hadjustment())
            && let Some(bounds) = card.compute_bounds(&viewport)
        {
            let start = f64::from(bounds.x()) + adjustment.value();
            let end = start + f64::from(bounds.width());
            let margin = SCROLL_MARGIN + self.carousel_padding();
            adjustment.clamp_page(start - margin, end + margin);
        }
        self.show_focused(&focused);
    }

    /// The carousel's padding on each side, which only a design gives it: its width less
    /// its cards' and the gaps between them, halved.
    fn carousel_padding(&self) -> f64 {
        let carousel = &self.imp().carousel;
        let (mut width, ..) = carousel.measure(gtk::Orientation::Horizontal, -1);
        let mut cards = 0;
        let mut child = carousel.first_child();
        while let Some(card) = child {
            if card.should_layout() {
                cards += 1;
                width -= card.measure(gtk::Orientation::Horizontal, -1).0;
            }
            child = card.next_sibling();
        }
        let gaps = carousel.spacing() * (cards - 1).max(0);
        f64::from((width - gaps).max(0)) / 2.0
    }

    /// Shows what has focus: a game's hero, logo or title and details; a section's name
    /// and details without art; the title "Library" without art for the Library tile.
    fn show_focused(&self, focused: &Focused) {
        match focused {
            Focused::Game(game) => self.show_hero(Some(game)),
            Focused::Library => self.show_hero(None),
            Focused::Section(id) => {
                let section = self
                    .imp()
                    .sections
                    .borrow()
                    .iter()
                    .find(|(section, _)| section.id == *id)
                    .map(|(section, _)| section.clone());
                match section {
                    Some(section) => self.show_section_info(&section),
                    None => self.show_hero(None),
                }
            }
        }
    }

    /// Shows a section's name with its details, and no art.
    fn show_section_info(&self, section: &Section) {
        let imp = self.imp();
        imp.hero_generation.set(imp.hero_generation.get() + 1);
        self.hide_hero_art();
        imp.logo.set_paintable(gtk::gdk::Paintable::NONE);
        imp.logo.set_visible(false);
        imp.title.set_visible(true);
        imp.title.set_label(&section_title(section));
        self.fill_details(&home::section_details(section));
    }

    /// Fills the `.home-details` row with the details of what has focus, while the design
    /// asks for them; the row hides without any.
    fn show_details(&self, focused: &Focused) {
        let details = match focused {
            Focused::Game(game) => {
                let now = glib::DateTime::now_local().ok();
                let (now, offset) =
                    now.map_or((0, 0), |now| (now.to_unix(), now.utc_offset().as_seconds()));
                let entry = game.entry();
                home::details(&entry.game, entry.playtime_secs, now, offset)
            }
            Focused::Section(id) => self
                .imp()
                .sections
                .borrow()
                .iter()
                .find(|(section, _)| section.id == *id)
                .map(|(section, _)| home::section_details(section))
                .unwrap_or_default(),
            Focused::Library => Vec::new(),
        };
        self.fill_details(&details);
    }

    /// Fills the `.home-details` row with `details` as `.detail` items, while the design
    /// asks for them; the row hides without any.
    fn fill_details(&self, details: &[Detail]) {
        let imp = self.imp();
        while let Some(item) = imp.details.first_child() {
            imp.details.remove(&item);
        }
        let shown = if imp.layout.get().home_details {
            details
        } else {
            &[]
        };
        for detail in shown {
            imp.details.append(&detail_item(*detail));
        }
        imp.details.set_visible(!shown.is_empty());
    }

    /// The two hero pictures: the one with the latest art shown, then the other.
    fn heroes(&self) -> (gtk::Picture, gtk::Picture) {
        let imp = self.imp();
        let (a, b) = (imp.hero_a.get(), imp.hero_b.get());
        if imp.hero_b_latest.get() {
            (b, a)
        } else {
            (a, b)
        }
    }

    /// Shows `texture` as the hero: the picture that does not hold the latest art takes it
    /// and `.shown`, and the other one loses `.shown`, so a design's `transition: opacity`
    /// crossfades them. Pindu's own style hides a hero without `.shown` at once.
    fn show_hero_art(&self, texture: &gtk::gdk::Texture) {
        let imp = self.imp();
        let (outgoing, incoming) = self.heroes();
        incoming.set_paintable(Some(texture));
        incoming.add_css_class("shown");
        outgoing.remove_css_class("shown");
        imp.hero_b_latest.set(!imp.hero_b_latest.get());
        imp.home.remove_css_class("no-art");
    }

    /// Hides the hero art, keeping it in its picture so a design can fade it out, and marks
    /// the page `.no-art`.
    fn hide_hero_art(&self) {
        let imp = self.imp();
        imp.hero_a.remove_css_class("shown");
        imp.hero_b.remove_css_class("shown");
        imp.home.add_css_class("no-art");
    }

    /// Shows the game's hero art (or its cover) with its logo (or its title), and its
    /// details when the design asks for them; the Library tile shows the title "Library"
    /// without art. The page carries `.no-art` while no hero art shows. Art that still has
    /// to load replaces the hero shown once it has, so a design fades straight from one to
    /// the other.
    fn show_hero(&self, game: Option<&GameObject>) {
        let imp = self.imp();
        let generation = imp.hero_generation.get() + 1;
        imp.hero_generation.set(generation);
        imp.logo.set_paintable(gtk::gdk::Paintable::NONE);
        imp.logo.set_visible(false);
        imp.title.set_visible(true);
        let Some(game) = game else {
            self.fill_details(&[]);
            self.hide_hero_art();
            imp.title.set_label(&gettext("Library"));
            return;
        };
        self.show_details(&Focused::Game(game.clone()));
        let (title, art, logo) = {
            let entry = game.entry();
            (
                entry.game.title.clone(),
                entry.art.hero.clone().or_else(|| entry.art.cover.clone()),
                entry.art.logo.clone(),
            )
        };
        imp.title.set_label(&title);
        imp.logo
            .update_property(&[gtk::accessible::Property::Label(&title)]);
        // Art in memory shows at once, so moving along the carousel does not flicker.
        let art = match art.as_deref().and_then(textures::cached) {
            Some(texture) => {
                self.show_hero_art(&texture);
                None
            }
            None if art.is_none() => {
                self.hide_hero_art();
                None
            }
            None => art,
        };
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
            if let Some(path) = art {
                let texture = textures::load(&path).await;
                if let Some(page) = page.upgrade()
                    && page.imp().hero_generation.get() == generation
                {
                    match texture {
                        Ok(texture) => page.show_hero_art(&texture),
                        Err(_) => page.hide_hero_art(),
                    }
                }
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

/// Shows a card's art at `size`, decoded for a design that scales the card up.
fn show_card_art(picture: &CoverArt, art: CardArt, size: (i32, i32), scale_factor: i32) {
    picture.set_crop(art.crop);
    picture.show(art.art, None, home::art_pixels(size, scale_factor), |_| {});
}
