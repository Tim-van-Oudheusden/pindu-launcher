//! Get Games' catalogue page (#887; `docs/get-games.md` section 8.1): Flathub's games and
//! a few extras (`pindu_core::catalogue::Offer`) as shelves of cards, picks first; search
//! results as a grid; or a status page that says why there is nothing to show. A card is
//! an app's icon and name, marked once installed or while a job for it is queued or runs;
//! A or a click opens its page (`win.get-games-app`). The shoulder buttons jump between
//! shelves in fullscreen mode.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::catalogue::{App, GameCategory, Offer, Shelf};
use pindu_core::get_games::CatalogueStatus;

use crate::home_page::GET_GAMES_ICON;
use crate::widgets::textures;

/// The side of an app's icon on its card, in pixels.
const ICON_SIZE: i32 = 96;

/// How wide an app's name may be on its card, in characters, before it wraps.
const NAME_CHARS: i32 = 14;

/// The room left around a shelf's cards, inside the scroller that clips them, for a
/// focused card, which grows by 5% and has an outline outside it (#919).
const FOCUS_ROOM: i32 = 12;

/// What the page shows of the catalogue.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    /// Whether there is a catalogue, one is on its way, or why not.
    pub status: CatalogueStatus,
    /// Whether the catalogue is there but could not be read.
    pub broken: bool,
    /// Whether it is being read.
    pub loading: bool,
    /// What it offers.
    pub offer: &'a Offer,
    /// Its folder, which the apps' icons are under.
    pub folder: Option<&'a Path>,
    /// The apps installed.
    pub installed: &'a HashSet<String>,
    /// The apps a job is queued or runs for.
    pub busy: &'a HashSet<String>,
}

