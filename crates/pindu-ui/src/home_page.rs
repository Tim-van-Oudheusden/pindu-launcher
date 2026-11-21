//! Fullscreen mode's Home page, where the mode opens. A design chooses one of three
//! layouts (`pindu_core::themes::HomeLayout`). Two of them have two screens. The first
//! lists Home's sections (`pindu_core::sections`), a tile each, then the Library tile and,
//! where Steam is present, the Steam Big Picture tile (`win.steam-big-picture`), over
//! Home's background, with the focused section's name and game count; a design can start
//! the row with the Recently Played games as cards (`home-start`), label the tiles
//! (`home-section-tile`) and preview the focused section's covers (`home-section-preview`).
//! Activating a section (`win.home-section`) opens the second screen: its games, with the
//! focused game's hero behind them. B goes back to the first screen, on the section it
//! came from. The third layout, Shelves, is one screen: every section as a row of its
//! games (`pindu_core::home::shelves`), then the Library row; Up and Down change row
//! (`HomePage::navigate`). Each screen and row comes back to the tile or card that last
//! had focus. The page carries `.home-sections` or `.home-games` for designs, which can
//! choose the tiles' shape and size (`pindu_core::home`) and crossfade the hero (`.shown`,
//! `.no-art`). Behind the first screen plays Home's background (`.home-background`), as
//! `home::background_playback` says for where the window is. Quick Access's Blur and Dim
//! name their strength on the page (`blur-*`, `dim-*`, `home::background_classes`), which
//! blurs the background and the hero and darkens them under `.home-dim`.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::glib;
use pindu_core::focus::FocusMemory;
use pindu_core::gamepad::Direction;
use pindu_core::home::{
    self, Background, BackgroundContext, CardArt, Detail, Playback, RowItem, ShelfStep, Strength,
};
use pindu_core::library::{ArtSet, HistoryGroup};
use pindu_core::model::GameId;
use pindu_core::sections::{self, Section, SectionId};
use pindu_core::themes::{HomeLayout, HomeStart, SectionTile, ThemeLayout, TileShape};

use crate::config::{PKGDATADIR, RESOURCE_BASE_PATH};
use crate::game_page::{date, playtime};
use crate::models::GameObject;
use crate::sources::{source_icon, source_name};
use crate::widgets::{CoverArt, SpacedLabel, TileFrame, textures};
use crate::window::{jump_card_frame, show_jump_art};

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

/// How many games a section holds, as its details row and its labelled tile say it.
fn game_count(count: u32) -> String {
    ngettext("{count} game", "{count} games", count).replace("{count}", &count.to_string())
}

/// How many games `section` holds.
fn games_in(section: &Section) -> u32 {
    u32::try_from(section.games.len()).unwrap_or(u32::MAX)
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

/// The name of a section's row in the Shelves layout: Recently Played is "Jump Back In"
/// there, as on the desktop.
fn shelf_title(section: &Section) -> String {
    match section.id {
        SectionId::RecentlyPlayed => gettext("Jump Back In"),
        _ => section_title(section),
    }
}

/// The side of the icon at the top of a labelled tile, in pixels.
const GLYPH_SIZE: i32 = 32;

/// The side of the icon over the name of a tile that is not labelled, in pixels.
const ICON_SIZE: i32 = 48;

/// The size of a cover in the preview of the focused section, in pixels.
const PREVIEW_COVER: (i32, i32) = (80, 120);

/// Adds `class` to `widget` or takes it off, as `on` says.
fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

/// The glyph at the top of a section's labelled tile (`.section-glyph`): a launcher's
/// monogram, or else the icon the desktop sidebar shows for the section.
fn section_glyph(id: SectionId) -> gtk::Widget {
    let icon = match id {
        SectionId::RecentlyPlayed => "document-open-recent-symbolic",
        SectionId::Favourites => "starred-symbolic",
        SectionId::Installed => "drive-harddisk-symbolic",
        SectionId::Collection(_) => "folder-symbolic",
        SectionId::Source(source) => match sections::monogram(source) {
            Some(monogram) => {
                return gtk::Label::builder()
                    .label(monogram)
                    .halign(gtk::Align::Start)
                    .css_classes(["section-glyph"])
                    .build()
                    .upcast();
            }
            None => source_icon(source),
        },
    };
    gtk::Image::builder()
        .icon_name(icon)
        .pixel_size(GLYPH_SIZE)
        .halign(gtk::Align::Start)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .css_classes(["section-glyph"])
        .build()
        .upcast()
}

/// The name at the bottom left of a labelled tile (`.section-name`): at most two lines,
/// then ellipsized, and one line where the tile has no room for two.
fn section_name(name: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(name)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .vexpand(true)
        .valign(gtk::Align::End)
        .css_classes(["section-name"])
        .build()
}

/// The number of games under a labelled tile's name (`.section-count`).
fn section_count(count: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(count)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["section-count"])
        .build()
}

/// A section's tile (`.home-section-tile`), which opens it through `win.home-section`,
/// `width`×`height` pixels. As art (`SectionTile::Art`): its art (`data/home/<name>.svg`)
/// filling the tile, or for a collection, which has none, an icon over its name
/// (`.home-collection-tile`); the art is an overlay, which does not size the tile.
/// Labelled (`SectionTile::Label`, `.labelled` and the section's own class): a glyph at
/// the top left, and at the bottom left its name over its number of games.
fn section_tile(
    section: &Section,
    title: &str,
    size: (i32, i32),
    kind: SectionTile,
) -> gtk::Button {
    let tile = gtk::Button::builder()
        .overflow(gtk::Overflow::Hidden)
        .tooltip_text(title)
        .action_name("win.home-section")
        .action_target(&section.id.name().to_variant())
        .css_classes(["card", "home-section-tile"])
        .build();
    let collection = matches!(section.id, SectionId::Collection(_));
    if collection {
        tile.add_css_class("home-collection-tile");
    }
    tile.update_property(&[gtk::accessible::Property::Label(title)]);
    if kind == SectionTile::Label {
        tile.add_css_class("labelled");
        tile.add_css_class(&section.id.css_class());
        let count = game_count(games_in(section));
        let label = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["section-label"])
            .build();
        label.append(&section_glyph(section.id));
        label.append(&section_name(title));
        label.append(&section_count(&count));
        tile.set_child(Some(&TileFrame::new(&label, size)));
        tile.update_property(&[gtk::accessible::Property::Description(&count)]);
        return tile;
    }
    let (width, height) = size;
    let frame = gtk::Overlay::builder()
        .child(
            &gtk::Box::builder()
                .width_request(width)
                .height_request(height)
                .build(),
        )
        .build();
    if collection {
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
                .pixel_size(ICON_SIZE)
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
    tile.set_child(Some(&frame));
    tile
}

/// The tile that ends a row of the Shelves layout whose section holds more games than
/// the row shows (`.shelf-more`): it opens the library at the section's view
/// (`win.home-section-library`).
fn see_all_tile(section: SectionId, title: &str, size: (i32, i32)) -> gtk::Button {
    let label = gtk::Label::builder()
        .label(gettext("See All"))
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .justify(gtk::Justification::Center)
        .css_classes(["heading"])
        .build();
    let tile = gtk::Button::builder()
        .child(&TileFrame::new(&label, size))
        .overflow(gtk::Overflow::Hidden)
        .action_name("win.home-section-library")
        .action_target(&section.name().to_variant())
        .css_classes(["card", "shelf-more"])
        .build();
    // What it shows all of: the button's own name is its label.
    tile.update_property(&[gtk::accessible::Property::Description(title)]);
    tile
}

/// Room kept beside the focused card when the carousel scrolls, as wide as its margins;
/// padding a design gives the carousel, for a focused card it scales up, adds to it.
const SCROLL_MARGIN: f64 = 48.0;

/// What a card shows of its game, to rebuild the cards only when that changes.
type CardKey = (GameId, String, Option<u8>, ArtSet);

fn card_key(game: &GameObject) -> CardKey {
    let entry = game.entry();
    (
        entry.game.id.clone(),
        entry.game.title.clone(),
        entry.game.pin_order,
        entry.art.clone(),
    )
}

/// What an item of the first screen's row shows, to rebuild the row only when that
/// changes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKey {
    Card(CardKey),
    Divider,
    /// A section's tile: its name and, where the tile says it, its number of games.
    Tile(SectionId, String, Option<u32>),
}

