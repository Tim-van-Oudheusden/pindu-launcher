//! Change Artwork: a game's art from SteamGridDB, first, or from a file, in each kind of
//! art (`pindu_core::art_picker::KINDS`). The desktop window lists the kinds down the side
//! and pages SteamGridDB's images with arrows; fullscreen mode shows the kinds as tabs the
//! shoulder buttons switch and loads the next page once focus reaches the last two rows
//! (`art_picker::loads_more`) or the grid is scrolled to its end. A picked image becomes
//! the game's own art at once. A game SteamGridDB does not know by Steam app ID is found
//! by its title; the user can choose another match, which the library remembers. The
//! Edit Game dialog opens it for one kind, and gets the picked image back instead.

use std::cell::{Cell, RefCell};
use std::fmt;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::art_picker::{self, KINDS};
use pindu_core::model::{ArtLayout, GameId};
use pindu_core::steamgriddb::{GameRef, Image, SearchHit, matching_hit};

use crate::file_service::{self, PickedImage};
use crate::library_service::LibraryService;
use crate::models::GameObject;
use crate::steamgriddb_service::{self, SteamGridDb, steam_app_id};
use crate::widgets::textures;

/// Where to get an API key.
const KEY_PAGE: &str = "https://www.steamgriddb.com/profile/preferences/api";

/// The game to find art for.
#[derive(Debug, Clone)]
pub struct PickerGame {
    /// The game, when it is in the library already; its SteamGridDB match is remembered.
    pub id: Option<GameId>,
    /// The game's Steam app ID, for Steam games.
    pub steam_app_id: Option<u32>,
    /// The game's title.
    pub title: String,
}

impl PickerGame {
    /// The library game `game`.
    pub fn of(game: &GameObject) -> Self {
        let game = game.game();
        Self {
            id: Some(game.id.clone()),
            steam_app_id: steam_app_id(&game.id),
            title: game.title.clone(),
        }
    }
}

/// What a picked image is for.
#[derive(Default)]
enum Purpose {
    /// Nothing yet.
    #[default]
    None,
    /// The game's own art, set at once.
    Apply(GameObject),
    /// Handed to the caller, which closes the dialog.
    Pick(Box<dyn Fn(PickedImage)>),
}