/// What a page shows, to rebuild it only when that changes: the stack's page and each
/// row's apps with their marks.
type Shape = (String, Vec<(String, Vec<(String, bool, bool)>)>);

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/get_games_page.ui")]
    pub struct GetGamesPage {
        #[template_child]
        pub toolbar: TemplateChild<adw::ToolbarView>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub sidebar_toggle: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub shelves: TemplateChild<gtk::Box>,
        #[template_child]
        pub results: TemplateChild<gtk::FlowBox>,
        #[template_child]
        pub status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub status_button: TemplateChild<gtk::Button>,
        /// What the page shows now.
        pub shape: RefCell<Shape>,
        /// Each shelf's row of cards, top to bottom, for the shoulder buttons.
        pub rows: RefCell<Vec<gtk::Box>>,
        /// The page opened with nothing to focus yet: its first card takes focus once
        /// there is one.
        pub focus_pending: std::cell::Cell<bool>,
        /// The app whose page opened last.
        pub last_app: RefCell<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GetGamesPage {
        const NAME: &'static str = "PinduGetGamesPage";
        type Type = super::GetGamesPage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for GetGamesPage {}
    impl WidgetImpl for GetGamesPage {}
    impl NavigationPageImpl for GetGamesPage {
        fn shown(&self) {
            self.parent_shown();
            // Back from an app's page, whose install or removal built the cards again,
            // focus returns to that app's new card rather than to the search field.
            self.obj().focus_remembered();
        }
    }
}

glib::wrapper! {
    /// Get Games' catalogue page.
    pub struct GetGamesPage(ObjectSubclass<imp::GetGamesPage>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for GetGamesPage {
    fn default() -> Self {
        Self::new()
    }
}

impl GetGamesPage {
    /// An empty catalogue page.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The page's search field, which the search keyboard types into.
    pub fn search_entry(&self) -> gtk::SearchEntry {
        self.imp().search_entry.get()
    }

    /// The header bar's button that shows and hides the desktop window's sidebar.
    pub fn sidebar_toggle(&self) -> gtk::ToggleButton {
        self.imp().sidebar_toggle.get()
    }

    /// Shows the page as fullscreen mode does, without its header bar and with its title
    /// in the page, or as the desktop window does.
    pub fn set_fullscreen(&self, fullscreen: bool) {
        let imp = self.imp();
        imp.toolbar.set_reveal_top_bars(!fullscreen);
        imp.title.set_visible(fullscreen);
    }

    /// Shows `view`: the catalogue's shelves, the results of the search typed, or why
    /// there is nothing to show. Cards are built again only when what they show changed;
    /// focus stays on the app it was on.
    pub fn show(&self, view: View<'_>) {
        let imp = self.imp();
        let query = imp.search_entry.text().trim().to_owned();
        let page = if view.loading && view.offer.apps().is_empty() {
            "loading"
        } else if view.status != CatalogueStatus::Ready || view.broken {
            "status"
        } else if !query.is_empty() {
            "results"
        } else {
            "shelves"
        };
        let mark = |app: &App| {
            (
                app.id.clone(),
                view.installed.contains(&app.id),
                view.busy.contains(&app.id),
            )
        };
        let rows: Vec<(String, Vec<&App>)> = match page {
            "results" => {
                let found = view.offer.search(&query);
                if found.is_empty() {
                    Vec::new()
                } else {
                    vec![(String::new(), found)]
                }
            }
            "shelves" => view
                .offer
                .shelves()
                .into_iter()
                .map(|(shelf, apps)| (shelf_title(shelf), apps))
                .collect(),
            _ => Vec::new(),
        };
        let page = if page == "results" && rows.is_empty() {
            "status"
        } else {
            page
        };
        if page == "status" {
            self.show_status(view, &query);
        }
        let shape: Shape = (
            page.to_owned(),
            rows.iter()
                .map(|(title, apps)| (title.clone(), apps.iter().map(|app| mark(app)).collect()))
                .collect(),
        );
        let changed = *imp.shape.borrow() != shape;
        if changed {
            let focused = self.focused_app();
            let folder = view.folder.map(Path::to_owned);
            match page {
                "shelves" => self.build_shelves(&rows, folder.as_deref(), view),
                "results" => self.build_results(&rows[0].1, folder.as_deref(), view),
                _ => {}
            }
            imp.shape.replace(shape);
            if let Some(id) = focused {
                self.focus_app(&id);
            }
        }
        imp.stack.set_visible_child_name(page);
        if imp.focus_pending.get() {
            // Cards just built take focus once they are on screen.
            self.focus_when_drawn();
        }
    }

    /// Says why the page has nothing to show.
    fn show_status(&self, view: View<'_>, query: &str) {
        let imp = self.imp();
        let (icon, title, description, retry) = if view.status == CatalogueStatus::Ready
            && !view.broken
        {
            (
                "edit-find-symbolic",
                gettext("No Results"),
                // Translators: {query} is what was typed into Get Games' search field.
                gettext("No game or app on Flathub matches “{query}”.").replace("{query}", query),
                false,
            )
        } else if view.broken {
            (
                "dialog-warning-symbolic",
                gettext("The Game Catalogue Could Not Be Read"),
                gettext("Download it again to fix it."),
                true,
            )
        } else {
            match view.status {
                CatalogueStatus::Waiting => (
                    "folder-download-symbolic",
                    gettext("Getting the Game Catalogue"),
                    gettext("Downloading Flathub's list of games. The first time takes a minute."),
                    false,
                ),
                CatalogueStatus::Offline => (
                    "network-offline-symbolic",
                    gettext("Offline"),
                    gettext(
                        "Get Games needs the network to download Flathub's list of games the \
                         first time. Connect to a network, then try again.",
                    ),
                    true,
                ),
                CatalogueStatus::Unavailable | CatalogueStatus::Ready => (
                    "dialog-warning-symbolic",
                    gettext("No Game Catalogue"),
                    gettext("Flathub's list of games could not be downloaded."),
                    true,
                ),
            }
        };
        imp.status.set_icon_name(Some(icon));
        imp.status.set_title(&title);
        imp.status.set_description(Some(&description));
        imp.status_button.set_visible(retry);
    }

    fn build_shelves(&self, rows: &[(String, Vec<&App>)], folder: Option<&Path>, view: View<'_>) {
        let imp = self.imp();
        while let Some(child) = imp.shelves.first_child() {
            imp.shelves.remove(&child);
        }
        let mut cards = Vec::new();
        for (title, apps) in rows {
            let shelf = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(12)
                .css_classes(["get-games-shelf"])
                .build();
            shelf.append(
                &gtk::Label::builder()
                    .label(title)
                    .xalign(0.0)
                    .margin_start(FOCUS_ROOM)
                    .css_classes(["heading"])
                    .build(),
            );
            let row = gtk::Box::builder()
                .spacing(12)
                .margin_top(FOCUS_ROOM)
                .margin_bottom(FOCUS_ROOM)
                .margin_start(FOCUS_ROOM)
                .margin_end(FOCUS_ROOM)
                .build();
            row.update_property(&[gtk::accessible::Property::Label(title)]);
            for app in apps {
                row.append(&card(app, folder, view));
            }
            shelf.append(
                &gtk::ScrolledWindow::builder()
                    .vscrollbar_policy(gtk::PolicyType::Never)
                    .hscrollbar_policy(gtk::PolicyType::External)
                    // As tall as the row wants: its minimum leaves out a second line of a
                    // card's name, and the room around a focused card with it.
                    .propagate_natural_height(true)
                    .child(&row)
                    .build(),
            );
            imp.shelves.append(&shelf);
            cards.push(row);
        }
        imp.rows.replace(cards);
    }

    fn build_results(&self, apps: &[&App], folder: Option<&Path>, view: View<'_>) {
        let imp = self.imp();
        imp.results.remove_all();
        for app in apps {
            // The card takes focus and shows it; the grid's cell around it would take it
            // first and show only a thin line (#919).
            let child = gtk::FlowBoxChild::builder()
                .focusable(false)
                .child(&card(app, folder, view))
                .build();
            imp.results.append(&child);
        }
        imp.rows.replace(Vec::new());
    }

    /// The app whose card has focus.
    fn focused_app(&self) -> Option<String> {
        let focus = self.root()?.focus()?;
        let card = focus
            .ancestor(gtk::Button::static_type())
            .or(Some(focus))
            .and_downcast::<gtk::Button>()?;
        card.action_target_value()?.str().map(str::to_owned)
    }

    /// The card of the app `id` on the stack page that shows.
    fn card_of(&self, id: &str) -> Option<gtk::Button> {
        let root = self.imp().stack.visible_child()?;
        let mut widget = root.first_child();
        while let Some(current) = widget {
            if let Some(button) = current.downcast_ref::<gtk::Button>()
                && button
                    .action_target_value()
                    .and_then(|target| target.str().map(str::to_owned))
                    .as_deref()
                    == Some(id)
            {
                return Some(button.clone());
            }
            widget = next_widget(&current, &root);
        }
        None
    }

    /// Focuses the card of the app `id`; `false` without one.
    fn focus_app(&self, id: &str) -> bool {
        self.card_of(id).is_some_and(|card| card.grab_focus())
    }

    /// Focuses the card of the app whose page opened last, or the first card, now or once
    /// there is one.
    pub fn focus_first_when_ready(&self) {
        let pending = !self.focus_remembered();
        self.imp().focus_pending.set(pending);
        if pending {
            self.focus_when_drawn();
        }
    }

    /// Gives focus as [`focus_first_when_ready`](Self::focus_first_when_ready) asked, once
    /// the page is drawn.
    fn focus_when_drawn(&self) {
        glib::idle_add_local_once(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move || {
                if page.imp().focus_pending.get() && page.focus_remembered() {
                    page.imp().focus_pending.set(false);
                }
            }
        ));
    }

    /// Notes the app whose page opened, whose card focus comes back to.
    pub fn remember_app(&self, id: &str) {
        self.imp().last_app.replace(Some(id.to_owned()));
    }

    /// Focuses the card of the app whose page opened last, or else the first card; `false`
    /// while another page shows above this one, which keeps its focus.
    fn focus_remembered(&self) -> bool {
        let shown = self
            .ancestor(adw::NavigationView::static_type())
            .and_downcast::<adw::NavigationView>()
            .and_then(|view| view.visible_page())
            .is_some_and(|page| page == *self.upcast_ref::<adw::NavigationPage>());
        if !shown {
            return false;
        }
        let last = self.imp().last_app.borrow().clone();
        match last.and_then(|id| self.card_of(&id)) {
            Some(card) => card.grab_focus(),
            None => self.focus_first(),
        }
    }

    /// Focuses the first card of the page, or the status page's button; `false` when
    /// nothing takes focus.
    pub fn focus_first(&self) -> bool {
        let imp = self.imp();
        match imp.stack.visible_child_name().as_deref() {
            Some("shelves") => imp
                .rows
                .borrow()
                .first()
                .and_then(gtk::Box::first_child)
                .is_some_and(|card| card.grab_focus()),
            Some("results") => imp
                .results
                .child_at_index(0)
                .is_some_and(|child| child.child_focus(gtk::DirectionType::TabForward)),
            Some("status") => imp.status_button.is_visible() && imp.status_button.grab_focus(),
            _ => false,
        }
    }

    /// Moves focus to the first card of the shelf after (`forward`) or before the one
    /// that has focus, as the shoulder buttons do; `false` when there is none that way.
    pub fn jump_shelf(&self, forward: bool) -> bool {
        let rows = self.imp().rows.borrow();
        let focus = self.root().and_then(|root| root.focus());
        let current = focus.as_ref().and_then(|focus| {
            rows.iter()
                .position(|row| focus.is_ancestor(row) || focus == row.upcast_ref::<gtk::Widget>())
        });
        let target = match (current, forward) {
            (Some(index), true) => index + 1,
            (Some(index), false) => match index.checked_sub(1) {
                Some(index) => index,
                None => return false,
            },
            (None, _) => 0,
        };
        rows.get(target)
            .and_then(gtk::Box::first_child)
            .is_some_and(|card| card.grab_focus())
    }

    /// Whether the page has shelves for the shoulder buttons to jump between.
    pub fn has_shelves(&self) -> bool {
        self.imp().stack.visible_child_name().as_deref() == Some("shelves")
    }
}

/// The widget after `widget` in a depth-first walk of `root`'s children.
fn next_widget(widget: &gtk::Widget, root: &gtk::Widget) -> Option<gtk::Widget> {
    if let Some(child) = widget.first_child() {
        return Some(child);
    }
    let mut current = widget.clone();
    loop {
        if current == *root {
            return None;
        }
        if let Some(sibling) = current.next_sibling() {
            return Some(sibling);
        }
        current = current.parent()?;
    }
}

/// A shelf's title.
pub fn shelf_title(shelf: Shelf) -> String {
    match shelf {
        Shelf::Picks => gettext("Picks"),
        Shelf::Launchers => gettext("Launchers"),
        Shelf::Category(category) => category_title(category),
        Shelf::More => gettext("More Games"),
        Shelf::Extras => gettext("Extras"),
    }
}

/// A category shelf's title.
fn category_title(category: GameCategory) -> String {
    match category {
        GameCategory::Action => gettext("Action"),
        GameCategory::Adventure => gettext("Adventure"),
        GameCategory::Arcade => gettext("Arcade"),
        GameCategory::Board => gettext("Board Games"),
        GameCategory::Blocks => gettext("Falling Blocks"),
        GameCategory::Card => gettext("Card Games"),
        GameCategory::Kids => gettext("For Kids"),
        GameCategory::Logic => gettext("Puzzles"),
        GameCategory::RolePlaying => gettext("Role Playing"),
        GameCategory::Shooter => gettext("Shooters"),
        GameCategory::Simulation => gettext("Simulation"),
        GameCategory::Sports => gettext("Sports"),
        GameCategory::Strategy => gettext("Strategy"),
        GameCategory::Emulator => gettext("Emulators"),
    }
}

/// An app's card (`.get-games-card`): its icon, or a generic one until it loads or where
/// it has none, over its name, marked Installed or Waiting; it opens the app's page.
fn card(app: &App, folder: Option<&Path>, view: View<'_>) -> gtk::Button {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(6)
        .margin_end(6)
        .build();
    let icon = gtk::Image::builder()
        .icon_name(GET_GAMES_ICON)
        .pixel_size(ICON_SIZE)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    if let Some(path) = icon_path(app, folder) {
        load_icon(&icon, path);
    }
    content.append(&icon);
    content.append(
        &gtk::Label::builder()
            .label(&app.name)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .justify(gtk::Justification::Center)
            .max_width_chars(NAME_CHARS)
            .width_chars(NAME_CHARS)
            .build(),
    );
    let installed = view.installed.contains(&app.id);
    let busy = view.busy.contains(&app.id);
    let mark = if busy {
        Some(gettext("In Progress"))
    } else if installed {
        Some(gettext("Installed"))
    } else {
        None
    };
    content.append(
        &gtk::Label::builder()
            .label(mark.as_deref().unwrap_or(" "))
            .css_classes(["caption", "dim-label"])
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build(),
    );
    let card = gtk::Button::builder()
        .child(&content)
        .tooltip_text(&app.name)
        .action_name("win.get-games-app")
        .action_target(&app.id.to_variant())
        .width_request(168)
        .css_classes(["card", "get-games-card"])
        .build();
    let label = match &mark {
        // Translators: an app's card in Get Games, such as "SuperTuxKart, Installed".
        Some(mark) => gettext("{name}, {mark}")
            .replace("{name}", &app.name)
            .replace("{mark}", mark),
        None => app.name.clone(),
    };
    card.update_property(&[
        gtk::accessible::Property::Label(&label),
        gtk::accessible::Property::Description(&app.summary),
    ]);
    card.connect_has_focus_notify(|card| {
        if card.has_focus() {
            // After the scrollers' own scrolling to the focused card, which stops at its
            // edges.
            glib::idle_add_local_once(glib::clone!(
                #[weak]
                card,
                move || keep_room_around(&card)
            ));
        }
    });
    card
}

/// Scrolls each scroller around `card`, its shelf's and the page's, so it shows with the
/// room its focus highlight takes around it (#919).
fn keep_room_around(card: &gtk::Button) {
    let mut ancestor = card.parent();
    while let Some(widget) = ancestor {
        if let Some(viewport) = widget.downcast_ref::<gtk::Viewport>()
            && let Some(bounds) = card.compute_bounds(viewport)
        {
            if let Some(adjustment) = viewport.hadjustment() {
                reveal(&adjustment, bounds.x(), bounds.width());
            }
            if let Some(adjustment) = viewport.vadjustment() {
                reveal(&adjustment, bounds.y(), bounds.height());
            }
        }
        ancestor = widget.parent();
    }
}

/// Moves `adjustment` so the span of `size` at `start`, in its viewport's coordinates,
/// shows with [`FOCUS_ROOM`] on both sides, as far as it can scroll.
fn reveal(adjustment: &gtk::Adjustment, start: f32, size: f32) {
    let room = f64::from(FOCUS_ROOM);
    let value = adjustment.value();
    let first = value + f64::from(start) - room;
    let last = value + f64::from(start + size) + room;
    if first < value {
        adjustment.set_value(first);
    } else if last > value + adjustment.page_size() {
        adjustment.set_value(last - adjustment.page_size());
    }
}

/// Where the app's icon is, under the catalogue's `folder`.
pub fn icon_path(app: &App, folder: Option<&Path>) -> Option<PathBuf> {
    Some(folder?.join(app.icon.as_ref()?))
}

/// Loads the icon at `path` into `icon` once it is read.
pub fn load_icon(icon: &gtk::Image, path: PathBuf) {
    let icon = icon.downgrade();
    glib::spawn_future_local(async move {
        if let Ok(texture) = textures::load(&path).await
            && let Some(icon) = icon.upgrade()
        {
            icon.set_paintable(Some(&texture));
        }
    });
}