/// What a row of the Shelves layout shows, to rebuild it only when that changes: its
/// section (`None` for the Library row), its title, its number of games and its cards.
type ShelfKey = (Option<SectionId>, String, usize, Vec<CardKey>);

/// A tile or card of Home, as focus memory keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Game(GameId),
    Section(SectionId),
    /// The See All tile of a row of the Shelves layout.
    SeeAll,
    Library,
    BigPicture,
}

/// What has focus on Home, which the title, details and hero show.
#[derive(Debug, Clone)]
enum Focused {
    Section(SectionId),
    Game(GameObject),
    /// The See All tile of a section's row in the Shelves layout.
    SeeAll(SectionId),
    Library,
    BigPicture,
}

impl Focused {
    fn item(&self) -> Item {
        match self {
            Self::Section(id) => Item::Section(*id),
            Self::Game(game) => Item::Game(game.id()),
            Self::SeeAll(_) => Item::SeeAll,
            Self::Library => Item::Library,
            Self::BigPicture => Item::BigPicture,
        }
    }
}

/// Home's sections, in order, and the games of each.
#[derive(Debug, Default)]
struct Sections {
    sections: Vec<Section>,
    games: Vec<Vec<GameObject>>,
}

impl Sections {
    /// The section `id` with its games, if Home has it.
    fn get(&self, id: SectionId) -> Option<(&Section, &[GameObject])> {
        let position = self.sections.iter().position(|section| section.id == id)?;
        let games = self.games.get(position)?;
        Some((&self.sections[position], games))
    }
}