impl fmt::Debug for Purpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::None => "None",
            Self::Apply(_) => "Apply",
            Self::Pick(_) => "Pick",
        })
    }
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/artwork_dialog.ui")]
    pub struct ArtworkDialog {
        #[template_child]
        pub toasts: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub file_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub reset_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub kind_tabs: TemplateChild<gtk::Box>,
        #[template_child]
        pub kind_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub kind_separator: TemplateChild<gtk::Separator>,
        #[template_child]
        pub match_bar: TemplateChild<gtk::Box>,
        #[template_child]
        pub match_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub change_match: TemplateChild<gtk::Button>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub images: TemplateChild<gtk::FlowBox>,
        #[template_child]
        pub status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub match_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub match_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub pager: TemplateChild<gtk::CenterBox>,
        #[template_child]
        pub previous_page: TemplateChild<gtk::Button>,
        #[template_child]
        pub page_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub next_page: TemplateChild<gtk::Button>,
        pub(super) purpose: RefCell<Purpose>,
        pub library: RefCell<Option<LibraryService>>,
        pub game: RefCell<Option<PickerGame>>,
        /// Fullscreen mode's tabs and progressive loading, rather than the list and arrows.
        pub fullscreen: Cell<bool>,
        pub kind: Cell<Option<ArtLayout>>,
        pub client: RefCell<Option<SteamGridDb>>,
        pub game_ref: RefCell<Option<GameRef>>,
        /// The shown page, and the page after the loaded ones.
        pub page: Cell<u32>,
        pub next: Cell<Option<u32>>,
        /// The images shown, in the grid's order.
        pub shown: RefCell<Vec<Image>>,
        /// Bumped whenever what is shown changes, so a late answer is dropped.
        pub generation: Cell<u64>,
        pub loading_more: Cell<bool>,
        /// The kinds the user gave the game its own art in.
        pub user_art: RefCell<Vec<ArtLayout>>,
        pub hits: RefCell<Vec<SearchHit>>,
        pub tabs: RefCell<Vec<(ArtLayout, gtk::ToggleButton)>>,
        pub rows: RefCell<Vec<(ArtLayout, adw::ActionRow, gtk::Image)>>,
        pub art_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ArtworkDialog {
        const NAME: &'static str = "PinduArtworkDialog";
        type Type = super::ArtworkDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ArtworkDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let dialog = self.obj();
            self.scroller.connect_edge_reached(glib::clone!(
                #[weak]
                dialog,
                move |scroller, edge| {
                    // Only a grid the user scrolled down to its end loads more; one being
                    // laid out, or growing with the dialog, reaches it without them.
                    let height = scroller.vadjustment();
                    if edge == gtk::PositionType::Bottom
                        && dialog.imp().fullscreen.get()
                        && height.value() > 0.0
                        && height.upper() > height.page_size() + 1.0
                    {
                        dialog.load_more();
                    }
                }
            ));
            // The arrows page the images, so they go with them.
            self.stack.connect_visible_child_name_notify(glib::clone!(
                #[weak]
                dialog,
                move |stack| {
                    let imp = dialog.imp();
                    imp.pager.set_visible(
                        !imp.fullscreen.get()
                            && stack.visible_child_name().as_deref() == Some("images"),
                    );
                }
            ));
        }

        fn dispose(&self) {
            if let (Purpose::Apply(game), Some(handler)) =
                (&*self.purpose.borrow(), self.art_handler.take())
            {
                game.disconnect(handler);
            }
        }
    }

    impl WidgetImpl for ArtworkDialog {}

    impl AdwDialogImpl for ArtworkDialog {
        fn closed(&self) {
            // Late answers find nothing to fill.
            self.generation.set(self.generation.get() + 1);
            self.parent_closed();
        }
    }

    #[gtk::template_callbacks]
    impl ArtworkDialog {
        #[template_callback]
        fn choose_file(&self) {
            self.obj().choose_file();
        }

        #[template_callback]
        fn reset(&self) {
            self.obj().reset();
        }

        #[template_callback]
        fn kind_row_selected(&self, row: Option<&gtk::ListBoxRow>) {
            let Some(row) = row else { return };
            let kind = self
                .rows
                .borrow()
                .iter()
                .find(|(_, candidate, _)| candidate.upcast_ref::<gtk::ListBoxRow>() == row)
                .map(|(kind, _, _)| *kind);
            if let Some(kind) = kind {
                self.obj().show_kind(kind);
            }
        }

        #[template_callback]
        fn image_activated(&self, child: &gtk::FlowBoxChild) {
            self.obj().use_image(child.index());
        }

        #[template_callback]
        fn show_matches(&self) {
            self.obj().show_matches();
        }

        #[template_callback]
        fn search_matches(&self) {
            self.obj().search_matches();
        }

        #[template_callback]
        fn match_activated(&self, row: &gtk::ListBoxRow) {
            let hit = self.hits.borrow().get(row.index() as usize).cloned();
            if let Some(hit) = hit {
                self.obj().choose_match(hit);
            }
        }

        #[template_callback]
        fn show_previous_page(&self) {
            let page = self.page.get();
            if page > 0 {
                self.obj().load_page(page - 1, false);
            }
        }

        #[template_callback]
        fn show_next_page(&self) {
            if let Some(next) = self.next.get() {
                self.obj().load_page(next, false);
            }
        }
    }
}

