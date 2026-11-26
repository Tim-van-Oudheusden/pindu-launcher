//! Fullscreen mode's Home page, where the mode opens. A design chooses one of four
//! layouts (`pindu_core::themes::HomeLayout`). Two of them have two screens. The first
//! lists Home's sections (`pindu_core::sections`), a tile each, then the Library tile and,
//! where Steam is present, the Steam Big Picture tile (`win.steam-big-picture`), over
//! Home's background, with the focused section's name and game count; a design can start
//! the row with the Recently Played games as cards (`home-start`), label the tiles
//! (`home-section-tile`) and preview the focused section's covers (`home-section-preview`).
//! Activating a section (`win.home-section`) opens the second screen: its games, with the
//! focused game's hero behind them. The Categories tile, after Installed, opens the
//! Categories screen instead (`.home-categories`): a tile per category, each opening its
//! games. The Apps tile, after it while there are apps (`set_apps`), opens the window's
//! Apps page (`apps_page`). B goes back one screen, to the tile it came from. The other
//! two are one screen of rows that scroll; Up and Down change row (`HomePage::navigate`).
//! Shelves: every section as a row of its games (`pindu_core::home::shelves`), then the
//! eight categories with the most games (`.home-shelf-category`), the More Categories row
//! for the others, and the Library row, which holds the Apps tile. Explore
//! (`home::explore_rows`),
//! Adwaita's: a carousel of the Jump Back In games with Play and Details on each page
//! (`.home-featured`), a row of compact section tiles with the Library and Big Picture
//! tiles, then a row of games for each other section, all on the window's own background.
//! Each screen and row comes back to the tile or card that last had focus. The page
//! carries `.home-sections` or `.home-games` for designs, which can choose the tiles'
//! shape and size (`pindu_core::home`) and crossfade the hero (`.shown`, `.no-art`).
//! Behind the first screen plays Home's background (`.home-background`), as
//! `home::background_playback` says for where the window is; it holds its first frame for
//! good once the window's frame rate falls well short of it (`home::FrameRateWatch`).
//! Quick Access's Blur and Dim name their strength on the page (`blur-*`, `dim-*`,
//! `home::background_classes`), which blurs the background and the hero and darkens them
//! under `.home-dim`. Art that has to load waits for focus to rest
//! (`pindu_core::gamepad::FOCUS_REST`), and focus held moving through the items keeps the
//! hero shown until then, so a held direction or arrow key keeps its pace and stops where
//! it is let go.

use std::cell::{Cell, OnceCell, RefCell};
use std::path::{Path, PathBuf};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::glib::translate::IntoGlib;
use gtk::{gio, glib};
use pindu_core::focus::FocusMemory;
use pindu_core::gamepad::{Direction, FOCUS_REST};
use pindu_core::home::{
    self, Background, BackgroundContext, CardArt, Detail, ExploreRow, FRAME_RATE_SAMPLE_INTERVAL,
    FeaturedButton, FrameRateWatch, Playback, RowItem, ShelfStep, Strength,
};
use pindu_core::keys::HeldKey;
use pindu_core::library::{ArtSet, HistoryGroup};
use pindu_core::model::GameId;
use pindu_core::sections::{self, Section, SectionId};
use pindu_core::themes::{HomeLayout, HomeStart, SectionTile, ThemeLayout, TileShape};

use crate::config::{PKGDATADIR, RESOURCE_BASE_PATH};
use crate::fullscreen_panels::step_out_of_list;
use crate::game_page::{date, playtime};
use crate::genres::genre_label;
use crate::models::GameObject;
use crate::sources::{source_icon, source_name};
use crate::widgets::{CoverArt, SpacedLabel, TileFrame, textures};
use crate::window::{bind_play_button, jump_card_frame, show_art_class, show_jump_art};

/// Fills `row`, a `.home-details` row, with `details` as `.detail` items, in place of
/// those it held.
fn put_details(row: &gtk::Box, details: &[Detail]) {
    while let Some(item) = row.first_child() {
        row.remove(&item);
    }
    for detail in details {
        row.append(&detail_item(*detail));
    }
}

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
        Detail::Categories(count) => (
            "view-app-grid-symbolic",
            category_count(count),
            gettext("Categories"),
        ),
        Detail::Apps(count) => (APPS_ICON, app_count(count), gettext("Apps")),
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

/// How many categories the Categories tile leads to, as its details row and its labelled
/// tile say it.
fn category_count(count: u32) -> String {
    ngettext("{count} category", "{count} categories", count).replace("{count}", &count.to_string())
}

/// The icon that stands for Apps, on its tile and in the desktop window's sidebar.
pub(crate) const APPS_ICON: &str = "preferences-desktop-apps-symbolic";
/// The icon of the Get Games tile and its pages.
pub(crate) const GET_GAMES_ICON: &str = "system-software-install-symbolic";

/// How many apps the Apps tile leads to, as its details row and its labelled tile say
/// it.
pub(crate) fn app_count(count: u32) -> String {
    ngettext("{count} app", "{count} apps", count).replace("{count}", &count.to_string())
}

/// How many games `section` holds.
fn games_in(section: &Section) -> u32 {
    u32::try_from(section.games.len()).unwrap_or(u32::MAX)
}