/// A row of the Shelves layout (`.home-shelf`).
#[derive(Debug)]
struct Shelf {
    /// The row's section; `None` for the Library row.
    id: Option<SectionId>,
    key: ShelfKey,
    row: gtk::Box,
    /// The row's cards and tiles, in order, each with what it stands for.
    items: Vec<(gtk::Button, Focused)>,
    /// The shape and size of the row's cards.
    shape: TileShape,
    size: (i32, i32),
    /// The cards that have not loaded their art yet: they do once the row is near the
    /// part of the list that shows.
    pending: RefCell<Vec<(CoverArt, GameObject)>>,
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
        /// The background in force: the one chosen in Quick Access, where the design
        /// offers the choice.
        pub background_choice: Cell<Background>,
        /// Where the window is, as the window last said (`set_background_context`).
        pub background_context: Cell<BackgroundContext>,
        #[template_child]
        pub hero_a: TemplateChild<gtk::Picture>,
        #[template_child]
        pub hero_b: TemplateChild<gtk::Picture>,
        /// Whether `hero_b`, not `hero_a`, holds the latest art shown.
        pub hero_b_latest: Cell<bool>,
        /// Whether hero art shows, over Home's background: the page is not `.no-art`.
        pub art_shown: Cell<bool>,
        #[template_child]
        pub eyebrow: TemplateChild<gtk::Label>,
        #[template_child]
        pub title_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub logo_width: TemplateChild<adw::Clamp>,
        #[template_child]
        pub logo_height: TemplateChild<adw::Clamp>,
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
        pub preview: TemplateChild<gtk::Box>,
        #[template_child]
        pub carousel_row: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub carousel: TemplateChild<gtk::Box>,
        #[template_child]
        pub shelves_scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub shelves: TemplateChild<gtk::Box>,
        #[template_child]
        pub library_tile: TemplateChild<gtk::Button>,
        #[template_child]
        pub library_tile_frame: TemplateChild<TileFrame>,
        #[template_child]
        pub library_tile_content: TemplateChild<gtk::Box>,
        #[template_child]
        pub library_tile_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub library_tile_label: TemplateChild<gtk::Label>,
        /// The number of games under the Library tile's name, while the tiles are labelled.
        pub library_tile_count: RefCell<Option<gtk::Label>>,
        /// How many games the library holds.
        pub library_count: Cell<u32>,
        #[template_child]
        pub big_picture_tile: TemplateChild<gtk::Button>,
        #[template_child]
        pub big_picture_tile_frame: TemplateChild<TileFrame>,
        #[template_child]
        pub big_picture_tile_content: TemplateChild<gtk::Box>,
        #[template_child]
        pub big_picture_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub big_picture_spinner: TemplateChild<adw::Spinner>,
        #[template_child]
        pub big_picture_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub big_picture_status: TemplateChild<gtk::Label>,
        /// Whether Home offers Steam Big Picture: Steam is present.
        pub big_picture_offered: Cell<bool>,
        /// Home's sections, in order, each with its games.
        pub(super) sections: RefCell<Sections>,
        /// The section whose games the second screen shows; `None` on the first screen
        /// and in the Shelves layout, which has one screen.
        pub open: Cell<Option<SectionId>>,
        /// What the first screen's row shows before the Library tile, in order.
        pub(super) row_shown: RefCell<Vec<RowKey>>,
        /// The cards and tiles of the first screen's row, each with what it stands for.
        pub(super) row: RefCell<Vec<(gtk::Button, Focused)>>,
        /// The line between the row's game cards and its section tiles, if it has both.
        pub divider: RefCell<Option<gtk::Separator>>,
        /// What the second screen's game cards show, in carousel order.
        pub shown: RefCell<Vec<CardKey>>,
        /// The second screen's game cards in the carousel, each with its game.
        pub cards: RefCell<Vec<(gtk::Button, GameObject)>>,
        /// The rows of the Shelves layout, in order.
        pub(super) shelf_rows: RefCell<Vec<Shelf>>,
        /// The card or tile that last had focus on the first screen.
        pub(super) first_focus: RefCell<FocusMemory<(), Item>>,
        /// The card or tile that last had focus among each section's games: on the second
        /// screen and in its row of the Shelves layout, whose Library row is `None`.
        pub(super) game_focus: RefCell<FocusMemory<Option<SectionId>, Item>>,
        /// The row that last had focus in the Shelves layout.
        pub shelf_focus: RefCell<FocusMemory<(), Option<SectionId>>>,
        /// What the Shelves layout does once its rows have their place: `Some` while that
        /// is queued, `true` if the row with focus goes back to the top then.
        pub shelves_settle: Cell<Option<bool>>,
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
            TileFrame::ensure_type();
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
                        page.card_focused(tile, &Focused::Library);
                    }
                }
            ));
            self.big_picture_tile.connect_has_focus_notify(glib::clone!(
                #[weak]
                page,
                move |tile| {
                    if tile.has_focus() {
                        page.card_focused(tile, &Focused::BigPicture);
                    }
                }
            ));
            // The page scrolls the carousel itself, keeping the lifted card's ring in view,
            // and the shelves to the row that has focus.
            for scrolled in [&*self.carousel, &*self.shelves] {
                if let Some(viewport) = scrolled.parent().and_downcast::<gtk::Viewport>() {
                    viewport.set_scroll_to_focus(false);
                }
            }
            let shelves = self.shelves_scroller.vadjustment();
            // The rows got their size, or the list a new one: the row with focus goes
            // back to the top, and the rows that came near load their art.
            shelves.connect_changed(glib::clone!(
                #[weak]
                page,
                move |_| page.settle_shelves(true)
            ));
            // The list is shown again with rows built while it was not, which have no
            // place until now. Its size may be what it was, and then it says nothing.
            self.shelves_scroller.connect_map(glib::clone!(
                #[weak]
                page,
                move |_| page.settle_shelves(true)
            ));
            // The list scrolled, by the page or under the pointer.
            shelves.connect_value_changed(glib::clone!(
                #[weak]
                page,
                move |_| page.settle_shelves(false)
            ));
            // The arrow keys move through the rows as the controller does, before GTK
            // moves focus to whatever lies nearest that way.
            for rows in [
                self.shelves_scroller.upcast_ref::<gtk::Widget>(),
                self.carousel_row.upcast_ref(),
            ] {
                let arrows = gtk::EventControllerKey::new();
                arrows.set_propagation_phase(gtk::PropagationPhase::Capture);
                arrows.connect_key_pressed(glib::clone!(
                    #[weak]
                    page,
                    #[upgrade_or]
                    glib::Propagation::Proceed,
                    move |_, key, _, state| {
                        let direction = match key {
                            gtk::gdk::Key::Up | gtk::gdk::Key::KP_Up => Direction::Up,
                            gtk::gdk::Key::Down | gtk::gdk::Key::KP_Down => Direction::Down,
                            gtk::gdk::Key::Left | gtk::gdk::Key::KP_Left => Direction::Left,
                            gtk::gdk::Key::Right | gtk::gdk::Key::KP_Right => Direction::Right,
                            _ => return glib::Propagation::Proceed,
                        };
                        let modified = state.intersects(
                            gtk::gdk::ModifierType::CONTROL_MASK
                                | gtk::gdk::ModifierType::SHIFT_MASK
                                | gtk::gdk::ModifierType::ALT_MASK,
                        );
                        if !modified && page.navigate(direction) {
                            glib::Propagation::Stop
                        } else {
                            glib::Propagation::Proceed
                        }
                    }
                ));
                rows.add_controller(arrows);
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
    /// Follows the layout of the design in force: Home's screens and where its row sits,
    /// what the row starts with, the shape, size and labels of its tiles, and whether the
    /// focused item's details and the focused section's covers show. Focus stays on Home,
    /// on what it remembers of the screen shown.
    pub fn set_layout(&self, layout: ThemeLayout) {
        let imp = self.imp();
        let old = imp.layout.replace(layout);
        // The top bar and the choice of background are the window's.
        let home_of = |layout: ThemeLayout| ThemeLayout {
            top_bar: false,
            home_background_choice: false,
            ..layout
        };
        if home_of(old) == home_of(layout) {
            return;
        }
        let had_focus = self.has_focus_within();
        self.clear_row();
        self.clear_cards();
        self.clear_shelves();
        if layout.home == HomeLayout::Shelves {
            // One screen: no section's games are open.
            imp.open.set(None);
            imp.library_tile.set_visible(true);
            imp.big_picture_tile
                .set_visible(imp.big_picture_offered.get());
        }
        self.show_home_layout(layout.home);
        self.show_tile_shape(layout.home_tile, layout.home_tile_size);
        self.show_tile_labels(layout.home_section_tile == SectionTile::Label);
        self.show_screen_class();
        self.refresh();
        if had_focus {
            self.focus_remembered();
        } else if let Some((_, focused)) = self.remembered() {
            self.show_focused(&focused);
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
    /// them, or shows the shelves under them in its place, and names the layout on the
    /// page (`home-layout-hero-bottom`, `home-layout-row-top`, `home-layout-shelves`) for
    /// designs. Moving the rows keeps focus where it is.
    fn show_home_layout(&self, layout: HomeLayout) {
        let imp = self.imp();
        for other in HomeLayout::ALL {
            imp.home
                .remove_css_class(&format!("home-layout-{}", other.name()));
        }
        imp.home
            .add_css_class(&format!("home-layout-{}", layout.name()));
        let content = &imp.content;
        let shelves = layout == HomeLayout::Shelves;
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
            HomeLayout::Shelves => {
                content.reorder_child_after(&*imp.shelves_scroller, Some(&*imp.info));
                content.set_valign(gtk::Align::Fill);
                content.set_margin_top(24);
                content.set_margin_bottom(0);
            }
        }
        imp.carousel_row.set_visible(!shelves);
        imp.shelves_scroller.set_visible(shelves);
        // A smaller logo in the Shelves layout, where the title's box is as tall as the
        // logo's with the title at its foot, over the details: the rows stay in place as
        // focus moves between games with and without a logo, and the first is on a 720
        // pixel screen. On that box, as the row's own height would take in a design's
        // margin, and the label's would wrap the title to fill it.
        let (width, height) = if shelves { (360, 96) } else { (480, 160) };
        imp.logo_width.set_maximum_size(width);
        imp.logo_width.set_tightening_threshold(width);
        imp.logo_height.set_maximum_size(height);
        imp.logo_height.set_tightening_threshold(height);
        imp.logo.set_size_request(width, height);
        imp.title_box
            .set_height_request(if shelves { height } else { -1 });
    }

    /// Sizes the Library and Steam Big Picture tiles for `shape` with its shorter side
    /// `size` pixels, if set, and names the shape on the carousel (`home-tile-landscape`,
    /// `home-tile-square`, `home-tile-portrait`) for designs.
    fn show_tile_shape(&self, shape: TileShape, size: Option<i32>) {
        let imp = self.imp();
        for other in TileShape::ALL {
            imp.carousel
                .remove_css_class(&format!("home-tile-{}", other.name()));
        }
        imp.carousel
            .add_css_class(&format!("home-tile-{}", shape.name()));
        let size = home::card_size(shape, size);
        imp.library_tile_frame.set_size(size);
        imp.big_picture_tile_frame.set_size(size);
    }

    /// Gives the Library and Steam Big Picture tiles the shape of the labelled section
    /// tiles (`.labelled`): the icon at the top left (`.section-glyph`), the name at the
    /// bottom left (`.section-name`) and, for the Library tile, the number of games in
    /// the library under it (`.section-count`). Or back to an icon over the name.
    fn show_tile_labels(&self, labelled: bool) {
        let imp = self.imp();
        let icons = [
            imp.library_tile_icon.upcast_ref::<gtk::Widget>(),
            imp.big_picture_icon.upcast_ref(),
            imp.big_picture_spinner.upcast_ref(),
        ];
        for icon in icons {
            icon.set_halign(if labelled {
                gtk::Align::Start
            } else {
                gtk::Align::Center
            });
            set_class(icon, "section-glyph", labelled);
        }
        let side = if labelled { GLYPH_SIZE } else { ICON_SIZE };
        imp.library_tile_icon.set_pixel_size(side);
        imp.big_picture_icon.set_pixel_size(side);
        imp.big_picture_spinner.set_size_request(side, side);
        for tile in [&*imp.library_tile, &*imp.big_picture_tile] {
            set_class(tile, "labelled", labelled);
        }
        for content in [&*imp.library_tile_content, &*imp.big_picture_tile_content] {
            content.set_spacing(if labelled { 0 } else { 12 });
            content.set_valign(if labelled {
                gtk::Align::Fill
            } else {
                gtk::Align::Center
            });
            set_class(content, "section-label", labelled);
        }
        for name in [&*imp.library_tile_label, &*imp.big_picture_label] {
            name.set_xalign(if labelled { 0.0 } else { 0.5 });
            name.set_vexpand(labelled);
            name.set_valign(if labelled {
                gtk::Align::End
            } else {
                gtk::Align::Fill
            });
            set_class(name, "section-name", labelled);
            set_class(name, "heading", !labelled);
        }
        let status = &imp.big_picture_status;
        status.set_xalign(if labelled { 0.0 } else { 0.5 });
        set_class(&**status, "section-count", labelled);
        set_class(&**status, "dimmed", !labelled);
        if let Some(count) = imp.library_tile_count.take() {
            imp.library_tile_content.remove(&count);
        }
        if labelled {
            let count = section_count("");
            imp.library_tile_content.append(&count);
            imp.library_tile_count.replace(Some(count));
        }
        self.show_library_count();
    }

    /// Says how many games the library holds on the Library tile, while the tiles are
    /// labelled: under its name, and as its description.
    fn show_library_count(&self) {
        let imp = self.imp();
        let count = imp.library_tile_count.borrow();
        let text = match &*count {
            Some(label) => {
                let text = game_count(imp.library_count.get());
                label.set_label(&text);
                text
            }
            None => String::new(),
        };
        imp.library_tile
            .update_property(&[gtk::accessible::Property::Description(&text)]);
    }

    /// Takes the number of games in the library, which the labelled Library tile shows.
    pub fn set_library_count(&self, count: u32) {
        if self.imp().library_count.replace(count) != count {
            self.show_library_count();
        }
    }

    /// Whether the layout is Shelves: one screen of rows.
    fn shelves(&self) -> bool {
        self.imp().layout.get().home == HomeLayout::Shelves
    }

    /// Whether the first screen shows: no section's games do, and the layout has screens.
    fn on_first_screen(&self) -> bool {
        self.imp().open.get().is_none() && !self.shelves()
    }

    /// Names the screen shown on the page for designs: `.home-sections` for the first,
    /// `.home-games` for a section's games and for the Shelves layout; and
    /// `.home-start-games` on both screens while the first one's row starts with games.
    fn show_screen_class(&self) {
        let imp = self.imp();
        let home = &imp.home;
        let layout = imp.layout.get();
        let games = !self.on_first_screen();
        home.remove_css_class(if games { "home-sections" } else { "home-games" });
        home.add_css_class(if games { "home-games" } else { "home-sections" });
        set_class(
            &**home,
            "home-start-games",
            layout.home_start == HomeStart::Games && !self.shelves(),
        );
        self.show_background_playback();
    }

    /// Switches Home to `choice`, the background in force, or remembers it until Home
    /// first loads.
    pub(crate) fn set_background(&self, choice: Background) {
        let imp = self.imp();
        if imp.background_choice.replace(choice) == choice {
            return;
        }
        let opened = imp
            .background_media
            .borrow()
            .as_ref()
            .is_some_and(|media| media.file().is_some());
        // A pending first map will open the latest choice. For an opened video, use a
        // fresh stream: reassigning the file can leave GTK stuck on the new first frame.
        if opened {
            self.load_background();
            self.show_background_playback();
        }
    }

    /// Follows where the window is for Home's background. `first_screen` says whether
    /// Home is the shown page; the page itself knows whether its first screen shows, and
    /// whether art covers the background there.
    pub fn set_background_context(&self, context: BackgroundContext) {
        let imp = self.imp();
        imp.background_context.set(context);
        if context.fullscreen && imp.background_media.borrow().is_none() {
            self.load_background();
        }
        self.show_background_playback();
    }

    /// Loads the chosen Home background, muted and looped. A file that is missing or
    /// does not play is logged and leaves the black of `.home-background`.
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
        if let Some(previous) = imp.background_media.replace(Some(media.clone())) {
            previous.pause();
        }
        imp.background.set_paintable(Some(&media));
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

    /// Opens the chosen background in the size that suits the window's screen, muted.
    fn open_background(&self, media: &gtk::MediaFile) {
        let choice = self.imp().background_choice.get();
        let video = choice.video(self.screen_pixels());
        media.set_filename(Some(Path::new(PKGDATADIR).join("home").join(video)));
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

    /// Plays, pauses, holds Home's background on its first frame or takes it off the page,
    /// as `home::background_playback` says for where the window is, the screen shown and
    /// the art over it. Taken off, `.home-background`'s plain black shows in its place.
    fn show_background_playback(&self) {
        let imp = self.imp();
        let Some(media) = imp.background_media.borrow().clone() else {
            return;
        };
        let context = imp.background_context.get();
        let context = BackgroundContext {
            first_screen: context.first_screen && imp.open.get().is_none(),
            covered: context.covered || imp.art_shown.get(),
            ..context
        };
        let playback = home::background_playback(context);
        // A video that failed to play stays off the page.
        let shown = playback != Playback::Hidden && media.error().is_none();
        if imp.background.paintable().is_some() != shown {
            imp.background.set_paintable(shown.then_some(&media));
        }
        match playback {
            Playback::Play => media.play(),
            Playback::Pause | Playback::Hidden => media.pause(),
            Playback::FirstFrame => {
                media.pause();
                if media.is_seekable() && media.timestamp() != 0 {
                    media.seek(0);
                }
            }
        }
    }

    /// Takes Home's sections, each with its games, and shows them on the screen shown. A
    /// section whose games show and that is gone now goes back to the first screen. Focus
    /// that went to the Library tile only because Home had no sections yet goes to its
    /// first card or tile once it has some.
    pub fn set_sections(&self, sections: Vec<(Section, Vec<GameObject>)>) {
        let imp = self.imp();
        let (sections, games): (Vec<Section>, Vec<Vec<GameObject>>) = sections.into_iter().unzip();
        let first = imp.sections.borrow().sections.is_empty()
            && !sections.is_empty()
            && imp.library_tile.is_focus();
        imp.sections.replace(Sections { sections, games });
        self.refresh();
        if first {
            self.focus_first();
        }
    }

    /// Focuses the first card or tile of the screen shown, the first row's in the Shelves
    /// layout, or the one that last had focus in that row.
    fn focus_first(&self) {
        let imp = self.imp();
        if self.shelves() {
            let first = imp.shelf_rows.borrow().first().map(|shelf| shelf.id);
            if let Some(first) = first {
                imp.shelf_focus.borrow_mut().remember((), first);
            }
        } else if imp.open.get().is_none() {
            let first = imp.row.borrow().first().map(|(_, focused)| focused.item());
            if let Some(first) = first {
                imp.first_focus.borrow_mut().remember((), first);
            }
        }
        self.focus_remembered();
    }

    /// Shows the shown screen's tiles, cards or rows again, after the sections or the
    /// layout changed.
    fn refresh(&self) {
        let imp = self.imp();
        if self.shelves() {
            self.show_shelves();
            return;
        }
        let Some(open) = imp.open.get() else {
            self.show_row();
            return;
        };
        let games = imp
            .sections
            .borrow()
            .get(open)
            .map(|(_, games)| games.to_vec());
        match games {
            Some(games) if !games.is_empty() => self.show_cards(&games),
            _ => self.show_sections(),
        }
    }

    /// Whether the second screen, a section's games, shows. Never in the Shelves layout,
    /// which has one screen.
    pub fn shows_section(&self) -> bool {
        self.imp().open.get().is_some()
    }

    /// Offers Steam Big Picture on the first screen, where Steam is present (`offered`).
    pub fn set_big_picture_offered(&self, offered: bool) {
        let imp = self.imp();
        imp.big_picture_offered.set(offered);
        imp.big_picture_tile
            .set_visible(offered && imp.open.get().is_none());
    }

    /// Shows on the Steam Big Picture tile whether Big Picture is `opening`, while the
    /// launcher waits for its window.
    pub fn set_big_picture_opening(&self, opening: bool) {
        let imp = self.imp();
        imp.big_picture_icon.set_visible(!opening);
        imp.big_picture_spinner.set_visible(opening);
        imp.big_picture_status.set_visible(opening);
        let description = if opening {
            gettext("Opening…")
        } else {
            String::new()
        };
        imp.big_picture_tile
            .update_property(&[gtk::accessible::Property::Description(&description)]);
    }

    /// Shows the first screen, focused on the tile that last had focus there: the
    /// section whose games showed, when coming back from them.
    pub fn show_sections(&self) {
        let imp = self.imp();
        if imp.open.take().is_none() {
            return;
        }
        self.clear_cards();
        imp.library_tile.set_visible(true);
        imp.big_picture_tile
            .set_visible(imp.big_picture_offered.get());
        self.show_screen_class();
        self.show_row();
        self.focus_remembered();
    }

    /// Shows the second screen with the games of the section named `name`
    /// (`SectionId::name`), focused on the one that last had focus there; `false` when
    /// Home has no such section. In the Shelves layout, which has no second screen, it
    /// focuses that section's row instead.
    pub fn show_section(&self, name: &str) -> bool {
        let imp = self.imp();
        let Some(id) = SectionId::from_name(name) else {
            return false;
        };
        if self.shelves() {
            return self.focus_shelf(id);
        }
        let games = imp
            .sections
            .borrow()
            .get(id)
            .map(|(_, games)| games.to_vec());
        let Some(games) = games.filter(|games| !games.is_empty()) else {
            return false;
        };
        imp.first_focus.borrow_mut().remember((), Item::Section(id));
        self.clear_row();
        self.clear_cards();
        imp.open.set(Some(id));
        imp.library_tile.set_visible(false);
        imp.big_picture_tile.set_visible(false);
        self.show_screen_class();
        self.show_cards(&games);
        self.focus_remembered();
        true
    }

    /// Takes the first screen's cards, divider and tiles out of the carousel.
    fn clear_row(&self) {
        let imp = self.imp();
        for (tile, _) in imp.row.take() {
            imp.carousel.remove(&tile);
        }
        if let Some(divider) = imp.divider.take() {
            imp.carousel.remove(&divider);
        }
        imp.row_shown.replace(Vec::new());
    }

    /// Takes the second screen's cards out of the carousel.
    fn clear_cards(&self) {
        let imp = self.imp();
        for (card, _) in imp.cards.take() {
            imp.carousel.remove(&card);
        }
        imp.shown.replace(Vec::new());
    }

    /// Takes the rows of the Shelves layout away, and puts the Library and Steam Big
    /// Picture tiles back at the end of the carousel.
    fn clear_shelves(&self) {
        let imp = self.imp();
        for shelf in imp.shelf_rows.take() {
            if shelf.id.is_none() {
                for tile in [&*imp.library_tile, &*imp.big_picture_tile] {
                    if let Some(carousel) = tile.parent().and_downcast::<gtk::Box>() {
                        carousel.remove(tile);
                    }
                    imp.carousel.append(tile);
                }
            }
            imp.shelves.remove(&shelf.row);
        }
    }

    /// A game's card for the carousel or a row of the Shelves layout, `size` pixels, and
    /// its art for the caller to show. Focus on it shows its game.
    fn game_card(&self, game: &GameObject, size: (i32, i32)) -> (gtk::Button, CoverArt) {
        let (card, picture) = jump_card_frame(game, size);
        card.connect_has_focus_notify(glib::clone!(
            #[weak(rename_to = page)]
            self,
            #[weak]
            game,
            move |card| {
                if card.has_focus() {
                    page.card_focused(card, &Focused::Game(game));
                }
            }
        ));
        (card, picture)
    }

    /// Shows the first screen's row before the Library tile, as the design starts it
    /// (`home::first_screen_row`): a tile for each section, or the Recently Played games
    /// as cards, a divider (`.home-divider`) and a tile for each other section. It is
    /// rebuilt only when what it shows changed.
    fn show_row(&self) {
        /// What an item of the row is built from.
        enum Shown<'a> {
            Card(&'a GameObject),
            Divider,
            Tile(&'a Section, String),
        }
        let imp = self.imp();
        let layout = imp.layout.get();
        let labelled = layout.home_section_tile == SectionTile::Label;
        let sections = imp.sections.borrow();
        let recent = sections
            .get(SectionId::RecentlyPlayed)
            .map_or(&[][..], |(_, games)| games);
        let items: Vec<(RowKey, Shown<'_>)> =
            home::first_screen_row(layout.home_start, &sections.sections)
                .into_iter()
                .filter_map(|item| match item {
                    RowItem::Game(id) => recent
                        .iter()
                        .find(|game| game.id() == id)
                        .map(|game| (RowKey::Card(card_key(game)), Shown::Card(game))),
                    RowItem::Divider => Some((RowKey::Divider, Shown::Divider)),
                    RowItem::Section(id) => sections.get(id).map(|(section, _)| {
                        let title = section_title(section);
                        let count = labelled.then(|| games_in(section));
                        (
                            RowKey::Tile(id, title.clone(), count),
                            Shown::Tile(section, title),
                        )
                    }),
                })
                .collect();
        let unchanged = imp
            .row_shown
            .borrow()
            .iter()
            .eq(items.iter().map(|(key, _)| key));
        if !unchanged {
            let had_focus = imp.row.borrow().iter().any(|(tile, _)| tile.has_focus());
            self.clear_row();
            let shape = layout.home_tile;
            let size = home::card_size(shape, layout.home_tile_size);
            let scale = self.scale_factor();
            let mut row: Vec<(gtk::Button, Focused)> = Vec::with_capacity(items.len());
            let mut keys = Vec::with_capacity(items.len());
            let mut last: Option<gtk::Widget> = None;
            for (key, shown) in items {
                keys.push(key);
                let widget: gtk::Widget = match shown {
                    Shown::Card(game) => {
                        let (card, picture) = self.game_card(game, size);
                        let art = home::card_art(shape, &game.entry().art);
                        show_card_art(&picture, art, size, scale);
                        row.push((card.clone(), Focused::Game(game.clone())));
                        card.upcast()
                    }
                    Shown::Divider => {
                        let divider = gtk::Separator::builder()
                            .orientation(gtk::Orientation::Vertical)
                            .can_focus(false)
                            .css_classes(["home-divider"])
                            .build();
                        imp.divider.replace(Some(divider.clone()));
                        divider.upcast()
                    }
                    Shown::Tile(section, title) => {
                        let tile = section_tile(section, &title, size, layout.home_section_tile);
                        let id = section.id;
                        tile.connect_has_focus_notify(glib::clone!(
                            #[weak(rename_to = page)]
                            self,
                            move |tile| {
                                if tile.has_focus() {
                                    page.card_focused(tile, &Focused::Section(id));
                                }
                            }
                        ));
                        row.push((tile.clone(), Focused::Section(id)));
                        tile.upcast()
                    }
                };
                imp.carousel.insert_child_after(&widget, last.as_ref());
                last = Some(widget);
            }
            drop(sections);
            imp.row.replace(row);
            imp.row_shown.replace(keys);
            if had_focus {
                // The focused item was rebuilt: focus what stands for it now.
                self.focus_remembered();
                return;
            }
        }
        if !self.has_focus_within()
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
        let keys: Vec<CardKey> = games.iter().map(card_key).collect();
        if *imp.shown.borrow() == keys {
            return;
        }
        let had_focus = self.has_focus_within();
        self.clear_cards();
        imp.shown.replace(keys);
        let layout = imp.layout.get();
        let shape = layout.home_tile;
        let size = home::card_size(shape, layout.home_tile_size);
        let scale = self.scale_factor();
        let mut cards: Vec<(gtk::Button, GameObject)> = Vec::with_capacity(games.len());
        for game in games {
            let (card, picture) = self.game_card(game, size);
            let art = home::card_art(shape, &game.entry().art);
            show_card_art(&picture, art, size, scale);
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

    /// Shows the rows of the Shelves layout (`home::shelves`): one per section, with its
    /// first games as cards and See All where it holds more, then the Library row. A row
    /// is rebuilt only when what it shows changed.
    fn show_shelves(&self) {
        /// What a section's row is built from: the row and the games on it.
        type Parts<'a> = (home::Shelf, Vec<&'a GameObject>);
        let imp = self.imp();
        let layout = imp.layout.get();
        let sections = imp.sections.borrow();
        let library = gettext("Library");
        // What each row shows: its key, and for a section's row what it is built from.
        let mut wanted: Vec<(ShelfKey, Option<Parts<'_>>)> = home::shelves(&sections.sections)
            .into_iter()
            .zip(&sections.sections)
            .zip(&sections.games)
            .map(|((shelf, section), games)| {
                let games: Vec<&GameObject> = shelf
                    .games
                    .iter()
                    .filter_map(|id| games.iter().find(|game| game.id() == *id))
                    .collect();
                let key = (
                    Some(shelf.id),
                    shelf_title(section),
                    shelf.total,
                    games.iter().map(|game| card_key(game)).collect(),
                );
                (key, Some((shelf, games)))
            })
            .collect();
        wanted.push(((None, library, 0, Vec::new()), None));
        let unchanged = imp
            .shelf_rows
            .borrow()
            .iter()
            .map(|shelf| &shelf.key)
            .eq(wanted.iter().map(|(key, _)| key));
        if unchanged {
            return;
        }
        let had_focus = self.has_focus_within();
        let mut old: Vec<Option<Shelf>> = imp.shelf_rows.take().into_iter().map(Some).collect();
        let mut rows: Vec<Shelf> = Vec::with_capacity(wanted.len());
        let mut previous: Option<gtk::Box> = None;
        for (key, parts) in wanted {
            // A row that shows the same stays as it is.
            let kept = old
                .iter_mut()
                .find_map(|shelf| shelf.take_if(|shelf| shelf.key == key));
            let shelf = match (kept, parts) {
                (Some(shelf), _) => {
                    imp.shelves
                        .reorder_child_after(&shelf.row, previous.as_ref());
                    shelf
                }
                (None, parts) => {
                    let shelf = match parts {
                        Some((shelf, games)) => self.section_shelf(key, &shelf, &games, layout),
                        None => self.library_shelf(key, layout),
                    };
                    imp.shelves
                        .insert_child_after(&shelf.row, previous.as_ref());
                    shelf
                }
            };
            previous = Some(shelf.row.clone());
            rows.push(shelf);
        }
        drop(sections);
        for gone in old.into_iter().flatten() {
            imp.shelves.remove(&gone.row);
        }
        imp.shelf_rows.replace(rows);
        // The new rows load their art once they have their place; the list itself says
        // nothing when they come to the height the old ones had.
        self.settle_shelves(true);
        if had_focus {
            // The focused card may have been rebuilt: focus what stands for it now.
            self.focus_remembered();
        } else if let Some((_, focused)) = self.remembered() {
            self.show_focused(&focused);
        }
    }

    /// The frame of a row of the Shelves layout (`.home-shelf`): its header, the `title`
    /// (`.shelf-title`) and the number of games if it shows one (`.shelf-count`), over a
    /// carousel of cards of `shape` that scrolls sideways. Returns the row and its
    /// carousel, which the caller fills.
    fn shelf_frame(title: &str, count: Option<usize>, shape: TileShape) -> (gtk::Box, gtk::Box) {
        // Not a bare label: a design may space the title's letters.
        let name = SpacedLabel::new(title, &["shelf-title", "heading"]);
        let header = gtk::Box::builder()
            .margin_start(48)
            .margin_end(48)
            .css_classes(["shelf-header"])
            .build();
        header.append(&name);
        if let Some(count) = count {
            header.append(
                &gtk::Label::builder()
                    .label(count.to_string())
                    .css_classes(["shelf-count"])
                    .build(),
            );
        }
        let carousel = gtk::Box::builder()
            .spacing(24)
            .margin_start(48)
            .margin_end(48)
            .margin_top(12)
            .margin_bottom(12)
            .css_classes(["home-carousel", &format!("home-tile-{}", shape.name())])
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&carousel)
            .build();
        // The page scrolls the row itself, keeping the lifted card's ring in view.
        if let Some(viewport) = carousel.parent().and_downcast::<gtk::Viewport>() {
            viewport.set_scroll_to_focus(false);
        }
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .accessible_role(gtk::AccessibleRole::Group)
            .css_classes(["home-shelf"])
            .build();
        row.append(&header);
        row.append(&scroller);
        row.update_relation(&[gtk::accessible::Relation::LabelledBy(&[name
            .label()
            .upcast_ref()])]);
        (row, carousel)
    }

    /// The row of a section in the Shelves layout: a card for each of `games`, the
    /// section's first ones, in the shape `home::shelf_shape` gives the row, then See All
    /// (`.shelf-more`) when the section holds more. The cards load their art once the
    /// row is near the part of the list that shows.
    fn section_shelf(
        &self,
        key: ShelfKey,
        shelf: &home::Shelf,
        games: &[&GameObject],
        layout: ThemeLayout,
    ) -> Shelf {
        let (shape, size) = home::shelf_shape(shelf.id, layout);
        // Jump Back In holds few games, all on the row: it says no number.
        let count = (shelf.id != SectionId::RecentlyPlayed).then_some(shelf.total);
        let (row, carousel) = Self::shelf_frame(&key.1, count, shape);
        let mut items: Vec<(gtk::Button, Focused)> = Vec::with_capacity(games.len() + 1);
        let mut pending = Vec::with_capacity(games.len());
        for &game in games {
            let (card, picture) = self.game_card(game, size);
            carousel.append(&card);
            items.push((card, Focused::Game(game.clone())));
            pending.push((picture, game.clone()));
        }
        if shelf.total > shelf.games.len() {
            let more = see_all_tile(shelf.id, &key.1, size);
            let id = shelf.id;
            more.connect_has_focus_notify(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |tile| {
                    if tile.has_focus() {
                        page.card_focused(tile, &Focused::SeeAll(id));
                    }
                }
            ));
            carousel.append(&more);
            items.push((more, Focused::SeeAll(id)));
        }
        Shelf {
            id: Some(shelf.id),
            key,
            row,
            items,
            shape,
            size,
            pending: RefCell::new(pending),
        }
    }

    /// The last row of the Shelves layout (`.shelf-library`): the Library tile and, where
    /// Steam is present, the Steam Big Picture tile, moved here from the carousel.
    fn library_shelf(&self, key: ShelfKey, layout: ThemeLayout) -> Shelf {
        let imp = self.imp();
        let shape = layout.home_tile;
        let (row, carousel) = Self::shelf_frame(&key.1, None, shape);
        row.add_css_class("shelf-library");
        for tile in [&*imp.library_tile, &*imp.big_picture_tile] {
            if let Some(parent) = tile.parent().and_downcast::<gtk::Box>() {
                parent.remove(tile);
            }
            carousel.append(tile);
        }
        Shelf {
            id: None,
            key,
            row,
            items: vec![
                (imp.library_tile.get(), Focused::Library),
                (imp.big_picture_tile.get(), Focused::BigPicture),
            ],
            shape,
            size: home::card_size(shape, layout.home_tile_size),
            pending: RefCell::default(),
        }
    }

    /// Queues what the Shelves layout does once its rows have their place: the rows near
    /// the part of the list that shows load their art, and with `scroll` the row that
    /// last had focus goes back to the top. It waits for the frame after the next
    /// layout: rows that were only just built have no place before it, and the list
    /// says it changed before placing them, or says nothing when rows are rebuilt to
    /// the height they had.
    fn settle_shelves(&self, scroll: bool) {
        if !self.shelves() {
            return;
        }
        let imp = self.imp();
        let queued = imp.shelves_settle.replace(Some(scroll));
        if let Some(scroll_queued) = queued {
            imp.shelves_settle.set(Some(scroll || scroll_queued));
            return;
        }
        let frames = Cell::new(0);
        imp.shelves.add_tick_callback(glib::clone!(
            #[weak(rename_to = page)]
            self,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move |_, _| {
                let imp = page.imp();
                frames.set(frames.get() + 1);
                // A frame's tick comes before its layout: the first one sees the rows as
                // they were.
                let placed = frames.get() > 1
                    && imp
                        .shelf_rows
                        .borrow()
                        .iter()
                        .all(|shelf| shelf.row.height() > 0);
                if !placed && frames.get() < PLACE_FRAMES {
                    return glib::ControlFlow::Continue;
                }
                if imp.shelves_settle.take() == Some(true) {
                    page.scroll_to_shelf();
                }
                page.load_shelf_art();
                glib::ControlFlow::Break
            }
        ));
    }

    /// Loads the art of the rows of the Shelves layout that show or are within a page of
    /// showing; the rows further off load theirs once the list scrolls near them.
    fn load_shelf_art(&self) {
        let imp = self.imp();
        let adjustment = imp.shelves_scroller.vadjustment();
        let page = adjustment.page_size();
        if page <= 0.0 {
            return;
        }
        let near = (adjustment.value() - page)..(adjustment.value() + 2.0 * page);
        let scale = self.scale_factor();
        for shelf in imp.shelf_rows.borrow().iter() {
            if shelf.pending.borrow().is_empty() {
                continue;
            }
            let Some(bounds) = shelf.row.compute_bounds(&*imp.shelves) else {
                continue;
            };
            let top = f64::from(bounds.y());
            let bottom = top + f64::from(bounds.height());
            if bottom > top && top < near.end && bottom > near.start {
                // Jump Back In's cards show what the desktop's Jump Back In row shows.
                let jump = shelf.id == Some(SectionId::RecentlyPlayed);
                for (picture, game) in shelf.pending.take() {
                    if jump {
                        show_jump_art(&picture, &game, home::art_pixels(shelf.size, scale));
                    } else {
                        let art = home::card_art(shelf.shape, &game.entry().art);
                        show_card_art(&picture, art, shelf.size, scale);
                    }
                }
            }
        }
    }

    /// Scrolls the Shelves layout so the row that last had focus has its header at the
    /// top of the list, or as near as the end of the list allows.
    fn scroll_to_shelf(&self) {
        let imp = self.imp();
        let rows = imp.shelf_rows.borrow();
        let ids: Vec<Option<SectionId>> = rows.iter().map(|shelf| shelf.id).collect();
        let position = imp.shelf_focus.borrow().position(&(), &ids);
        if let Some(shelf) = rows.get(position)
            && let Some(bounds) = shelf.row.compute_bounds(&*imp.shelves)
        {
            imp.shelves_scroller
                .vadjustment()
                .set_value(f64::from(bounds.y()));
        }
    }

    /// Moves focus along and between Home's rows as the controller or the arrow keys ask:
    /// in the Shelves layout, left and right along the row, stopping at its ends, and down
    /// and up to the next row and the one before, on the card that last had focus there
    /// (`home::shelf_step`); in the other layouts, along the one row of the screen shown,
    /// stopping at its ends (`home::row_step`). `false` when the move is not Home's to
    /// make: focus outside the rows, or up from the first row, which leaves for the top
    /// bar.
    pub fn navigate(&self, direction: Direction) -> bool {
        // A row runs from the right where the text does.
        let direction = match direction {
            Direction::Left if self.direction() == gtk::TextDirection::Rtl => Direction::Right,
            Direction::Right if self.direction() == gtk::TextDirection::Rtl => Direction::Left,
            direction => direction,
        };
        if self.shelves() {
            self.navigate_shelves(direction)
        } else {
            self.navigate_row(direction)
        }
    }

    /// Moves focus along the row of the screen shown, in the layouts with two screens.
    fn navigate_row(&self, direction: Direction) -> bool {
        let carousel = &self.imp().carousel;
        let shown: Vec<gtk::Widget> =
            std::iter::successors(carousel.first_child(), |child| child.next_sibling())
                .filter(|child| child.is_visible() && child.is_focusable())
                .collect();
        let Some(item) = shown.iter().position(|child| child.has_focus()) else {
            return false;
        };
        match home::row_step(item, direction, shown.len()) {
            ShelfStep::Item(item) => {
                shown[item].grab_focus();
            }
            ShelfStep::Row(_) | ShelfStep::Stay => {}
            ShelfStep::Leave => return false,
        }
        true
    }

    /// Moves focus along and between the rows of the Shelves layout.
    fn navigate_shelves(&self, direction: Direction) -> bool {
        let rows = self.imp().shelf_rows.borrow();
        let shown: Vec<Vec<&(gtk::Button, Focused)>> = rows
            .iter()
            .map(|shelf| {
                shelf
                    .items
                    .iter()
                    .filter(|(tile, _)| tile.get_visible())
                    .collect()
            })
            .collect();
        let at = shown.iter().enumerate().find_map(|(row, items)| {
            let item = items.iter().position(|(tile, _)| tile.has_focus())?;
            Some((row, item))
        });
        let Some((row, item)) = at else {
            return false;
        };
        let lengths: Vec<usize> = shown.iter().map(Vec::len).collect();
        let target = match home::shelf_step((row, item), direction, &lengths) {
            ShelfStep::Item(item) => shown[row].get(item).map(|(tile, _)| tile.clone()),
            ShelfStep::Row(row) => rows
                .get(row)
                .and_then(|shelf| self.remembered_in(shelf))
                .map(|(tile, _)| tile),
            ShelfStep::Stay => None,
            ShelfStep::Leave => return false,
        };
        drop(shown);
        drop(rows);
        if let Some(target) = target {
            target.grab_focus();
        }
        true
    }

    /// Focuses the row of section `id` in the Shelves layout, on the card that last had
    /// focus there; `false` when Home has no such row.
    fn focus_shelf(&self, id: SectionId) -> bool {
        let imp = self.imp();
        let target = imp
            .shelf_rows
            .borrow()
            .iter()
            .find(|shelf| shelf.id == Some(id))
            .and_then(|shelf| self.remembered_in(shelf));
        let Some((card, focused)) = target else {
            return false;
        };
        // Remembered outright: a window that is not active does not say its focus moved.
        imp.shelf_focus.borrow_mut().remember((), Some(id));
        card.grab_focus();
        self.scroll_to_shelf();
        self.show_focused(&focused);
        true
    }

    /// Focuses the tile or card that last had focus on the screen shown, wherever it
    /// moved to, or the first one when it is gone, and scrolls its row to it: a card that
    /// has focus already does not say so again.
    pub fn focus_remembered(&self) {
        if let Some((card, focused)) = self.remembered() {
            card.grab_focus();
            keep_in_view(&card, true);
            self.show_focused(&focused);
        }
    }

    /// The tile or card that last had focus on the screen shown, with what it shows: in
    /// the Shelves layout, the one of the row that last had focus.
    fn remembered(&self) -> Option<(gtk::Button, Focused)> {
        let imp = self.imp();
        if self.shelves() {
            let rows = imp.shelf_rows.borrow();
            let ids: Vec<Option<SectionId>> = rows.iter().map(|shelf| shelf.id).collect();
            let position = imp.shelf_focus.borrow().position(&(), &ids);
            return rows
                .get(position)
                .and_then(|shelf| self.remembered_in(shelf));
        }
        match imp.open.get() {
            None => {
                let row = imp.row.borrow();
                let items: Vec<Item> = row
                    .iter()
                    .map(|(_, focused)| focused.item())
                    .chain([Item::Library])
                    .collect();
                let position = imp.first_focus.borrow().position(&(), &items);
                Some(match row.get(position) {
                    Some((tile, focused)) => (tile.clone(), focused.clone()),
                    None => (imp.library_tile.get(), Focused::Library),
                })
            }
            Some(open) => {
                let cards = imp.cards.borrow();
                let items: Vec<Item> = cards
                    .iter()
                    .map(|(_, game)| Item::Game(game.id()))
                    .collect();
                let position = imp.game_focus.borrow().position(&Some(open), &items);
                cards
                    .get(position)
                    .map(|(card, game)| (card.clone(), Focused::Game(game.clone())))
            }
        }
    }

    /// The card or tile of `shelf` that last had focus there, or its first.
    fn remembered_in(&self, shelf: &Shelf) -> Option<(gtk::Button, Focused)> {
        let shown: Vec<&(gtk::Button, Focused)> = shelf
            .items
            .iter()
            .filter(|(tile, _)| tile.get_visible())
            .collect();
        let items: Vec<Item> = shown.iter().map(|(_, focused)| focused.item()).collect();
        let position = self.imp().game_focus.borrow().position(&shelf.id, &items);
        shown
            .get(position)
            .map(|&(tile, focused)| (tile.clone(), focused.clone()))
    }

    /// The game of the card with focus, if a game's card has it.
    pub fn focused_game(&self) -> Option<GameObject> {
        let imp = self.imp();
        let game = |(card, focused): &(gtk::Button, Focused)| match focused {
            Focused::Game(game) if card.has_focus() => Some(game.clone()),
            _ => None,
        };
        let on_second_screen = imp
            .cards
            .borrow()
            .iter()
            .find(|(card, _)| card.has_focus())
            .map(|(_, game)| game.clone());
        on_second_screen
            .or_else(|| imp.row.borrow().iter().find_map(game))
            .or_else(|| {
                let rows = imp.shelf_rows.borrow();
                rows.iter().flat_map(|shelf| &shelf.items).find_map(game)
            })
    }

    /// Whether one of Home's tiles or cards has focus.
    fn has_focus_within(&self) -> bool {
        self.imp()
            .content
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_WITHIN)
    }

    /// Notes the focused tile or card, scrolls it into view and shows what it stands for.
    fn card_focused(&self, card: &gtk::Button, focused: &Focused) {
        let imp = self.imp();
        let item = focused.item();
        if self.shelves() {
            let shelf = imp
                .shelf_rows
                .borrow()
                .iter()
                .find(|shelf| shelf.items.iter().any(|(tile, _)| tile == card))
                .map(|shelf| shelf.id);
            if let Some(shelf) = shelf {
                imp.game_focus.borrow_mut().remember(shelf, item);
                imp.shelf_focus.borrow_mut().remember((), shelf);
                self.scroll_to_shelf();
            }
        } else if let Some(open) = imp.open.get() {
            if let Item::Game(_) = item {
                imp.game_focus.borrow_mut().remember(Some(open), item);
            }
        } else if item != Item::BigPicture {
            imp.first_focus.borrow_mut().remember((), item);
        }
        keep_in_view(card, false);
        self.show_focused(focused);
    }

    /// Shows what has focus: a game's hero, logo or title and details, under "Jump Back
    /// In" (`.home-eyebrow`) on a first screen that starts with the games; a section's
    /// name and details, without art, or over its first game's on such a first screen,
    /// with its first covers (`.home-preview`) where the design asks for them; the title
    /// alone for the Library and Steam Big Picture tiles.
    fn show_focused(&self, focused: &Focused) {
        let imp = self.imp();
        let layout = imp.layout.get();
        let first_screen = self.on_first_screen();
        let starts_with_games = first_screen && layout.home_start == HomeStart::Games;
        imp.eyebrow
            .set_visible(starts_with_games && matches!(focused, Focused::Game(_)));
        let sections = imp.sections.borrow();
        let section = match focused {
            Focused::Section(id) | Focused::SeeAll(id) => sections.get(*id),
            _ => None,
        };
        let on_tile = matches!(focused, Focused::Section(_));
        self.show_preview(
            section.filter(|_| first_screen && on_tile && layout.home_section_preview),
        );
        match (focused, section) {
            (Focused::Game(game), _) => self.show_game(game),
            (Focused::BigPicture, _) => self.show_title(&gettext("Steam Big Picture")),
            (Focused::Section(_) | Focused::SeeAll(_), Some((section, games))) => {
                let art = games
                    .first()
                    .filter(|_| starts_with_games && on_tile)
                    .and_then(hero_art);
                self.show_section_info(section, art);
            }
            (Focused::Library | Focused::Section(_) | Focused::SeeAll(_), _) => {
                self.show_title(&gettext("Library"));
            }
        }
    }

    /// Shows `title` alone: no logo, art or details.
    fn show_title(&self, title: &str) {
        let imp = self.imp();
        imp.hero_generation.set(imp.hero_generation.get() + 1);
        self.hide_hero_art();
        imp.logo.set_paintable(gtk::gdk::Paintable::NONE);
        imp.logo.set_visible(false);
        imp.title.set_visible(true);
        imp.title.set_label(title);
        self.fill_details(&[]);
    }

    /// Shows a section's name with its details, over `art` if it is given some.
    fn show_section_info(&self, section: &Section, art: Option<PathBuf>) {
        let imp = self.imp();
        let generation = imp.hero_generation.get() + 1;
        imp.hero_generation.set(generation);
        imp.logo.set_paintable(gtk::gdk::Paintable::NONE);
        imp.logo.set_visible(false);
        imp.title.set_visible(true);
        imp.title.set_label(&section_title(section));
        self.fill_details(&home::section_details(section));
        self.show_art(art, None, generation);
    }

    /// Fills the strip of the focused section's first covers (`.home-preview`,
    /// `home::preview`): a cover each, then `+N` (`.preview-more`) for the games beyond
    /// them. Without a section the strip hides. Art in memory shows at once.
    fn show_preview(&self, section: Option<(&Section, &[GameObject])>) {
        let strip = &self.imp().preview;
        while let Some(cover) = strip.first_child() {
            strip.remove(&cover);
        }
        let Some((section, games)) = section else {
            strip.set_visible(false);
            return;
        };
        let preview = home::preview(section);
        let (width, height) = PREVIEW_COVER;
        let scale = self.scale_factor().max(1);
        let pixels = ((width * scale) as u32, (height * scale) as u32);
        for id in &preview.games {
            let Some(game) = games.iter().find(|game| game.id() == *id) else {
                continue;
            };
            let entry = game.entry();
            let art = home::card_art(TileShape::Portrait, &entry.art);
            let cover = CoverArt::new();
            cover.set_size_request(width, height);
            cover.set_alternative_text(&entry.game.title);
            cover.set_crop(art.crop);
            cover.show(art.art, None, pixels, |_| {});
            strip.append(&cover);
        }
        if preview.more > 0 {
            // Translators: how many more games a section holds than the covers shown
            // beside this, as in "+6".
            let more = gettext("+{count}").replace("{count}", &preview.more.to_string());
            strip.append(
                &gtk::Label::builder()
                    .label(&more)
                    .width_request(width)
                    .height_request(height)
                    .css_classes(["preview-more"])
                    .build(),
            );
        }
        strip.set_visible(true);
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
    /// crossfades them. Pindu's own style hides a hero without `.shown` at once. The art
    /// covers Home's background, which pauses under it.
    fn show_hero_art(&self, texture: &gtk::gdk::Texture) {
        let imp = self.imp();
        let (outgoing, incoming) = self.heroes();
        incoming.set_paintable(Some(texture));
        incoming.add_css_class("shown");
        outgoing.remove_css_class("shown");
        imp.hero_b_latest.set(!imp.hero_b_latest.get());
        imp.home.remove_css_class("no-art");
        if !imp.art_shown.replace(true) {
            self.show_background_playback();
        }
    }

    /// Hides the hero art, keeping it in its picture so a design can fade it out, and marks
    /// the page `.no-art`: Home's background shows again.
    fn hide_hero_art(&self) {
        let imp = self.imp();
        imp.hero_a.remove_css_class("shown");
        imp.hero_b.remove_css_class("shown");
        imp.home.add_css_class("no-art");
        if imp.art_shown.replace(false) {
            self.show_background_playback();
        }
    }

    /// Shows the game's hero art (or its cover) with its logo (or its title), and its
    /// details when the design asks for them.
    fn show_game(&self, game: &GameObject) {
        let imp = self.imp();
        let generation = imp.hero_generation.get() + 1;
        imp.hero_generation.set(generation);
        imp.logo.set_paintable(gtk::gdk::Paintable::NONE);
        imp.logo.set_visible(false);
        imp.title.set_visible(true);
        let (title, logo) = {
            let now = glib::DateTime::now_local().ok();
            let (now, offset) =
                now.map_or((0, 0), |now| (now.to_unix(), now.utc_offset().as_seconds()));
            let entry = game.entry();
            self.fill_details(&home::details(
                &entry.game,
                entry.playtime_secs,
                now,
                offset,
            ));
            (entry.game.title.clone(), entry.art.logo.clone())
        };
        imp.title.set_label(&title);
        imp.logo
            .update_property(&[gtk::accessible::Property::Label(&title)]);
        self.show_art(hero_art(game), logo, generation);
    }

    /// Shows `art` as the hero and `logo` in the title's place, for the hero of
    /// `generation`. The page carries `.no-art` while no hero art shows. Art in memory
    /// shows at once, so moving along the carousel does not flicker; art that still has
    /// to load replaces the hero shown once it has, so a design fades straight from one
    /// to the other, and is dropped if another hero was asked for meanwhile.
    fn show_art(&self, art: Option<PathBuf>, logo: Option<PathBuf>, generation: u64) {
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

/// The art that fills the page behind a game: its hero, or else its cover.
fn hero_art(game: &GameObject) -> Option<PathBuf> {
    let entry = game.entry();
    entry.art.hero.clone().or_else(|| entry.art.cover.clone())
}

/// How many frames a card that was only just built is given to get its place in its
/// row, before the row is left where it is.
const PLACE_FRAMES: u32 = 30;

/// Scrolls `card`'s row sideways so the card shows whole, with room beside it for the
/// ring and the growth a design gives the focused card. A card that was only just built
/// has no place yet: its row scrolls once it has. With `relaid` the row may have been
/// rebuilt around a card that still holds its old place: it scrolls again once it is
/// laid out.
fn keep_in_view(card: &gtk::Button, relaid: bool) {
    let placed = card.width() > 0;
    if placed {
        scroll_to_card(card);
        if !relaid {
            return;
        }
    }
    let frames = Cell::new(0);
    // A frame's tick comes before its layout: the second one with a placed card sees
    // the row as it now is.
    let placed_frames = Cell::new(0);
    card.add_tick_callback(move |card, _| {
        if card.width() > 0 {
            scroll_to_card(card);
            placed_frames.set(placed_frames.get() + 1);
        }
        frames.set(frames.get() + 1);
        if placed_frames.get() < 2 && frames.get() < PLACE_FRAMES {
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Break
        }
    });
}

/// Scrolls the row of `card`, which has its place there, so the card shows whole.
fn scroll_to_card(card: &gtk::Button) {
    if let Some(carousel) = card.parent().and_downcast::<gtk::Box>()
        && let Some(viewport) = carousel.parent().and_downcast::<gtk::Viewport>()
        && let Some(adjustment) = viewport.hadjustment()
        && let Some(bounds) = card.compute_bounds(&viewport)
    {
        let start = f64::from(bounds.x()) + adjustment.value();
        let end = start + f64::from(bounds.width());
        let margin = SCROLL_MARGIN + carousel_padding(&carousel);
        adjustment.clamp_page(start - margin, end + margin);
    }
}

/// The padding of `carousel` on each side, which only a design gives it: its width less
/// its cards' and the gaps between them, halved.
fn carousel_padding(carousel: &gtk::Box) -> f64 {
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

/// Shows a card's art at `size`, decoded for a design that scales the card up.
fn show_card_art(picture: &CoverArt, art: CardArt, size: (i32, i32), scale_factor: i32) {
    picture.set_crop(art.crop);
    picture.show(art.art, None, home::art_pixels(size, scale_factor), |_| {});
}