glib::wrapper! {
    /// The Change Artwork dialog.
    pub struct ArtworkDialog(ObjectSubclass<imp::ArtworkDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl ArtworkDialog {
    /// Opens Change Artwork for `game` over `parent`, with fullscreen mode's tabs and
    /// progressive loading when `fullscreen`, else the desktop window's list and arrows.
    pub fn change(
        parent: &impl IsA<gtk::Widget>,
        library: LibraryService,
        game: &GameObject,
        fullscreen: bool,
    ) {
        let dialog: Self = glib::Object::new();
        let imp = dialog.imp();
        imp.library.replace(Some(library));
        imp.game.replace(Some(PickerGame::of(game)));
        imp.purpose.replace(Purpose::Apply(game.clone()));
        imp.fullscreen.set(fullscreen);
        let handler = game.connect_art_changed(glib::clone!(
            #[weak]
            dialog,
            move |_| dialog.show_current_art()
        ));
        imp.art_handler.replace(Some(handler));
        dialog.build_kinds(&KINDS);
        dialog.present(Some(parent));
        dialog.read_user_art();
        dialog.start(KINDS[0]);
    }

    /// Opens the dialog for one kind of art for `game` over `parent`; `done` gets the
    /// picked image (from SteamGridDB or a file) and the dialog closes.
    pub fn pick(
        parent: &impl IsA<gtk::Widget>,
        library: LibraryService,
        layout: ArtLayout,
        game: PickerGame,
        done: impl Fn(PickedImage) + 'static,
    ) {
        let dialog: Self = glib::Object::new();
        let imp = dialog.imp();
        imp.library.replace(Some(library));
        imp.game.replace(Some(game));
        imp.purpose.replace(Purpose::Pick(Box::new(done)));
        dialog.set_title(&gettext("Choose {kind}").replace("{kind}", &kind_name(layout)));
        dialog.build_kinds(&[]);
        dialog.present(Some(parent));
        dialog.start(layout);
    }

    /// Whether the shoulder buttons switch the dialog's tabs (fullscreen mode).
    pub fn has_tabs(&self) -> bool {
        let imp = self.imp();
        imp.fullscreen.get() && !imp.tabs.borrow().is_empty()
    }

    /// Shows the kind before or after the shown one; says whether there was one.
    pub fn step_kind(&self, forward: bool) -> bool {
        if !self.has_tabs() {
            return false;
        }
        let Some(kind) = self
            .imp()
            .kind
            .get()
            .and_then(|kind| art_picker::step_kind(kind, forward))
        else {
            return false;
        };
        self.show_kind(kind);
        true
    }

    /// The desktop window's list of `kinds` with their current art, or fullscreen mode's
    /// tabs; neither for a single kind.
    fn build_kinds(&self, kinds: &[ArtLayout]) {
        let imp = self.imp();
        let fullscreen = imp.fullscreen.get();
        let list = !kinds.is_empty() && !fullscreen;
        imp.kind_list.set_visible(list);
        imp.kind_separator.set_visible(list);
        imp.kind_tabs.set_visible(!kinds.is_empty() && fullscreen);
        // Use Original shows once the shown kind has the user's own art.
        imp.reset_button.set_visible(false);
        let mut group: Option<gtk::ToggleButton> = None;
        for &kind in kinds {
            if fullscreen {
                let tab = gtk::ToggleButton::builder().label(kind_name(kind)).build();
                tab.set_group(group.as_ref());
                group.get_or_insert_with(|| tab.clone());
                tab.connect_toggled(glib::clone!(
                    #[weak(rename_to = dialog)]
                    self,
                    move |tab| {
                        if tab.is_active() {
                            dialog.show_kind(kind);
                        }
                    }
                ));
                imp.kind_tabs.append(&tab);
                imp.tabs.borrow_mut().push((kind, tab));
            } else {
                // An image, not a picture: it keeps to its size whatever the art's.
                let picture = gtk::Image::builder()
                    .pixel_size(48)
                    .accessible_role(gtk::AccessibleRole::Presentation)
                    .build();
                let row = adw::ActionRow::builder()
                    .title(kind_name(kind))
                    .activatable(true)
                    .build();
                row.add_prefix(&picture);
                imp.kind_list.append(&row);
                imp.rows.borrow_mut().push((kind, row, picture));
            }
        }
        self.show_current_art();
    }

    /// The game's current art beside each kind in the list.
    fn show_current_art(&self) {
        let imp = self.imp();
        let Purpose::Apply(game) = &*imp.purpose.borrow() else {
            return;
        };
        let art = game.entry().art.clone();
        for (kind, _, picture) in imp.rows.borrow().iter() {
            let path = match kind {
                ArtLayout::Cover => art.cover.clone(),
                ArtLayout::Hero => art.hero.clone(),
                ArtLayout::Logo => art.logo.clone(),
                ArtLayout::Icon => art.icon.clone(),
                ArtLayout::Square => art.square.clone(),
            };
            let picture = picture.clone();
            match path {
                Some(path) => {
                    glib::spawn_future_local(async move {
                        let texture = textures::load(&path).await.ok();
                        picture.set_paintable(texture.as_ref());
                    });
                }
                None => picture.set_paintable(None::<&gtk::gdk::Paintable>),
            }
        }
        self.show_user_art();
    }

    /// Reads which kinds have the user's own art, for Use Original and the list.
    fn read_user_art(&self) {
        let imp = self.imp();
        let (Some(library), Some(id)) = (
            imp.library.borrow().clone(),
            imp.game.borrow().as_ref().and_then(|game| game.id.clone()),
        ) else {
            return;
        };
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let layouts = library.user_art_layouts(&id).await;
            if let Some(dialog) = dialog.upgrade() {
                dialog.imp().user_art.replace(layouts);
                dialog.show_user_art();
            }
        });
    }

    /// Use Original for a kind with the user's own art, and each row's subtitle.
    fn show_user_art(&self) {
        let imp = self.imp();
        if !matches!(*imp.purpose.borrow(), Purpose::Apply(_)) {
            return;
        }
        let user_art = imp.user_art.borrow();
        for (kind, row, _) in imp.rows.borrow().iter() {
            row.set_subtitle(&if user_art.contains(kind) {
                gettext("Your image")
            } else {
                gettext("From the launcher or SteamGridDB")
            });
        }
        imp.reset_button
            .set_visible(imp.kind.get().is_some_and(|kind| user_art.contains(&kind)));
    }

    /// Asks for the API key if needed, finds the game on SteamGridDB, then shows `kind`.
    fn start(&self, kind: ArtLayout) {
        let imp = self.imp();
        imp.kind.set(Some(kind));
        self.mark_kind(kind);
        imp.stack.set_visible_child_name("loading");
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let key = steamgriddb_service::api_key().await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            match key {
                Ok(Some(key)) => dialog.connect(SteamGridDb::new(key)).await,
                Ok(None) => dialog.ask_for_key(None),
                Err(error) => dialog.show_message(&gettext("The Keyring Cannot Be Used"), &error),
            }
        });
    }

    /// Finds the game with `client`, then loads the shown kind's first page.
    async fn connect(&self, client: SteamGridDb) {
        let imp = self.imp();
        imp.client.replace(Some(client.clone()));
        let Some(game) = imp.game.borrow().clone() else {
            return;
        };
        imp.change_match.set_visible(game.steam_app_id.is_none());
        if let Some(app_id) = game.steam_app_id {
            imp.game_ref.replace(Some(GameRef::Steam(app_id)));
            imp.match_bar.set_visible(false);
            self.load_page(0, false);
            return;
        }
        let library = imp.library.borrow().clone();
        let remembered = match (&library, &game.id) {
            (Some(library), Some(id)) => library.steamgriddb_match(id).await,
            _ => None,
        };
        let hits = match client.search(&game.title).await {
            Ok(hits) => hits,
            Err(error) if error.key_rejected() => {
                self.ask_for_key(Some(&gettext(
                    "SteamGridDB did not accept the stored key. Enter a valid one; it is kept in your keyring.",
                )));
                return;
            }
            Err(error) => {
                self.show_message(
                    &gettext("SteamGridDB Cannot Be Reached"),
                    &error.to_string(),
                );
                return;
            }
        };
        imp.hits.replace(hits.clone());
        let chosen = remembered
            .or_else(|| matching_hit(&game.title, &hits))
            .or_else(|| hits.first().map(|hit| hit.id));
        match chosen {
            Some(id) => {
                let name = hits
                    .iter()
                    .find(|hit| hit.id == id)
                    .map_or_else(|| game.title.clone(), |hit| hit.name.clone());
                self.use_match(id, &name);
                self.load_page(0, false);
            }
            None => self.show_matches(),
        }
    }

    /// Shows art of the SteamGridDB game `id`, named `name`.
    fn use_match(&self, id: u64, name: &str) {
        let imp = self.imp();
        imp.game_ref.replace(Some(GameRef::Grid(id)));
        imp.match_bar.set_visible(true);
        imp.match_label
            .set_label(&gettext("SteamGridDB: {game}").replace("{game}", name));
    }

    /// Selects `kind` in the list or its tab, without showing it again.
    fn mark_kind(&self, kind: ArtLayout) {
        let imp = self.imp();
        for (candidate, tab) in imp.tabs.borrow().iter() {
            if *candidate == kind && !tab.is_active() {
                tab.set_active(true);
            }
        }
        for (candidate, row, _) in imp.rows.borrow().iter() {
            if *candidate == kind && !row.is_selected() {
                imp.kind_list.select_row(Some(row));
            }
        }
    }

    /// Shows SteamGridDB's images of `kind`, from the first page.
    fn show_kind(&self, kind: ArtLayout) {
        let imp = self.imp();
        if imp.kind.get() == Some(kind) {
            return;
        }
        imp.kind.set(Some(kind));
        self.mark_kind(kind);
        self.show_user_art();
        if imp.game_ref.borrow().is_some() {
            self.load_page(0, false);
        }
    }

    /// Loads `page` of the shown kind: in place of the shown images, or after them when
    /// `append`ing (fullscreen mode's progressive loading).
    fn load_page(&self, page: u32, append: bool) {
        let imp = self.imp();
        let (Some(client), Some(game_ref), Some(kind)) = (
            imp.client.borrow().clone(),
            imp.game_ref.borrow().clone(),
            imp.kind.get(),
        ) else {
            return;
        };
        let generation = if append {
            imp.generation.get()
        } else {
            imp.generation.set(imp.generation.get() + 1);
            imp.stack.set_visible_child_name("loading");
            imp.generation.get()
        };
        imp.loading_more.set(true);
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let answer = client.images_page(kind, &game_ref, page).await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            let imp = dialog.imp();
            if imp.generation.get() != generation {
                return;
            }
            imp.loading_more.set(false);
            let page = match answer {
                Ok(page) => page,
                Err(error) if error.key_rejected() => {
                    dialog.ask_for_key(Some(&gettext(
                        "SteamGridDB did not accept the stored key. Enter a valid one; it is kept in your keyring.",
                    )));
                    return;
                }
                Err(error) if append => {
                    dialog.toast(&error.to_string());
                    return;
                }
                Err(error) => {
                    dialog.show_message(
                        &gettext("SteamGridDB Cannot Be Reached"),
                        &error.to_string(),
                    );
                    return;
                }
            };
            imp.page.set(page.page);
            imp.next.set(page.next_page());
            imp.page_label.set_label(
                &gettext("Page {page} of {pages}")
                    .replace("{page}", &(page.page + 1).to_string())
                    .replace("{pages}", &page.pages.to_string()),
            );
            imp.previous_page.set_sensitive(page.page > 0);
            imp.next_page.set_sensitive(page.next_page().is_some());
            if !append {
                imp.images.remove_all();
                imp.shown.borrow_mut().clear();
                imp.scroller.vadjustment().set_value(0.0);
            }
            if !append && page.images.is_empty() {
                dialog.show_message(
                    &gettext("No Images Found"),
                    &gettext("SteamGridDB has no images of this kind for the game"),
                );
                return;
            }
            imp.stack.set_visible_child_name("images");
            dialog
                .add_images(client, kind, page.images, generation)
                .await;
        });
    }

    /// Adds `images` to the grid, then fetches their thumbnails one by one.
    async fn add_images(
        &self,
        client: SteamGridDb,
        kind: ArtLayout,
        images: Vec<Image>,
        generation: u64,
    ) {
        let imp = self.imp();
        let (width, height) = thumbnail_size(kind);
        let mut pictures = Vec::with_capacity(images.len());
        for image in images {
            let picture = gtk::Picture::builder()
                .content_fit(gtk::ContentFit::Contain)
                .width_request(width)
                .height_request(height)
                .can_shrink(true)
                .build();
            // A picture asks for its image's own size; the clamp keeps it to the thumbnail's.
            let clamp = adw::Clamp::builder()
                .maximum_size(width)
                .tightening_threshold(width)
                .child(&picture)
                .build();
            let child = gtk::FlowBoxChild::builder()
                .child(&clamp)
                .tooltip_text(format!("{}×{}", image.width, image.height))
                .build();
            let index = imp.shown.borrow().len();
            child.update_property(&[gtk::accessible::Property::Label(
                &gettext("Image {number}, {width} by {height} pixels")
                    .replace("{number}", &(index + 1).to_string())
                    .replace("{width}", &image.width.to_string())
                    .replace("{height}", &image.height.to_string()),
            )]);
            let focus = gtk::EventControllerFocus::new();
            focus.connect_enter(glib::clone!(
                #[weak(rename_to = dialog)]
                self,
                move |focus| {
                    if let Some(child) = focus.widget().and_downcast::<gtk::FlowBoxChild>() {
                        dialog.focused(child.index());
                    }
                }
            ));
            child.add_controller(focus);
            imp.images.append(&child);
            imp.shown.borrow_mut().push(image.clone());
            pictures.push((picture, image.thumb));
        }
        // In fullscreen mode a fresh page takes the focus, once shown, so the D-pad moves
        // through it.
        if imp.fullscreen.get()
            && imp.shown.borrow().len() == pictures.len()
            && let Some(first) = imp.images.child_at_index(0)
        {
            glib::idle_add_local_once(move || {
                first.grab_focus();
            });
        }
        // Thumbnails load one by one through the shared request slot.
        for (picture, thumb) in pictures {
            if imp.generation.get() != generation {
                return;
            }
            if let Ok(path) = client.image_file(&thumb).await
                && let Ok(texture) = textures::load(&path).await
            {
                picture.set_paintable(Some(&texture));
            }
        }
    }

    /// Fullscreen mode loads the next page once focus reaches the last two rows.
    fn focused(&self, index: i32) {
        let imp = self.imp();
        if !imp.fullscreen.get() || index < 0 {
            return;
        }
        // A grid not laid out yet has no rows to count; scrolling to its end loads more.
        let Some(columns) = self.columns() else {
            return;
        };
        let loaded = imp.shown.borrow().len();
        if art_picker::loads_more(Some(index as usize), loaded, columns, imp.next.get()) {
            self.load_more();
        }
    }

    /// Loads the page after the loaded ones, once.
    fn load_more(&self) {
        let imp = self.imp();
        if imp.loading_more.get() {
            return;
        }
        if let Some(next) = imp.next.get() {
            self.load_page(next, true);
        }
    }

    /// How many images the grid fits to a row, from its width and an image's, as the grid
    /// lays them out; `None` before it has a width.
    fn columns(&self) -> Option<usize> {
        let images: &gtk::FlowBox = &self.imp().images;
        let width = images.width();
        let child = images.child_at_index(0)?;
        let (_, natural, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
        if width <= 0 || natural <= 0 {
            return None;
        }
        let spacing = images.column_spacing() as i32;
        let fit = ((width + spacing) / (natural + spacing)).max(1) as u32;
        Some(fit.min(images.max_children_per_line()) as usize)
    }

    /// Downloads the image at `index` of the grid and uses it.
    fn use_image(&self, index: i32) {
        let imp = self.imp();
        let image = usize::try_from(index)
            .ok()
            .and_then(|index| imp.shown.borrow().get(index).cloned());
        let (Some(image), Some(client)) = (image, imp.client.borrow().clone()) else {
            return;
        };
        self.set_sensitive(false);
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let downloaded = client.image(&image.url).await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            dialog.set_sensitive(true);
            match downloaded {
                Ok(picked) => dialog.use_picked(picked).await,
                Err(error) => dialog.toast(
                    &gettext("The image cannot be downloaded: {error}")
                        .replace("{error}", &error.to_string()),
                ),
            }
        });
    }

    /// Makes `image` the game's art in the shown kind, or hands it to the caller.
    async fn use_picked(&self, image: PickedImage) {
        let imp = self.imp();
        let Some(kind) = imp.kind.get() else { return };
        let game = match &*imp.purpose.borrow() {
            Purpose::None => return,
            Purpose::Pick(done) => {
                done(image);
                self.close();
                return;
            }
            Purpose::Apply(game) => game.id(),
        };
        let Some(library) = imp.library.borrow().clone() else {
            return;
        };
        match library.set_user_art(&game, kind, image).await {
            Ok(()) => {
                let mut user_art = imp.user_art.borrow_mut();
                if !user_art.contains(&kind) {
                    user_art.push(kind);
                }
                drop(user_art);
                self.show_user_art();
                self.toast(&gettext("{kind} changed").replace("{kind}", &kind_name(kind)));
            }
            Err(error) => self.toast(&error),
        }
    }

    /// Picks an image file through the portal and uses it.
    fn choose_file(&self) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Images")));
        filter.add_mime_type("image/*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let chooser = gtk::FileDialog::builder()
            .title(gettext("Choose an Image"))
            .modal(true)
            .filters(&filters)
            .build();
        let root = self.root().and_downcast::<gtk::Window>();
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(file) = chooser.open_future(root.as_ref()).await else {
                return;
            };
            let read = file_service::read_image(&file).await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            match read {
                Ok(image) => dialog.use_picked(image).await,
                Err(error) => dialog.toast(&error),
            }
        });
    }

    /// Removes the user's own art in the shown kind.
    fn reset(&self) {
        let imp = self.imp();
        let (Some(kind), Some(library)) = (imp.kind.get(), imp.library.borrow().clone()) else {
            return;
        };
        let Purpose::Apply(game) = &*imp.purpose.borrow() else {
            return;
        };
        let id = game.id();
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let removed = library.remove_user_art(&id, kind).await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            match removed {
                Ok(()) => {
                    dialog
                        .imp()
                        .user_art
                        .borrow_mut()
                        .retain(|&layout| layout != kind);
                    dialog.show_user_art();
                    dialog.toast(&gettext("Your image was removed; the original shows again"));
                }
                Err(error) => dialog.toast(&error),
            }
        });
    }

    /// Lists the games a title search finds, to choose the right one.
    fn show_matches(&self) {
        let imp = self.imp();
        if imp.match_entry.text().is_empty()
            && let Some(game) = imp.game.borrow().as_ref()
        {
            imp.match_entry.set_text(&game.title);
        }
        self.fill_matches();
        imp.stack.set_visible_child_name("matches");
        imp.match_entry.grab_focus();
    }

    /// Searches SteamGridDB for the entry's name.
    fn search_matches(&self) {
        let imp = self.imp();
        let Some(client) = imp.client.borrow().clone() else {
            return;
        };
        let name = imp.match_entry.text().trim().to_owned();
        if name.is_empty() {
            return;
        }
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let hits = client.search(&name).await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            match hits {
                Ok(hits) => {
                    dialog.imp().hits.replace(hits);
                    dialog.fill_matches();
                }
                Err(error) => dialog.toast(&error.to_string()),
            }
        });
    }

    fn fill_matches(&self) {
        let imp = self.imp();
        imp.match_list.remove_all();
        let hits = imp.hits.borrow();
        if hits.is_empty() {
            imp.match_list.append(
                &adw::ActionRow::builder()
                    .title(gettext("No games found; try another name"))
                    .activatable(false)
                    .build(),
            );
        }
        for hit in hits.iter() {
            imp.match_list.append(
                &adw::ActionRow::builder()
                    .title(glib::markup_escape_text(&hit.name))
                    .activatable(true)
                    .build(),
            );
        }
    }

    /// Shows art of the chosen game, and remembers it for a library game.
    fn choose_match(&self, hit: SearchHit) {
        let imp = self.imp();
        self.use_match(hit.id, &hit.name);
        if let (Some(library), Some(id)) = (
            imp.library.borrow().clone(),
            imp.game.borrow().as_ref().and_then(|game| game.id.clone()),
        ) {
            glib::spawn_future_local(async move {
                library.set_steamgriddb_match(&id, hit.id).await;
            });
        }
        self.load_page(0, false);
    }

    fn show_message(&self, title: &str, description: &str) {
        let imp = self.imp();
        imp.status.set_icon_name(Some("image-missing-symbolic"));
        imp.status.set_title(title);
        imp.status.set_description(Some(description));
        imp.status.set_child(None::<&gtk::Widget>);
        imp.stack.set_visible_child_name("status");
    }

    /// Asks for the API key, stores it in the keyring, then goes on; `note` says why.
    fn ask_for_key(&self, note: Option<&str>) {
        let imp = self.imp();
        let entry = adw::PasswordEntryRow::builder()
            .title(gettext("API Key"))
            .build();
        let list = gtk::ListBox::builder()
            .css_classes(["boxed-list"])
            .selection_mode(gtk::SelectionMode::None)
            .build();
        list.append(&entry);
        let save = gtk::Button::builder()
            .label(gettext("_Save Key"))
            .use_underline(true)
            .halign(gtk::Align::Center)
            .css_classes(["pill", "suggested-action"])
            .build();
        let link = gtk::LinkButton::with_label(KEY_PAGE, &gettext("Get a Key on SteamGridDB"));
        let column = gtk::Box::new(gtk::Orientation::Vertical, 18);
        column.append(&list);
        column.append(&save);
        column.append(&link);
        let clamp = adw::Clamp::builder()
            .maximum_size(400)
            .child(&column)
            .build();
        imp.status.set_icon_name(Some("dialog-password-symbolic"));
        imp.status
            .set_title(&gettext("SteamGridDB Needs Your API Key"));
        imp.status.set_description(Some(note.unwrap_or(&gettext(
            "The key is kept in your keyring, never in plain settings.",
        ))));
        imp.status.set_child(Some(&clamp));
        imp.stack.set_visible_child_name("status");
        let dialog = self.downgrade();
        save.connect_clicked(move |_| {
            let key = entry.text().trim().to_owned();
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            if key.is_empty() {
                return;
            }
            glib::spawn_future_local(async move {
                dialog.imp().stack.set_visible_child_name("loading");
                match steamgriddb_service::set_api_key(&key).await {
                    Ok(()) => dialog.connect(SteamGridDb::new(key)).await,
                    Err(error) => dialog.show_message(&gettext("The Key Was Not Saved"), &error),
                }
            });
        });
    }

    fn toast(&self, message: &str) {
        self.imp().toasts.add_toast(adw::Toast::new(message));
    }
}

/// What the dialog calls a kind of art.
pub fn kind_name(kind: ArtLayout) -> String {
    match kind {
        ArtLayout::Cover => gettext("Cover"),
        ArtLayout::Hero => gettext("Banner"),
        ArtLayout::Logo => gettext("Logo"),
        ArtLayout::Icon => gettext("Icon"),
        ArtLayout::Square => gettext("Square Art"),
    }
}

/// The size a kind's thumbnails show at.
fn thumbnail_size(kind: ArtLayout) -> (i32, i32) {
    match kind {
        ArtLayout::Cover => (150, 225),
        ArtLayout::Hero => (280, 90),
        ArtLayout::Logo | ArtLayout::Icon | ArtLayout::Square => (160, 160),
    }
}