/// A section's name: a category's genre as Pindu shows it (`genre_label`), the user's
/// for a collection, else Home's own.
fn section_title(section: &Section) -> String {
    match (section.id, &section.title) {
        (SectionId::Category(_), Some(genre)) => genre_label(genre),
        (_, Some(title)) => title.clone(),
        (SectionId::RecentlyPlayed, None) => gettext("Recently Played"),
        (SectionId::Favourites, None) => gettext("Favourites"),
        (SectionId::Installed, None) => gettext("Installed"),
        (SectionId::Source(source), None) => source_name(source),
        (SectionId::Categories, None) => gettext("Categories"),
        (SectionId::Apps, _) => gettext("Apps"),
        (SectionId::GetGames, _) => gettext("Get Games"),
        (SectionId::Collection(_) | SectionId::Category(_), None) => String::new(),
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

/// The side of the icon at the left of a compact tile, in pixels.
const COMPACT_GLYPH_SIZE: i32 = 20;

/// The size of a compact tile, in the Explore layout's row of tiles, in pixels.
const COMPACT_TILE: (i32, i32) = (176, 64);

/// The height of a page of the Explore layout's featured row, in pixels; it grows where
/// its text needs more.
const FEATURED_HEIGHT: i32 = 244;

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

/// The glyph of a section's labelled tile (`.section-glyph`): a launcher's own icon where
/// Pindu ships one (`sections::source_icon`), else its monogram, or the icon the desktop
/// sidebar shows for the section, `size` pixels.
fn section_glyph(id: SectionId, size: i32) -> gtk::Widget {
    let icon = match id {
        SectionId::RecentlyPlayed => "document-open-recent-symbolic",
        SectionId::Favourites => "starred-symbolic",
        SectionId::Installed => "drive-harddisk-symbolic",
        SectionId::Collection(_) => "folder-symbolic",
        SectionId::Categories => "view-app-grid-symbolic",
        SectionId::Apps => APPS_ICON,
        SectionId::GetGames => GET_GAMES_ICON,
        SectionId::Category(_) => "view-list-bullet-symbolic",
        SectionId::Source(source) => {
            match (sections::source_icon(source), sections::monogram(source)) {
                (Some(icon), _) => icon,
                (None, Some(monogram)) => {
                    return gtk::Label::builder()
                        .label(monogram)
                        .halign(gtk::Align::Start)
                        .css_classes(["section-glyph"])
                        .build()
                        .upcast();
                }
                (None, None) => source_icon(source),
            }
        }
    };
    gtk::Image::builder()
        .icon_name(icon)
        .pixel_size(size)
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

/// A section's tile (`.home-section-tile`), which opens it through `action`
/// (`win.home-section`, or `win.home-section-library` on the More Categories row),
/// `width`×`height` pixels. As art (`SectionTile::Art`): its art (`data/home/<name>.svg`)
/// filling the tile, or for a collection or a category, which have none, an icon over its
/// name (`.home-collection-tile`); the art is an overlay, which does not size the tile.
/// Labelled (`SectionTile::Label`, `.labelled` and the section's own class): a glyph at
/// the top left, and at the bottom left its name over `count`, its number of games. A
/// category's tile (`.section-category`) and the Categories tile (`.section-categories`)
/// carry their class and say their count to screen readers either way.
fn section_tile(
    id: SectionId,
    title: &str,
    count: &str,
    size: (i32, i32),
    kind: SectionTile,
    action: &str,
) -> gtk::Button {
    let tile = gtk::Button::builder()
        .overflow(gtk::Overflow::Hidden)
        .tooltip_text(title)
        .action_name(action)
        .action_target(&id.name().to_variant())
        .css_classes(["card", "home-section-tile"])
        .build();
    let no_art = matches!(id, SectionId::Collection(_) | SectionId::Category(_));
    if no_art {
        tile.add_css_class("home-collection-tile");
    }
    tile.update_property(&[gtk::accessible::Property::Label(title)]);
    if matches!(
        id,
        SectionId::Categories | SectionId::Category(_) | SectionId::Apps | SectionId::GetGames
    ) {
        tile.add_css_class(&id.css_class());
        tile.update_property(&[gtk::accessible::Property::Description(count)]);
    }
    if kind == SectionTile::Label {
        tile.add_css_class("labelled");
        tile.add_css_class(&id.css_class());
        let label = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["section-label"])
            .build();
        label.append(&section_glyph(id, GLYPH_SIZE));
        label.append(&section_name(title));
        label.append(&section_count(count));
        tile.set_child(Some(&TileFrame::new(&label, size)));
        tile.update_property(&[gtk::accessible::Property::Description(count)]);
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
    if no_art {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .valign(gtk::Align::Center)
            .margin_start(12)
            .margin_end(12)
            .build();
        let icon = if matches!(id, SectionId::Category(_)) {
            "view-list-bullet-symbolic"
        } else {
            "folder-symbolic"
        };
        content.append(
            &gtk::Image::builder()
                .icon_name(icon)
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
        if matches!(id, SectionId::Category(_)) {
            // Its count stays whole where the name ends in an ellipsis.
            content.append(
                &gtk::Label::builder()
                    .label(count)
                    .css_classes(["section-count", "dimmed"])
                    .build(),
            );
        }
        frame.add_overlay(&content);
    } else {
        let resource = format!("{RESOURCE_BASE_PATH}/home/{}.svg", id.name());
        let art = gtk::Picture::for_resource(&resource);
        art.set_content_fit(gtk::ContentFit::Cover);
        art.set_can_shrink(true);
        art.set_accessible_role(gtk::AccessibleRole::Presentation);
        frame.add_overlay(&art);
    }
    tile.set_child(Some(&frame));
    tile
}

/// A section's compact tile (`.home-section-tile.labelled.compact`), in the Explore
/// layout's row of tiles: its glyph at the left and its name over `count`, its number of
/// games, beside it, `COMPACT_TILE` pixels. It moves focus to the section's row
/// (`win.home-section`); the Categories tile to the first category's.
fn compact_section_tile(id: SectionId, title: &str, count: &str) -> gtk::Button {
    let tile = gtk::Button::builder()
        .overflow(gtk::Overflow::Hidden)
        .tooltip_text(title)
        .action_name("win.home-section")
        .action_target(&id.name().to_variant())
        .css_classes(["card", "home-section-tile", "labelled", "compact"])
        .build();
    tile.add_css_class(&id.css_class());
    tile.update_property(&[
        gtk::accessible::Property::Label(title),
        gtk::accessible::Property::Description(count),
    ]);
    let glyph = section_glyph(id, COMPACT_GLYPH_SIZE);
    glyph.set_valign(gtk::Align::Center);
    let name = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["section-name"])
        .build();
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    text.append(&name);
    text.append(&section_count(count));
    let label = gtk::Box::builder()
        .spacing(12)
        .css_classes(["section-label"])
        .build();
    label.append(&glyph);
    label.append(&text);
    tile.set_child(Some(&TileFrame::new(&label, COMPACT_TILE)));
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

/// What a row of the one-screen layouts shows, to rebuild it only when that changes: the
/// row, its title, its number of games and its cards and tiles.
type ShelfKey = (RowId, String, usize, Vec<RowKey>);

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

/// Home's sections, in order, and its categories, by name, with the games of each, how
/// many apps the Apps tile leads to, and whether this build offers Get Games.
#[derive(Debug, Default)]
struct Sections {
    sections: Vec<Section>,
    games: Vec<Vec<GameObject>>,
    categories: Vec<Section>,
    category_games: Vec<Vec<GameObject>>,
    apps: u32,
    get_games: bool,
}

impl Sections {
    /// The section or category `id` with its games, if Home has it.
    fn get(&self, id: SectionId) -> Option<(&Section, &[GameObject])> {
        let (sections, games) = match id {
            SectionId::Category(_) => (&self.categories, &self.category_games),
            _ => (&self.sections, &self.games),
        };
        let position = sections.iter().position(|section| section.id == id)?;
        let games = games.get(position)?;
        Some((&sections[position], games))
    }

    /// The name, and the count its tile shows, of the tile `id` stands for: a section, a
    /// category, the Categories tile, which counts the categories, the Apps tile, which
    /// counts the apps, or the Get Games tile, which says where its games come from.
    fn tile(&self, id: SectionId) -> Option<(String, u32, String)> {
        if id == SectionId::Categories {
            if self.categories.is_empty() {
                return None;
            }
            let count = u32::try_from(self.categories.len()).unwrap_or(u32::MAX);
            return Some((gettext("Categories"), count, category_count(count)));
        }
        if id == SectionId::Apps {
            return (self.apps > 0).then(|| (gettext("Apps"), self.apps, app_count(self.apps)));
        }
        if id == SectionId::GetGames {
            return self
                .get_games
                .then(|| (gettext("Get Games"), 0, gettext("From Flathub")));
        }
        let (section, _) = self.get(id)?;
        let count = games_in(section);
        Some((section_title(section), count, game_count(count)))
    }

    /// The tiles that lead elsewhere than the library, after the Categories tile: Apps
    /// while there are apps, Get Games where the build offers it.
    fn elsewhere(&self) -> Vec<SectionId> {
        [
            (self.apps > 0).then_some(SectionId::Apps),
            self.get_games.then_some(SectionId::GetGames),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// A row of Home's one-screen layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RowId {
    /// The Explore layout's carousel of the Jump Back In games.
    Featured,
    /// The Explore layout's row of section tiles, with the Library and Steam Big Picture
    /// tiles.
    Tiles,
    /// A section's games.
    Section(SectionId),
    /// The More Categories row: a tile for each category without a row of its own.
    MoreCategories,
    /// The Shelves layout's last row: the Library and Steam Big Picture tiles.
    Library,
}

/// A row of the one-screen layouts (`.home-shelf`, or `.home-featured` for the carousel).
#[derive(Debug)]
struct Shelf {
    id: RowId,
    key: ShelfKey,
    row: gtk::Box,
    /// The row's cards, tiles and buttons, in order, each with what it stands for. On the
    /// featured row, each page's Play then its Details.
    items: Vec<(gtk::Button, Focused)>,
    /// The shape and size of the row's cards.
    shape: TileShape,
    size: (i32, i32),
    /// The cards that have not loaded their art yet: they do once the row is near the
    /// part of the list that shows.
    pending: RefCell<Vec<(CoverArt, GameObject)>>,
}

/// What waits for focus to rest (`FOCUS_REST`) before it loads, so focus held through
/// many items does not load art for each one it passes.
#[derive(Debug, Default)]
struct Waiting {
    /// The hero art and logo `show_art` was given, for the hero of the generation given.
    hero: Option<(Option<PathBuf>, Option<PathBuf>, u64)>,
    /// The preview strip's covers whose art is not in memory, each with its art and size
    /// in pixels.
    covers: Vec<(CoverArt, PathBuf, (u32, u32))>,
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
        /// Whether the machine keeps up with the background while it plays.
        pub frame_rate_watch: Cell<FrameRateWatch>,
        /// Samples the frame rate into `frame_rate_watch` while the background plays.
        pub frame_rate_sampler: RefCell<Option<glib::SourceId>>,
        #[template_child]
        pub hero_a: TemplateChild<gtk::Picture>,
        #[template_child]
        pub hero_b: TemplateChild<gtk::Picture>,
        #[template_child]
        pub dim: TemplateChild<gtk::Box>,
        #[template_child]
        pub hero_gradient: TemplateChild<gtk::Box>,
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
        pub setup: TemplateChild<adw::Clamp>,
        #[template_child]
        pub setup_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub setup_hide: TemplateChild<gtk::Button>,
        /// The checklist's row last chosen, which focus goes back to.
        pub setup_focus: Cell<i32>,
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
        /// The Library tile's name and number of games, beside its icon while the tile is
        /// compact.
        pub library_tile_text: RefCell<Option<gtk::Box>>,
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
        /// The Steam Big Picture tile's name and status, beside its icon while the tile is
        /// compact.
        pub big_picture_text: RefCell<Option<gtk::Box>>,
        /// Home's sections, in order, and its categories, each with its games.
        pub(super) sections: RefCell<Sections>,
        /// The section or category whose games the second screen shows, or
        /// `SectionId::Categories` while the Categories screen shows; `None` on the first
        /// screen and in the one-screen layouts.
        pub open: Cell<Option<SectionId>>,
        /// What the first screen's row, or the Categories screen's, shows before the
        /// Library tile, in order.
        pub(super) row_shown: RefCell<Vec<RowKey>>,
        /// The cards and tiles of the first screen's row, or the Categories screen's, each
        /// with what it stands for.
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
        /// The card or tile that last had focus in each row: a section's games on the
        /// second screen, and each row of the one-screen layouts.
        pub(super) game_focus: RefCell<FocusMemory<RowId, Item>>,
        /// The row that last had focus in the one-screen layouts.
        pub(super) shelf_focus: RefCell<FocusMemory<(), RowId>>,
        /// What the Shelves layout does once its rows have their place: `Some` while that
        /// is queued, `true` if the row with focus goes back to the top then.
        pub shelves_settle: Cell<Option<bool>>,
        /// Bumped for every hero shown, so art that loads late for an earlier one is dropped.
        pub hero_generation: Cell<u64>,
        /// Cancels the decode of the hero art loading, once another hero is asked for.
        pub hero_loading: RefCell<Option<gio::Cancellable>>,
        /// The wait for focus to rest where it moved to (`FOCUS_REST`).
        pub rest: RefCell<Option<glib::SourceId>>,
        /// The tile or card focus is to rest on.
        pub(super) rest_item: RefCell<Option<Item>>,
        /// Whether focus is moving through items, held: it moved again before it rested.
        /// Hero art waits for it to rest then, even art in memory.
        pub moving: Cell<bool>,
        /// The art that waits for focus to rest before it loads.
        pub(super) waiting: RefCell<Waiting>,
        /// The preview strip's covers and its `+N`, made once and filled for each section.
        pub preview_items: OnceCell<(Vec<CoverArt>, gtk::Label)>,
        /// The arrow key held down, to tell the keyboard's repeats of it from presses.
        pub held_key: RefCell<HeldKey>,
        /// Whether a step the arrow keys made has yet to be drawn: their repeats are
        /// dropped until it has, so they never queue up ahead of the frames.
        pub step_pending: Cell<bool>,
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
            self.setup_list.connect_row_activated(glib::clone!(
                #[weak]
                page,
                move |_, row| page.imp().setup_focus.set(row.index())
            ));
            // Down from the last row goes on to Hide This List; Up from the first goes on
            // to the top bar, as from Home's tiles.
            self.setup_list.connect_keynav_failed(|list, direction| {
                if direction == gtk::DirectionType::Down {
                    step_out_of_list(list, direction);
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
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
                    move |_, key, _, state| page.arrow_pressed(key, state)
                ));
                arrows.connect_key_released(glib::clone!(
                    #[weak]
                    page,
                    move |_, key, _, _| page.imp().held_key.borrow_mut().release(key.into_glib())
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
        if layout.home.one_screen() {
            // One screen: no section's games are open.
            imp.open.set(None);
            imp.library_tile.set_visible(true);
            imp.big_picture_tile
                .set_visible(imp.big_picture_offered.get());
        }
        self.show_home_layout(layout.home);
        // The tiles take the labelled shape before they lose or take the compact one.
        self.show_tile_compact(false);
        self.show_tile_shape(layout.home_tile, layout.home_tile_size);
        self.show_tile_labels(layout.home_section_tile == SectionTile::Label);
        self.show_tile_compact(layout.home == HomeLayout::Explore);
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
    /// them, or shows the rows under them in its place, or the rows alone in the Explore
    /// layout, which shows no title, art or background over the window's own; and names
    /// the layout on the page (`home-layout-hero-bottom`, `home-layout-row-top`,
    /// `home-layout-shelves`, `home-layout-explore`) for designs. Moving the rows keeps
    /// focus where it is.
    fn show_home_layout(&self, layout: HomeLayout) {
        let imp = self.imp();
        for other in HomeLayout::ALL {
            imp.home
                .remove_css_class(&format!("home-layout-{}", other.name()));
        }
        imp.home
            .add_css_class(&format!("home-layout-{}", layout.name()));
        let content = &imp.content;
        let shelves = layout.one_screen();
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
            HomeLayout::Shelves | HomeLayout::Explore => {
                content.reorder_child_after(&*imp.shelves_scroller, Some(&*imp.info));
                content.set_valign(gtk::Align::Fill);
                content.set_margin_top(if layout == HomeLayout::Explore { 0 } else { 24 });
                content.set_margin_bottom(0);
            }
        }
        imp.carousel_row.set_visible(!shelves);
        imp.shelves_scroller.set_visible(shelves);
        let explore = layout == HomeLayout::Explore;
        for backdrop in [
            imp.info.upcast_ref::<gtk::Widget>(),
            imp.background.upcast_ref(),
            imp.hero_a.upcast_ref(),
            imp.hero_b.upcast_ref(),
            imp.dim.upcast_ref(),
            imp.hero_gradient.upcast_ref(),
        ] {
            backdrop.set_visible(!explore);
        }
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

    /// Makes the Library and Steam Big Picture tiles compact, as the Explore layout's
    /// section tiles are (`.compact`, `COMPACT_TILE`): the icon at the left, and the name
    /// over the number of games or the status beside it. Or back to the labelled shape
    /// `show_tile_labels` gave them, which it takes before it changes them again.
    fn show_tile_compact(&self, compact: bool) {
        let imp = self.imp();
        let tiles = [
            (
                &*imp.library_tile,
                &*imp.library_tile_content,
                &imp.library_tile_text,
                vec![imp.library_tile_label.clone().upcast::<gtk::Widget>()]
                    .into_iter()
                    .chain(
                        imp.library_tile_count
                            .borrow()
                            .clone()
                            .map(|count| count.upcast()),
                    )
                    .collect::<Vec<gtk::Widget>>(),
            ),
            (
                &*imp.big_picture_tile,
                &*imp.big_picture_tile_content,
                &imp.big_picture_text,
                vec![
                    imp.big_picture_label.clone().upcast(),
                    imp.big_picture_status.clone().upcast(),
                ],
            ),
        ];
        for (tile, content, text, labels) in tiles {
            if compact == text.borrow().is_some() {
                continue;
            }
            set_class(tile, "compact", compact);
            content.set_orientation(if compact {
                gtk::Orientation::Horizontal
            } else {
                gtk::Orientation::Vertical
            });
            if compact {
                content.set_spacing(12);
                content.set_valign(gtk::Align::Fill);
                let column = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .valign(gtk::Align::Center)
                    .hexpand(true)
                    .build();
                for label in &labels {
                    content.remove(label);
                    column.append(label);
                    label.set_vexpand(false);
                    label.set_valign(gtk::Align::Fill);
                }
                content.append(&column);
                text.replace(Some(column));
            } else if let Some(column) = text.take() {
                for label in &labels {
                    column.remove(label);
                    content.append(label);
                }
                content.remove(&column);
            }
        }
        let side = if compact {
            COMPACT_GLYPH_SIZE
        } else {
            GLYPH_SIZE
        };
        let icons = [
            imp.library_tile_icon.upcast_ref::<gtk::Widget>(),
            imp.big_picture_icon.upcast_ref(),
            imp.big_picture_spinner.upcast_ref(),
        ];
        for icon in icons {
            icon.set_valign(if compact {
                gtk::Align::Center
            } else {
                gtk::Align::Fill
            });
        }
        if compact {
            imp.library_tile_icon.set_pixel_size(side);
            imp.big_picture_icon.set_pixel_size(side);
            imp.big_picture_spinner.set_size_request(side, side);
            imp.library_tile_frame.set_size(COMPACT_TILE);
            imp.big_picture_tile_frame.set_size(COMPACT_TILE);
        }
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

    /// Whether the layout is one screen of rows: Shelves or Explore.
    fn shelves(&self) -> bool {
        self.imp().layout.get().home.one_screen()
    }

    /// Whether the first screen shows: no section's games do, and the layout has screens.
    fn on_first_screen(&self) -> bool {
        self.imp().open.get().is_none() && !self.shelves()
    }

    /// Whether the Categories screen shows.
    fn on_categories_screen(&self) -> bool {
        self.imp().open.get() == Some(SectionId::Categories)
    }

    /// Names the screen shown on the page for designs: `.home-sections` for the first,
    /// `.home-sections` and `.home-categories` for the Categories screen, `.home-games` for
    /// a section's games and for the Shelves layout, neither for the Explore layout; and
    /// `.home-start-games` on every screen while the first one's row starts with games.
    fn show_screen_class(&self) {
        let imp = self.imp();
        let home = &imp.home;
        let layout = imp.layout.get();
        home.remove_css_class("home-sections");
        home.remove_css_class("home-games");
        let categories = self.on_categories_screen();
        set_class(&**home, "home-categories", categories);
        // Explore's one screen is neither the sections nor a section's games.
        if layout.home != HomeLayout::Explore {
            let games = !self.on_first_screen() && !categories;
            home.add_css_class(if games { "home-games" } else { "home-sections" });
        }
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
        // A new stream warms up again once it plays.
        self.stop_frame_rate_sampler();
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

    /// The monitor the window is on, once it is shown.
    fn monitor(&self) -> Option<gtk::gdk::Monitor> {
        let surface = self.native()?.surface()?;
        surface.display().monitor_at_surface(&surface)
    }

    /// The size in device pixels of the screen the window is on, once it is shown.
    fn screen_pixels(&self) -> Option<(i32, i32)> {
        let monitor = self.monitor()?;
        let geometry = monitor.geometry();
        let scale = monitor.scale();
        Some((
            (f64::from(geometry.width()) * scale).round() as i32,
            (f64::from(geometry.height()) * scale).round() as i32,
        ))
    }

    /// Plays, pauses, holds Home's background on its first frame or takes it off the page,
    /// as `home::background_playback` says for where the window is, the screen shown, the
    /// art over it and whether playback stepped down. Taken off, `.home-background`'s
    /// plain black shows in its place.
    fn show_background_playback(&self) {
        let imp = self.imp();
        let Some(media) = imp.background_media.borrow().clone() else {
            return;
        };
        let context = imp.background_context.get();
        let context = BackgroundContext {
            first_screen: context.first_screen
                && matches!(imp.open.get(), None | Some(SectionId::Categories)),
            covered: context.covered || imp.art_shown.get(),
            stepped_down: imp.frame_rate_watch.get().stepped_down(),
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
        self.watch_frame_rate(playback == Playback::Play && shown);
    }

    /// Samples the window's frame rate while the background plays (`playing`), every
    /// `home::FRAME_RATE_SAMPLE_INTERVAL`, and stops when it does not. Starting again
    /// starts the watch's warm-up over.
    fn watch_frame_rate(&self, playing: bool) {
        let imp = self.imp();
        if playing == imp.frame_rate_sampler.borrow().is_some() {
            return;
        }
        if !playing {
            self.stop_frame_rate_sampler();
            return;
        }
        let mut watch = imp.frame_rate_watch.get();
        watch.resume();
        imp.frame_rate_watch.set(watch);
        let sampler = glib::timeout_add_local(
            FRAME_RATE_SAMPLE_INTERVAL,
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || page.sample_frame_rate()
            ),
        );
        imp.frame_rate_sampler.replace(Some(sampler));
    }

    /// Stops sampling the frame rate, if it was.
    fn stop_frame_rate_sampler(&self) {
        if let Some(sampler) = self.imp().frame_rate_sampler.take() {
            sampler.remove();
        }
    }

    /// Feeds the frame clock's frame rate and the monitor's refresh rate to the watch, and
    /// holds the background on its first frame once the watch steps it down.
    fn sample_frame_rate(&self) -> glib::ControlFlow {
        let imp = self.imp();
        let Some(clock) = self.frame_clock() else {
            return glib::ControlFlow::Continue;
        };
        let refresh_rate = self.monitor().map_or(0, |monitor| monitor.refresh_rate());
        let mut watch = imp.frame_rate_watch.get();
        let stepped_down = watch.sample(clock.fps(), refresh_rate, imp.background_choice.get());
        imp.frame_rate_watch.set(watch);
        if !stepped_down {
            return glib::ControlFlow::Continue;
        }
        glib::g_info!(
            "pindu",
            "Home's background holds its first frame: the window drew {:.0} frames a second",
            clock.fps()
        );
        // The source ends with `Break`: forget it rather than remove it.
        imp.frame_rate_sampler.take();
        self.show_background_playback();
        glib::ControlFlow::Break
    }

    /// Takes Home's sections and its categories, each with its games, and shows them on
    /// the screen shown. A section or category whose games show and that is gone now goes
    /// back a screen: a category to the Categories screen while there are categories, and
    /// the rest to the first screen. Focus that went to the Library tile only because
    /// Home had no sections yet goes to its first card or tile once it has some.
    pub fn set_sections(
        &self,
        sections: Vec<(Section, Vec<GameObject>)>,
        categories: Vec<(Section, Vec<GameObject>)>,
    ) {
        let imp = self.imp();
        let (sections, games): (Vec<Section>, Vec<Vec<GameObject>>) = sections.into_iter().unzip();
        let (categories, category_games): (Vec<Section>, Vec<Vec<GameObject>>) =
            categories.into_iter().unzip();
        let first = imp.sections.borrow().sections.is_empty()
            && !sections.is_empty()
            && imp.library_tile.is_focus();
        let (apps, get_games) = {
            let shown = imp.sections.borrow();
            (shown.apps, shown.get_games)
        };
        imp.sections.replace(Sections {
            sections,
            games,
            categories,
            category_games,
            apps,
            get_games,
        });
        self.refresh();
        if first {
            self.focus_first();
        }
    }

    /// Takes the number of installed apps (`pindu_core::apps`): Home shows the Apps tile
    /// while there are some, and none without.
    pub fn set_apps(&self, count: u32) {
        let imp = self.imp();
        if std::mem::replace(&mut imp.sections.borrow_mut().apps, count) != count {
            self.refresh();
        }
    }

    /// Says whether this build offers Get Games: the native one does, with a tile on Home;
    /// the Flatpak build, which cannot install from Flathub, does not.
    pub fn set_get_games_offered(&self, offered: bool) {
        let imp = self.imp();
        if std::mem::replace(&mut imp.sections.borrow_mut().get_games, offered) != offered {
            self.refresh();
        }
    }

    /// Focuses the first card or tile of the screen shown, or the first row's in the
    /// one-screen layouts, on the one that last had focus in that row. The row that holds
    /// the Library tile, which had focus only because Home had no sections, starts again
    /// at its first tile.
    fn focus_first(&self) {
        let imp = self.imp();
        if self.shelves() {
            let rows = imp.shelf_rows.borrow();
            if let Some(first) = rows.first() {
                imp.shelf_focus.borrow_mut().remember((), first.id);
            }
            let library = rows.iter().find(|shelf| {
                shelf
                    .items
                    .iter()
                    .any(|(tile, _)| *tile == *imp.library_tile)
            });
            if let Some(shelf) = library
                && let Some((_, focused)) = shelf.items.first()
            {
                imp.game_focus
                    .borrow_mut()
                    .remember(shelf.id, focused.item());
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
        let has_categories = !imp.sections.borrow().categories.is_empty();
        if open == SectionId::Categories {
            if has_categories {
                self.show_row();
            } else {
                self.show_sections();
            }
            return;
        }
        let games = imp
            .sections
            .borrow()
            .get(open)
            .map(|(_, games)| games.to_vec());
        match games {
            Some(games) if !games.is_empty() => self.show_cards(&games),
            _ if matches!(open, SectionId::Category(_)) && has_categories => {
                self.show_categories();
            }
            _ => self.show_sections(),
        }
    }

    /// Whether a screen past the first shows: a section's games or the categories, which
    /// B leaves. Never in the one-screen layouts.
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

    /// Shows the set-up checklist's `rows` in place of Home's content, or with `None`
    /// takes it away. Focus on a row stays on the row at its place once they are built
    /// again, and goes back to the tiles as the checklist goes. As it first shows, the row
    /// at `first`, the first not done, is where Home's focus goes (`focus_remembered`);
    /// `true` then.
    pub(crate) fn set_setup(&self, rows: Option<Vec<gtk::Widget>>, first: usize) -> bool {
        let imp = self.imp();
        let list = &imp.setup_list;
        let row_at = |index: usize| list.row_at_index(i32::try_from(index).ok()?);
        let focused = (0..)
            .map_while(row_at)
            .position(|row| row.has_focus() || row.focus_child().is_some());
        let in_setup = focused.is_some() || imp.setup_hide.has_focus();
        let was_shown = imp.setup.is_visible();
        list.remove_all();
        let shown = rows.is_some();
        for row in rows.into_iter().flatten() {
            list.append(&row);
        }
        imp.setup.set_visible(shown);
        imp.content.set_visible(!shown);
        if shown && !was_shown {
            imp.setup_focus
                .set(i32::try_from(first).unwrap_or_default());
        }
        if let Some(row) = focused.filter(|_| shown).and_then(row_at) {
            row.grab_focus();
        } else if !shown && was_shown && in_setup {
            self.show_sections();
            // Hide This List had focus: the first screen's tiles, shown again, take it once
            // they are mapped, so the controller is not left on nothing.
            glib::idle_add_local_once(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move || page.focus_remembered()
            ));
        }
        shown && !was_shown
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
    /// section whose games showed, or the Categories tile, when coming back from them.
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

    /// Goes back one screen: from a category's games to the Categories screen, focused on
    /// that category; from the Categories screen or a section's games to the first
    /// screen, focused on the tile that opened them.
    pub fn go_back(&self) {
        let from_category = matches!(self.imp().open.get(), Some(SectionId::Category(_)));
        if !(from_category && self.show_categories()) {
            self.show_sections();
        }
    }

    /// Shows the Categories screen, a tile per category, focused on the one that last had
    /// focus there; `false` while Home has no category.
    fn show_categories(&self) -> bool {
        let imp = self.imp();
        if imp.sections.borrow().categories.is_empty() {
            return false;
        }
        if imp.open.get().is_none() {
            imp.first_focus
                .borrow_mut()
                .remember((), Item::Section(SectionId::Categories));
        }
        self.clear_row();
        self.clear_cards();
        imp.open.set(Some(SectionId::Categories));
        imp.library_tile.set_visible(false);
        imp.big_picture_tile.set_visible(false);
        self.show_screen_class();
        self.show_row();
        self.focus_remembered();
        true
    }

    /// Shows the second screen with the games of the section or category named `name`
    /// (`SectionId::name`), focused on the one that last had focus there, or the
    /// Categories screen for the Categories tile; `false` when Home has no such section.
    /// In the one-screen layouts, which have no second screen, it focuses that section's
    /// row instead, or the first category's for the Categories tile.
    pub fn show_section(&self, name: &str) -> bool {
        let imp = self.imp();
        let Some(id) = SectionId::from_name(name) else {
            return false;
        };
        if self.shelves() {
            return self.focus_shelf(id);
        }
        if id == SectionId::Categories {
            return self.show_categories();
        }
        let games = imp
            .sections
            .borrow()
            .get(id)
            .map(|(_, games)| games.to_vec());
        let Some(games) = games.filter(|games| !games.is_empty()) else {
            return false;
        };
        // The tile B comes back to.
        match imp.open.get() {
            None => imp.first_focus.borrow_mut().remember((), Item::Section(id)),
            Some(SectionId::Categories) => imp
                .game_focus
                .borrow_mut()
                .remember(RowId::Section(SectionId::Categories), Item::Section(id)),
            Some(_) => {}
        }
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

    /// Takes the rows of the one-screen layouts away, and puts the Library and Steam Big
    /// Picture tiles back at the end of the carousel.
    fn clear_shelves(&self) {
        let imp = self.imp();
        for shelf in imp.shelf_rows.take() {
            if matches!(shelf.id, RowId::Library | RowId::Tiles) {
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
    /// as cards, a divider (`.home-divider`) and a tile for each other section, with the
    /// Categories tile after Installed. On the Categories screen, a tile for each category
    /// instead (`home::categories_row`). It is rebuilt only when what it shows changed.
    fn show_row(&self) {
        /// What an item of the row is built from: a tile's section, name and count.
        enum Shown<'a> {
            Card(&'a GameObject),
            Divider,
            Tile(SectionId, String, String),
        }
        let imp = self.imp();
        let layout = imp.layout.get();
        let labelled = layout.home_section_tile == SectionTile::Label;
        let sections = imp.sections.borrow();
        let recent = sections
            .get(SectionId::RecentlyPlayed)
            .map_or(&[][..], |(_, games)| games);
        let row = if self.on_categories_screen() {
            home::categories_row(&sections.categories)
                .into_iter()
                .map(RowItem::Section)
                .collect()
        } else {
            home::first_screen_row(
                layout.home_start,
                &sections.sections,
                &sections.categories,
                &sections.elsewhere(),
            )
        };
        let items: Vec<(RowKey, Shown<'_>)> = row
            .into_iter()
            .filter_map(|item| match item {
                RowItem::Game(id) => recent
                    .iter()
                    .find(|game| game.id() == id)
                    .map(|game| (RowKey::Card(card_key(game)), Shown::Card(game))),
                RowItem::Divider => Some((RowKey::Divider, Shown::Divider)),
                RowItem::Section(id) => sections.tile(id).map(|(title, count, text)| {
                    // A category's tile says its count whatever its kind.
                    let says_count = labelled || matches!(id, SectionId::Category(_));
                    (
                        RowKey::Tile(id, title.clone(), says_count.then_some(count)),
                        Shown::Tile(id, title, text),
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
                    Shown::Tile(id, title, count) => {
                        let tile = section_tile(
                            id,
                            &title,
                            &count,
                            size,
                            layout.home_section_tile,
                            "win.home-section",
                        );
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

    /// Shows the rows of the one-screen layouts. Shelves (`home::shelves`): one per
    /// section, with its first games as cards and See All where it holds more, one per
    /// category with the most games, the More Categories row (`home::more_categories`),
    /// then the Library row. Explore (`home::explore_rows`): the Jump Back In carousel, the
    /// row of section tiles with the Categories tile, then a row per other section and
    /// category, and the More Categories row. A row is rebuilt only when what it shows
    /// changed.
    fn show_shelves(&self) {
        /// What a row is built from; a tile is its section's ID, name and count.
        enum Parts<'a> {
            Featured(Vec<&'a GameObject>),
            Tiles(Vec<(SectionId, String, String)>),
            Section(home::Shelf, Vec<&'a GameObject>),
            MoreCategories(Vec<(SectionId, String, String)>),
            Library(Vec<(SectionId, String, String)>),
        }
        let imp = self.imp();
        let layout = imp.layout.get();
        let sections = imp.sections.borrow();
        let games_of = |id: SectionId, ids: &[GameId]| -> Vec<&GameObject> {
            let games = sections.get(id).map_or(&[][..], |(_, games)| games);
            ids.iter()
                .filter_map(|id| games.iter().find(|game| game.id() == *id))
                .collect()
        };
        let cards = |games: &[&GameObject]| -> Vec<RowKey> {
            games
                .iter()
                .map(|game| RowKey::Card(card_key(game)))
                .collect()
        };
        let section_row = |shelf: home::Shelf| -> Option<(ShelfKey, Parts<'_>)> {
            let (section, _) = sections.get(shelf.id)?;
            let games = games_of(shelf.id, &shelf.games);
            let key = (
                RowId::Section(shelf.id),
                shelf_title(section),
                shelf.total,
                cards(&games),
            );
            Some((key, Parts::Section(shelf, games)))
        };
        // The tiles of `ids`, each with its key.
        let tiles = |ids: &[SectionId]| -> (Vec<RowKey>, Vec<(SectionId, String, String)>) {
            ids.iter()
                .filter_map(|&id| {
                    let (title, count, text) = sections.tile(id)?;
                    Some((
                        RowKey::Tile(id, title.clone(), Some(count)),
                        (id, title, text),
                    ))
                })
                .unzip()
        };
        let more_row = |ids: &[SectionId]| -> Option<(ShelfKey, Parts<'_>)> {
            if ids.is_empty() {
                return None;
            }
            let (keys, tiles) = tiles(ids);
            let key = (RowId::MoreCategories, gettext("More Categories"), 0, keys);
            Some((key, Parts::MoreCategories(tiles)))
        };
        // What each row shows: its key, and what it is built from.
        let wanted: Vec<(ShelfKey, Parts<'_>)> = if layout.home == HomeLayout::Explore {
            home::explore_rows(
                &sections.sections,
                &sections.categories,
                &sections.elsewhere(),
            )
            .into_iter()
            .filter_map(|row| match row {
                ExploreRow::Featured(ids) => {
                    let games = games_of(SectionId::RecentlyPlayed, &ids);
                    let key = (RowId::Featured, String::new(), 0, cards(&games));
                    Some((key, Parts::Featured(games)))
                }
                ExploreRow::Tiles(ids) => {
                    let (keys, tiles) = tiles(&ids);
                    Some(((RowId::Tiles, String::new(), 0, keys), Parts::Tiles(tiles)))
                }
                ExploreRow::Shelf(shelf) => section_row(shelf),
                ExploreRow::MoreCategories(ids) => more_row(&ids),
            })
            .collect()
        } else {
            // The Library row leads elsewhere, as the Apps and Get Games tiles do: they
            // stand there.
            let (apps_keys, apps_tiles) = tiles(&sections.elsewhere());
            home::shelves(&sections.sections, &sections.categories)
                .into_iter()
                .filter_map(section_row)
                .chain(more_row(&home::more_categories(&sections.categories)))
                .chain([(
                    (RowId::Library, gettext("Library"), 0, apps_keys),
                    Parts::Library(apps_tiles),
                )])
                .collect()
        };
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
            let shelf = match kept {
                Some(shelf) => {
                    imp.shelves
                        .reorder_child_after(&shelf.row, previous.as_ref());
                    shelf
                }
                None => {
                    let shelf = match parts {
                        Parts::Featured(games) => self.featured_shelf(key, &games),
                        Parts::Tiles(tiles) => self.tiles_shelf(key, &tiles),
                        Parts::Section(shelf, games) => {
                            self.section_shelf(key, &shelf, &games, layout)
                        }
                        Parts::MoreCategories(tiles) => {
                            self.more_categories_shelf(key, &tiles, layout)
                        }
                        Parts::Library(tiles) => self.library_shelf(key, &tiles, layout),
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
        let scroller = sideways(&carousel);
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
    /// (`.shelf-more`) when the section holds more. A category's row also carries
    /// `.home-shelf-category`. The cards load their art once the row is near the part of
    /// the list that shows.
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
        if matches!(shelf.id, SectionId::Category(_)) {
            row.add_css_class("home-shelf-category");
        }
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
            id: RowId::Section(shelf.id),
            key,
            row,
            items,
            shape,
            size,
            pending: RefCell::new(pending),
        }
    }

    /// The More Categories row (`.home-shelf-more-categories`): a tile for each of
    /// `tiles`, the categories without a row of their own, by name, as the Categories
    /// screen shows them; each opens the library at All Games filtered to its category
    /// (`win.home-section-library`).
    fn more_categories_shelf(
        &self,
        key: ShelfKey,
        tiles: &[(SectionId, String, String)],
        layout: ThemeLayout,
    ) -> Shelf {
        let shape = layout.home_tile;
        let size = home::card_size(shape, layout.home_tile_size);
        let (row, carousel) = Self::shelf_frame(&key.1, None, shape);
        row.add_css_class("home-shelf-more-categories");
        let mut items: Vec<(gtk::Button, Focused)> = Vec::with_capacity(tiles.len());
        for (id, title, count) in tiles {
            let tile = section_tile(
                *id,
                title,
                count,
                size,
                layout.home_section_tile,
                "win.home-section-library",
            );
            let focused = Focused::Section(*id);
            tile.connect_has_focus_notify(glib::clone!(
                #[weak(rename_to = page)]
                self,
                #[strong]
                focused,
                move |tile| {
                    if tile.has_focus() {
                        page.card_focused(tile, &focused);
                    }
                }
            ));
            carousel.append(&tile);
            items.push((tile, focused));
        }
        Shelf {
            id: RowId::MoreCategories,
            key,
            row,
            items,
            shape,
            size,
            pending: RefCell::default(),
        }
    }

    /// The last row of the Shelves layout (`.shelf-library`): the Library tile, the tile
    /// of each of `tiles`, the Apps tile while there are apps, and, where Steam is present,
    /// the Steam Big Picture tile, the Library and Big Picture tiles moved here from the
    /// carousel.
    fn library_shelf(
        &self,
        key: ShelfKey,
        tiles: &[(SectionId, String, String)],
        layout: ThemeLayout,
    ) -> Shelf {
        let imp = self.imp();
        let shape = layout.home_tile;
        let size = home::card_size(shape, layout.home_tile_size);
        let (row, carousel) = Self::shelf_frame(&key.1, None, shape);
        row.add_css_class("shelf-library");
        for tile in [&*imp.library_tile, &*imp.big_picture_tile] {
            if let Some(parent) = tile.parent().and_downcast::<gtk::Box>() {
                parent.remove(tile);
            }
        }
        carousel.append(&*imp.library_tile);
        let mut items = vec![(imp.library_tile.get(), Focused::Library)];
        for (id, title, count) in tiles {
            let tile = section_tile(
                *id,
                title,
                count,
                size,
                layout.home_section_tile,
                "win.home-section",
            );
            let id = *id;
            tile.connect_has_focus_notify(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |tile| {
                    if tile.has_focus() {
                        page.card_focused(tile, &Focused::Section(id));
                    }
                }
            ));
            carousel.append(&tile);
            items.push((tile, Focused::Section(id)));
        }
        carousel.append(&*imp.big_picture_tile);
        items.push((imp.big_picture_tile.get(), Focused::BigPicture));
        Shelf {
            id: RowId::Library,
            key,
            row,
            items,
            shape,
            size,
            pending: RefCell::default(),
        }
    }

    /// The Explore layout's first row: a carousel (`.home-featured`) with a page for each
    /// of `games`, the Jump Back In games (`featured_page`), and its dots under it while
    /// it has more than one page. The carousel shows the page that has focus, and focus
    /// in the row follows a page turned by a swipe or the scroll wheel to its Play.
    fn featured_shelf(&self, key: ShelfKey, games: &[&GameObject]) -> Shelf {
        let carousel = adw::Carousel::builder()
            .spacing(24)
            .margin_start(48)
            .margin_end(48)
            .margin_top(20)
            .css_classes(["home-featured"])
            .build();
        let dots = adw::CarouselIndicatorDots::builder()
            .carousel(&carousel)
            .margin_top(12)
            .visible(games.len() > 1)
            .build();
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .accessible_role(gtk::AccessibleRole::Group)
            .build();
        row.update_property(&[gtk::accessible::Property::Label(&gettext("Jump Back In"))]);
        row.append(&carousel);
        row.append(&dots);
        let mut items = Vec::with_capacity(games.len() * 2);
        for &game in games {
            let (page, play, details) = self.featured_page(game);
            carousel.append(&page);
            for button in [&play, &details] {
                button.connect_has_focus_notify(glib::clone!(
                    #[weak(rename_to = home)]
                    self,
                    #[weak]
                    carousel,
                    #[weak]
                    page,
                    #[weak]
                    game,
                    move |button| {
                        if button.has_focus() {
                            home.card_focused(button, &Focused::Game(game));
                            home.show_page(&carousel, &page);
                        }
                    }
                ));
            }
            items.push((play, Focused::Game(game.clone())));
            items.push((details, Focused::Game(game.clone())));
        }
        carousel.connect_page_changed(glib::clone!(
            #[weak(rename_to = home)]
            self,
            move |_, page| home.follow_page(page as usize)
        ));
        Shelf {
            id: RowId::Featured,
            key,
            row,
            items,
            shape: TileShape::Landscape,
            size: (0, FEATURED_HEIGHT),
            pending: RefCell::default(),
        }
    }

    /// A page of the featured row (`.featured-page`): the game's art filling it (its
    /// hero, else its cover), `.featured-scrim` over the art, and at the left
    /// (`.featured-text`) "Jump Back In" (`.home-eyebrow`), the game's logo or else its
    /// title, its details (`.home-details`) where the design asks for them, then Play,
    /// or Stop while it runs (`.featured-play`), and Details (`.featured-details`), which
    /// opens the game page. The page is `FEATURED_HEIGHT` pixels tall, and taller where its
    /// text needs it. Returns the page, Play and Details.
    fn featured_page(&self, game: &GameObject) -> (gtk::Overlay, gtk::Button, gtk::Button) {
        let (title, logo) = {
            let entry = game.entry();
            (entry.game.title.clone(), entry.art.logo.clone())
        };
        let page = gtk::Overlay::builder()
            .overflow(gtk::Overflow::Hidden)
            .hexpand(true)
            .child(&gtk::Box::builder().height_request(FEATURED_HEIGHT).build())
            .css_classes(["featured-page"])
            .build();
        let art = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build();
        page.add_overlay(&art);
        page.add_overlay(
            &gtk::Box::builder()
                .can_target(false)
                .css_classes(["featured-scrim"])
                .build(),
        );
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .margin_start(40)
            .margin_end(40)
            .margin_top(24)
            .margin_bottom(24)
            .css_classes(["featured-text"])
            .build();
        text.append(
            &gtk::Label::builder()
                .label(gettext("Jump Back In"))
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["home-eyebrow"])
                .build(),
        );
        // The logo stays within 360×96 whatever the image's size, as on Home's other
        // layouts; the title stands in for it until it shows, and where there is none.
        let logo_picture = gtk::Picture::builder()
            .visible(false)
            .content_fit(gtk::ContentFit::Contain)
            .can_shrink(true)
            .halign(gtk::Align::Start)
            .width_request(360)
            .height_request(96)
            .css_classes(["home-logo"])
            .build();
        logo_picture.update_property(&[gtk::accessible::Property::Label(&title)]);
        text.append(
            &adw::Clamp::builder()
                .maximum_size(360)
                .tightening_threshold(360)
                .child(
                    &adw::Clamp::builder()
                        .orientation(gtk::Orientation::Vertical)
                        .maximum_size(96)
                        .tightening_threshold(96)
                        .child(&logo_picture)
                        .build(),
                )
                .build(),
        );
        let name = gtk::Label::builder()
            .label(&title)
            .xalign(0.0)
            .wrap(true)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["title-1"])
            .build();
        text.append(&name);
        let facts = self.imp().layout.get().home_details.then(|| {
            let facts = gtk::Box::builder()
                .spacing(12)
                .halign(gtk::Align::Start)
                .css_classes(["home-details"])
                .build();
            put_details(&facts, &game_details(game));
            text.append(&facts);
            facts
        });
        let play = gtk::Button::builder()
            .css_classes(["pill", "suggested-action", "featured-play"])
            .build();
        bind_play_button(&play, game);
        let details = gtk::Button::builder()
            .label(gettext("Details"))
            .action_name("win.show-details")
            .action_target(&game.id().as_str().to_variant())
            .css_classes(["pill", "featured-details"])
            .build();
        // "Details" alone does not say whose.
        details.update_property(&[gtk::accessible::Property::Description(&title)]);
        if let Some(facts) = facts {
            // The playtime and the last time played as they are now.
            for button in [&play, &details] {
                button.connect_has_focus_notify(glib::clone!(
                    #[weak]
                    facts,
                    #[weak]
                    game,
                    move |button| {
                        if button.has_focus() {
                            put_details(&facts, &game_details(&game));
                        }
                    }
                ));
            }
        }
        let buttons = gtk::Box::builder()
            .spacing(12)
            .halign(gtk::Align::Start)
            .build();
        buttons.append(&play);
        buttons.append(&details);
        text.append(&buttons);
        page.add_overlay(&text);
        page.set_measure_overlay(&text, true);
        if let Some(path) = hero_art(game) {
            load_picture(&art, path, |_| {});
        }
        if let Some(path) = logo {
            load_picture(&logo_picture, path, move |logo| {
                logo.set_visible(true);
                name.set_visible(false);
            });
        }
        (page, play, details)
    }

    /// Turns the featured row's `carousel` to `page`, at once while Pindu moves less.
    fn show_page(&self, carousel: &adw::Carousel, page: &gtk::Overlay) {
        let animate = !self.imp().background_context.get().reduced_motion;
        carousel.scroll_to(page, animate);
    }

    /// Follows a page of the featured row turned by a swipe or the scroll wheel: focus in
    /// the row goes to that page's Play.
    fn follow_page(&self, page: usize) {
        let rows = self.imp().shelf_rows.borrow();
        let Some(featured) = rows.iter().find(|shelf| shelf.id == RowId::Featured) else {
            return;
        };
        let focused = featured
            .items
            .iter()
            .position(|(button, _)| button.has_focus());
        let play = match focused {
            Some(focused) if focused / 2 != page => featured.items.get(page * 2),
            _ => None,
        }
        .map(|(play, _)| play.clone());
        drop(rows);
        if let Some(play) = play {
            play.grab_focus();
        }
    }

    /// The Explore layout's row of tiles (`.home-carousel`): a compact tile for each of
    /// `tiles`, the sections with the Categories tile, each its ID, name and count
    /// (`compact_section_tile`), which moves focus to its row, then the Library tile and,
    /// where Steam is present, the Steam Big Picture tile, moved here from the carousel.
    fn tiles_shelf(&self, key: ShelfKey, tiles: &[(SectionId, String, String)]) -> Shelf {
        let imp = self.imp();
        let carousel = gtk::Box::builder()
            .spacing(12)
            .margin_start(48)
            .margin_end(48)
            .margin_top(12)
            .margin_bottom(12)
            .css_classes(["home-carousel"])
            .build();
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_top(4)
            .build();
        row.append(&sideways(&carousel));
        let mut items: Vec<(gtk::Button, Focused)> = Vec::with_capacity(tiles.len() + 2);
        for (id, title, count) in tiles {
            let tile = compact_section_tile(*id, title, count);
            let id = *id;
            tile.connect_has_focus_notify(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |tile| {
                    if tile.has_focus() {
                        page.card_focused(tile, &Focused::Section(id));
                    }
                }
            ));
            carousel.append(&tile);
            items.push((tile, Focused::Section(id)));
        }
        for tile in [&*imp.library_tile, &*imp.big_picture_tile] {
            if let Some(parent) = tile.parent().and_downcast::<gtk::Box>() {
                parent.remove(tile);
            }
            carousel.append(tile);
        }
        items.push((imp.library_tile.get(), Focused::Library));
        items.push((imp.big_picture_tile.get(), Focused::BigPicture));
        Shelf {
            id: RowId::Tiles,
            key,
            row,
            items,
            shape: TileShape::Landscape,
            size: COMPACT_TILE,
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
                // Focus held through the rows loads the art near where it stops.
                if !imp.moving.get() {
                    page.load_shelf_art();
                }
                glib::ControlFlow::Break
            }
        ));
    }

    /// Loads the art of the rows of the Shelves layout that show or are within a page of
    /// showing; the rows further off load theirs once the list scrolls near them, and
    /// once focus rests there when it moved there held.
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
                let jump = shelf.id == RowId::Section(SectionId::RecentlyPlayed);
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
        let ids: Vec<RowId> = rows.iter().map(|shelf| shelf.id).collect();
        let position = imp.shelf_focus.borrow().position(&(), &ids);
        if let Some(shelf) = rows.get(position)
            && let Some(bounds) = shelf.row.compute_bounds(&*imp.shelves)
        {
            imp.shelves_scroller
                .vadjustment()
                .set_value(f64::from(bounds.y()));
        }
    }

    /// An arrow key pressed in the rows moves through them as the controller does
    /// (`navigate`). The keyboard's repeats of a held arrow are dropped while the step
    /// before is still to be drawn, so a step slower than the repeat rate does not queue
    /// up repeats that go on moving focus once the key is let go.
    fn arrow_pressed(
        &self,
        key: gtk::gdk::Key,
        state: gtk::gdk::ModifierType,
    ) -> glib::Propagation {
        let direction = match key {
            gtk::gdk::Key::Up | gtk::gdk::Key::KP_Up => Direction::Up,
            gtk::gdk::Key::Down | gtk::gdk::Key::KP_Down => Direction::Down,
            gtk::gdk::Key::Left | gtk::gdk::Key::KP_Left => Direction::Left,
            gtk::gdk::Key::Right | gtk::gdk::Key::KP_Right => Direction::Right,
            _ => return glib::Propagation::Proceed,
        };
        let imp = self.imp();
        let repeat = imp.held_key.borrow_mut().press(key.into_glib());
        let modified = state.intersects(
            gtk::gdk::ModifierType::CONTROL_MASK
                | gtk::gdk::ModifierType::SHIFT_MASK
                | gtk::gdk::ModifierType::ALT_MASK,
        );
        if modified {
            return glib::Propagation::Proceed;
        }
        if repeat && imp.step_pending.get() {
            return glib::Propagation::Stop;
        }
        if !self.navigate(direction) {
            return glib::Propagation::Proceed;
        }
        if !imp.step_pending.replace(true) {
            // A frame's tick comes before its layout and drawing: the second tick comes
            // once the step has been drawn.
            let ticks = Cell::new(0);
            self.add_tick_callback(move |page, _| {
                ticks.set(ticks.get() + 1);
                if ticks.get() < 2 {
                    return glib::ControlFlow::Continue;
                }
                page.imp().step_pending.set(false);
                glib::ControlFlow::Break
            });
        }
        glib::Propagation::Stop
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

    /// Moves focus along and between the rows of the one-screen layouts. In the featured
    /// row, Left and Right step through each page's Play and Details and turn the page
    /// (`home::featured_step`).
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
        let along = matches!(direction, Direction::Left | Direction::Right);
        let target = if rows[row].id == RowId::Featured && along {
            // Each page holds its Play, then its Details.
            let button = if item % 2 == 0 {
                FeaturedButton::Play
            } else {
                FeaturedButton::Details
            };
            home::featured_step(item / 2, button, direction, lengths[row] / 2).and_then(
                |(page, button)| {
                    let item = page * 2 + usize::from(button == FeaturedButton::Details);
                    shown[row].get(item).map(|(tile, _)| tile.clone())
                },
            )
        } else {
            match home::shelf_step((row, item), direction, &lengths) {
                ShelfStep::Item(item) => shown[row].get(item).map(|(tile, _)| tile.clone()),
                ShelfStep::Row(row) => rows
                    .get(row)
                    .and_then(|shelf| self.remembered_in(shelf))
                    .map(|(tile, _)| tile),
                ShelfStep::Stay => None,
                ShelfStep::Leave => return false,
            }
        };
        drop(shown);
        drop(rows);
        if let Some(target) = target {
            target.grab_focus();
        }
        true
    }

    /// Focuses the row of section `id` in the Shelves layout, on the card that last had
    /// focus there, or for the Categories tile the first category's row; `false` when
    /// Home has no such row.
    fn focus_shelf(&self, id: SectionId) -> bool {
        let imp = self.imp();
        let target = imp
            .shelf_rows
            .borrow()
            .iter()
            .find(|shelf| match id {
                SectionId::Categories => {
                    matches!(shelf.id, RowId::Section(SectionId::Category(_)))
                }
                _ => shelf.id == RowId::Section(id),
            })
            .and_then(|shelf| Some((shelf.id, self.remembered_in(shelf)?)));
        let Some((row, (card, focused))) = target else {
            return false;
        };
        // Remembered outright: a window that is not active does not say its focus moved.
        imp.shelf_focus.borrow_mut().remember((), row);
        card.grab_focus();
        self.scroll_to_shelf();
        self.show_focused(&focused);
        true
    }

    /// Coming down from the top bar's tabs into the one-screen layouts' rows, focuses the
    /// row and the card that last had focus there; `false` in the layouts with screens,
    /// where the nearest tile takes focus.
    pub fn enter_from_top(&self) -> bool {
        if !self.shelves() {
            return false;
        }
        self.focus_remembered();
        self.scroll_to_shelf();
        self.has_focus_within()
    }

    /// Focuses the tile or card that last had focus on the screen shown, wherever it
    /// moved to, or the first one when it is gone, and scrolls its row to it: a card that
    /// has focus already does not say so again.
    pub fn focus_remembered(&self) {
        let imp = self.imp();
        if imp.setup.is_visible() {
            let list = &imp.setup_list;
            if let Some(row) = list
                .row_at_index(imp.setup_focus.get())
                .or_else(|| list.row_at_index(0))
            {
                row.grab_focus();
            }
            return;
        }
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
            let ids: Vec<RowId> = rows.iter().map(|shelf| shelf.id).collect();
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
            Some(SectionId::Categories) => {
                let row = imp.row.borrow();
                let items: Vec<Item> = row.iter().map(|(_, focused)| focused.item()).collect();
                let position = imp
                    .game_focus
                    .borrow()
                    .position(&RowId::Section(SectionId::Categories), &items);
                row.get(position)
                    .map(|(tile, focused)| (tile.clone(), focused.clone()))
            }
            Some(open) => {
                let cards = imp.cards.borrow();
                let items: Vec<Item> = cards
                    .iter()
                    .map(|(_, game)| Item::Game(game.id()))
                    .collect();
                let position = imp
                    .game_focus
                    .borrow()
                    .position(&RowId::Section(open), &items);
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
            // A section's games, or the categories on the Categories screen.
            if matches!(item, Item::Game(_)) || open == SectionId::Categories {
                imp.game_focus
                    .borrow_mut()
                    .remember(RowId::Section(open), item);
            }
        } else if item != Item::BigPicture {
            imp.first_focus.borrow_mut().remember((), item);
        }
        keep_in_view(card, false);
        self.show_focused(focused);
    }

    /// Shows what has focus: a game's hero, logo or title and details, under "Jump Back
    /// In" (`.home-eyebrow`) on a first screen that starts with the games; a section's
    /// name and details, without art, or over its first game's on such a first screen and
    /// on the Categories screen, with its first covers (`.home-preview`) where the design
    /// asks for them; the Categories tile as a section whose first game is its first
    /// category's; the Apps tile's name and how many apps it leads to; the title alone for
    /// the Library and Steam Big Picture tiles. Nothing in the Explore layout, whose rows
    /// say it all themselves.
    fn show_focused(&self, focused: &Focused) {
        let imp = self.imp();
        self.focus_moved(focused.item());
        let layout = imp.layout.get();
        if layout.home == HomeLayout::Explore {
            return;
        }
        let first_screen = self.on_first_screen();
        let categories_screen = self.on_categories_screen();
        let starts_with_games = first_screen && layout.home_start == HomeStart::Games;
        imp.eyebrow
            .set_visible(starts_with_games && matches!(focused, Focused::Game(_)));
        let sections = imp.sections.borrow();
        let on_tile = matches!(focused, Focused::Section(_));
        if matches!(focused, Focused::Section(SectionId::Categories)) {
            self.show_preview(None);
            let art = sections
                .category_games
                .first()
                .and_then(|games| games.first())
                .filter(|_| starts_with_games)
                .and_then(hero_art);
            let details = home::categories_details(&sections.categories);
            drop(sections);
            self.show_info(&gettext("Categories"), &details, art);
            return;
        }
        if matches!(focused, Focused::Section(SectionId::Apps)) {
            let details = home::apps_details(sections.apps);
            drop(sections);
            self.show_preview(None);
            self.show_info(&gettext("Apps"), &details, None);
            return;
        }
        if matches!(focused, Focused::Section(SectionId::GetGames)) {
            drop(sections);
            self.show_preview(None);
            self.show_info(&gettext("Get Games"), &[], None);
            return;
        }
        let section = match focused {
            Focused::Section(id) | Focused::SeeAll(id) => sections.get(*id),
            _ => None,
        };
        self.show_preview(section.filter(|_| {
            (first_screen || categories_screen) && on_tile && layout.home_section_preview
        }));
        match (focused, section) {
            (Focused::Game(game), _) => self.show_game(game),
            (Focused::BigPicture, _) => self.show_title(&gettext("Steam Big Picture")),
            (Focused::Section(_) | Focused::SeeAll(_), Some((section, games))) => {
                let art = games
                    .first()
                    .filter(|_| (starts_with_games || categories_screen) && on_tile)
                    .and_then(hero_art);
                self.show_info(
                    &section_title(section),
                    &home::section_details(section),
                    art,
                );
            }
            (Focused::Library | Focused::Section(_) | Focused::SeeAll(_), _) => {
                self.show_title(&gettext("Library"));
            }
        }
    }

    /// The generation of a new hero: art that loads late for an earlier one is dropped,
    /// and its decode cancelled if it is still under way.
    fn next_hero(&self) -> u64 {
        let imp = self.imp();
        let generation = imp.hero_generation.get() + 1;
        imp.hero_generation.set(generation);
        if let Some(loading) = imp.hero_loading.take() {
            loading.cancel();
        }
        generation
    }

    /// Shows `title` alone: no logo, art or details.
    fn show_title(&self, title: &str) {
        let imp = self.imp();
        self.next_hero();
        self.hide_hero_art();
        imp.logo.set_paintable(gtk::gdk::Paintable::NONE);
        imp.logo.set_visible(false);
        imp.title.set_visible(true);
        imp.title.set_label(title);
        self.fill_details(&[]);
    }

    /// Shows a section's `title` with its `details`, over `art` if it is given some.
    fn show_info(&self, title: &str, details: &[Detail], art: Option<PathBuf>) {
        let imp = self.imp();
        let generation = self.next_hero();
        imp.logo.set_paintable(gtk::gdk::Paintable::NONE);
        imp.logo.set_visible(false);
        imp.title.set_visible(true);
        imp.title.set_label(title);
        self.fill_details(details);
        self.show_art(art, None, generation);
    }

    /// Fills the strip of the focused section's first covers (`.home-preview`,
    /// `home::preview`): a cover each, then `+N` (`.preview-more`) for the games beyond
    /// them. Without a section the strip hides. Art in memory shows at once; other art
    /// loads once focus rests. The strip's covers are made once and refilled.
    fn show_preview(&self, section: Option<(&Section, &[GameObject])>) {
        let imp = self.imp();
        let strip = &imp.preview;
        let (width, height) = PREVIEW_COVER;
        let (covers, more) = imp.preview_items.get_or_init(|| {
            let covers: Vec<CoverArt> = (0..home::PREVIEW_SIZE)
                .map(|_| {
                    let cover = CoverArt::new();
                    cover.set_size_request(width, height);
                    strip.append(&cover);
                    cover
                })
                .collect();
            let more = gtk::Label::builder()
                .width_request(width)
                .height_request(height)
                .css_classes(["preview-more"])
                .build();
            strip.append(&more);
            (covers, more)
        });
        let mut waiting = imp.waiting.borrow_mut();
        waiting.covers.clear();
        let Some((section, games)) = section else {
            strip.set_visible(false);
            return;
        };
        let preview = home::preview(section);
        let scale = self.scale_factor().max(1);
        let pixels = ((width * scale) as u32, (height * scale) as u32);
        let shown = preview
            .games
            .iter()
            .filter_map(|id| games.iter().find(|game| game.id() == *id));
        let mut covers = covers.iter();
        for (game, cover) in shown.zip(covers.by_ref()) {
            let entry = game.entry();
            let art = home::card_art(TileShape::Portrait, &entry.art);
            cover.set_alternative_text(&entry.game.title);
            cover.set_crop(art.crop);
            cover.set_visible(true);
            match art.art {
                Some(path) if textures::cached_cover(&path, pixels.0, pixels.1).is_none() => {
                    cover.clear();
                    waiting.covers.push((cover.clone(), path, pixels));
                }
                art => cover.show(art, None, pixels, |_| {}),
            }
        }
        for cover in covers {
            cover.clear();
            cover.set_visible(false);
        }
        if preview.more > 0 {
            // Translators: how many more games a section holds than the covers shown
            // beside this, as in "+6".
            more.set_label(&gettext("+{count}").replace("{count}", &preview.more.to_string()));
        }
        more.set_visible(preview.more > 0);
        strip.set_visible(true);
    }

    /// Fills the `.home-details` row with `details` as `.detail` items, while the design
    /// asks for them; the row hides without any.
    fn fill_details(&self, details: &[Detail]) {
        let imp = self.imp();
        let shown = if imp.layout.get().home_details {
            details
        } else {
            &[]
        };
        put_details(&imp.details, shown);
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
        let generation = self.next_hero();
        imp.logo.set_paintable(gtk::gdk::Paintable::NONE);
        imp.logo.set_visible(false);
        imp.title.set_visible(true);
        self.fill_details(&game_details(game));
        let (title, logo) = {
            let entry = game.entry();
            (entry.game.title.clone(), entry.art.logo.clone())
        };
        imp.title.set_label(&title);
        imp.logo
            .update_property(&[gtk::accessible::Property::Label(&title)]);
        self.show_art(hero_art(game), logo, generation);
    }

    /// Shows `art` as the hero and `logo` in the title's place, for the hero of
    /// `generation`. The page carries `.no-art` while no hero art shows. Art in memory
    /// shows at once, so moving along the carousel does not flicker, unless focus is held
    /// moving through the items; other art loads once focus rests (`FOCUS_REST`) and
    /// replaces the hero shown once it has, so a design fades straight from one to the
    /// other.
    fn show_art(&self, art: Option<PathBuf>, logo: Option<PathBuf>, generation: u64) {
        let imp = self.imp();
        let art = match art {
            None => {
                self.hide_hero_art();
                None
            }
            Some(path) if imp.moving.get() => Some(path),
            Some(path) => match self.cached_hero(&path) {
                Some(texture) => {
                    self.show_hero_art(&texture);
                    None
                }
                None => Some(path),
            },
        };
        let logo = logo.filter(|path| match textures::cached(path) {
            Some(texture) => {
                self.show_logo(&texture);
                false
            }
            None => true,
        });
        imp.waiting.borrow_mut().hero =
            (art.is_some() || logo.is_some()).then_some((art, logo, generation));
    }

    /// Notes that `item` has focus and waits for focus to rest there (`FOCUS_REST`): what
    /// has to load waits until then. Focus that moves to another item before it rests is
    /// held moving through the items (`moving`).
    fn focus_moved(&self, item: Item) {
        let imp = self.imp();
        let moved = imp.rest_item.replace(Some(item.clone())) != Some(item);
        if let Some(source) = imp.rest.take() {
            source.remove();
            if moved {
                imp.moving.set(true);
            }
        }
        let page = self.downgrade();
        let source = glib::timeout_add_local_once(FOCUS_REST, move || {
            if let Some(page) = page.upgrade() {
                // This timer has fired.
                page.imp().rest.take();
                page.rested();
            }
        });
        imp.rest.replace(Some(source));
    }

    /// Focus rests: loads the hero, logo and preview covers that waited for it, and the
    /// art of the rows near it in the Shelves layout.
    fn rested(&self) {
        let imp = self.imp();
        let moved = imp.moving.replace(false);
        let waiting = imp.waiting.take();
        if let Some((art, logo, generation)) = waiting.hero
            && generation == imp.hero_generation.get()
        {
            self.load_art(art, logo, generation);
        }
        for (cover, path, pixels) in waiting.covers {
            cover.show(Some(path), None, pixels, |_| {});
        }
        if moved {
            self.load_shelf_art();
        }
    }

    /// Loads `art` as the hero, sized for the screen, and `logo` in the title's place, for
    /// the hero of `generation`; art already in memory shows at once. Art that loads once
    /// another hero was asked for is dropped, and its decode cancelled.
    fn load_art(&self, art: Option<PathBuf>, logo: Option<PathBuf>, generation: u64) {
        let art = art.filter(|path| match self.cached_hero(path) {
            Some(texture) => {
                self.show_hero_art(&texture);
                false
            }
            None => true,
        });
        if art.is_none() && logo.is_none() {
            return;
        }
        let size = self.hero_pixels();
        let cancellable = gio::Cancellable::new();
        self.imp().hero_loading.replace(Some(cancellable.clone()));
        let page = self.downgrade();
        let current = move |page: &glib::WeakRef<Self>| {
            page.upgrade()
                .filter(|page| page.imp().hero_generation.get() == generation)
        };
        glib::spawn_future_local(async move {
            if let Some(path) = art {
                let texture = match size {
                    Some((width, height)) => {
                        textures::load_hero(&path, width, height, &cancellable).await
                    }
                    None => textures::load(&path).await,
                };
                if let Some(page) = current(&page) {
                    match texture {
                        Ok(texture) => page.show_hero_art(&texture),
                        Err(_) => page.hide_hero_art(),
                    }
                }
            }
            if let Some(path) = logo
                && current(&page).is_some()
                && let Ok(texture) = textures::load(&path).await
                && let Some(page) = current(&page)
            {
                page.show_logo(&texture);
            }
        });
    }

    /// The hero art at `path` if it is in memory, at the screen's size or at its own.
    fn cached_hero(&self, path: &Path) -> Option<gtk::gdk::Texture> {
        self.hero_pixels()
            .and_then(|(width, height)| textures::cached_cover(path, width, height))
            .or_else(|| textures::cached(path))
    }

    /// The size in device pixels hero art is decoded at: the screen's, which the hero
    /// covers. `None` before the window shows.
    fn hero_pixels(&self) -> Option<(u32, u32)> {
        let (width, height) = self.screen_pixels()?;
        Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?))
    }

    fn show_logo(&self, texture: &gtk::gdk::Texture) {
        let imp = self.imp();
        imp.logo.set_paintable(Some(texture));
        imp.logo.set_visible(true);
        imp.title.set_visible(false);
    }
}

/// The art that fills the page behind a game: its hero, or else its cover.
pub(crate) fn hero_art(game: &GameObject) -> Option<PathBuf> {
    let entry = game.entry();
    entry.art.hero.clone().or_else(|| entry.art.cover.clone())
}

/// What Home's details row says of `game` (`home::details`), as of now.
fn game_details(game: &GameObject) -> Vec<Detail> {
    let now = glib::DateTime::now_local().ok();
    let (now, offset) = now.map_or((0, 0), |now| (now.to_unix(), now.utc_offset().as_seconds()));
    let entry = game.entry();
    home::details(&entry.game, entry.playtime_secs, now, offset)
}

/// Shows the image at `path` in `picture`: at once when it is in memory, else once it has
/// loaded, and then runs `loaded`. An image that does not load leaves the picture empty.
pub(crate) fn load_picture(
    picture: &gtk::Picture,
    path: PathBuf,
    loaded: impl FnOnce(&gtk::Picture) + 'static,
) {
    if let Some(texture) = textures::cached(&path) {
        picture.set_paintable(Some(&texture));
        loaded(picture);
        return;
    }
    let picture = picture.downgrade();
    glib::spawn_future_local(async move {
        if let Ok(texture) = textures::load(&path).await
            && let Some(picture) = picture.upgrade()
        {
            picture.set_paintable(Some(&texture));
            loaded(&picture);
        }
    });
}

/// A row's frame that scrolls `carousel` sideways; the page scrolls it itself, keeping
/// the lifted card's ring in view.
fn sideways(carousel: &gtk::Box) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(carousel)
        .build();
    if let Some(viewport) = carousel.parent().and_downcast::<gtk::Viewport>() {
        viewport.set_scroll_to_focus(false);
    }
    scroller
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

/// Shows a card's art at `size`, decoded for a design that scales the card up, and names
/// the card `.no-art` when it has none.
fn show_card_art(picture: &CoverArt, art: CardArt, size: (i32, i32), scale_factor: i32) {
    picture.set_crop(art.crop);
    picture.show(
        art.art,
        None,
        home::art_pixels(size, scale_factor),
        show_art_class,
    );
}
