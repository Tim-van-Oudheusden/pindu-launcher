//! An app's page in Get Games (#887; `docs/get-games.md` sections 6.7 and 8.1): its icon,
//! name and summary, what Flathub's catalogue says about it (developer, licence and
//! verification in words, age rating, sizes, gamepad support), one main action that
//! follows its state, Remove beside it once installed, how a job for it goes, its
//! screenshots, which the shoulder buttons turn, and its description.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gdk, glib};
use pindu_core::add_store::StoreApp;
use pindu_core::catalogue::{App, Block, Licence, Screenshot};
use pindu_core::get_games::{Job, Outcome, Running, Sizes};
use pindu_core::model::GameId;

use crate::file_service::fetch_image;
use crate::get_games_page::{icon_path, load_icon};
use crate::get_games_service::{network_failure, no_space};
use crate::widgets::textures;

/// How many screenshots stay decoded for the session, most recently shown kept.
const SCREENSHOT_CACHE: usize = 24;

thread_local! {
    /// Screenshots fetched this session, by address, with when each was last shown.
    static SCREENSHOTS: RefCell<HashMap<String, (gdk::Texture, u64)>> = RefCell::default();
    static SHOWN: Cell<u64> = const { Cell::new(0) };
}

/// What the main button does for the app as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MainAction {
    /// Installs it.
    Install,
    /// Installs a store through Add a Store's flow.
    AddStore(StoreApp),
    /// Cancels the job queued or running for it.
    Cancel,
    /// Plays the game, which the library has under this ID.
    Play(GameId),
    /// Opens the app through its desktop entry, with this ID.
    Open(String),
    /// Nothing, as for a removal that runs.
    Nothing,
}

/// What the page shows of its app.
#[derive(Debug, Clone, Copy)]
pub struct AppView<'a> {
    /// The app.
    pub app: &'a App,
    /// The catalogue's folder, which the icon is under.
    pub folder: Option<&'a Path>,
    /// Whether it is installed.
    pub installed: bool,
    /// Whether it is an extra rather than a game.
    pub extra: bool,
    /// The store it is, which installs through Add a Store.
    pub store: Option<StoreApp>,
    /// The library's ID of the game, once it has it.
    pub game: Option<&'a GameId>,
    /// The desktop entry Apps opens it by, once Apps lists it.
    pub opens: Option<&'a str>,
    /// The job running for it.
    pub running: Option<&'a Running>,
    /// Whether a job for it waits its turn.
    pub queued: bool,
    /// How its last job ended.
    pub outcome: Option<&'a Outcome>,
    /// Its sizes, once Flathub said.
    pub sizes: Option<Sizes>,
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/get_games_app_page.ui")]
    pub struct GetGamesAppPage {
        #[template_child]
        pub toolbar: TemplateChild<adw::ToolbarView>,
        #[template_child]
        pub scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub name: TemplateChild<gtk::Label>,
        #[template_child]
        pub summary: TemplateChild<gtk::Label>,
        #[template_child]
        pub facts: TemplateChild<gtk::FlowBox>,
        #[template_child]
        pub main_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub remove_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub job: TemplateChild<gtk::Box>,
        #[template_child]
        pub progress: TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub job_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub screenshots: TemplateChild<gtk::Box>,
        #[template_child]
        pub screenshot: TemplateChild<gtk::Picture>,
        #[template_child]
        pub previous_shot: TemplateChild<gtk::Button>,
        #[template_child]
        pub next_shot: TemplateChild<gtk::Button>,
        #[template_child]
        pub screenshot_count: TemplateChild<gtk::Label>,
        #[template_child]
        pub description: TemplateChild<gtk::Box>,
        /// The app the page is for.
        pub id: RefCell<String>,
        /// Whether its icon, description and screenshots are filled in.
        pub filled: Cell<bool>,
        pub shots: RefCell<Vec<Screenshot>>,
        pub shot: Cell<usize>,
        /// The facts shown, to rebuild them only when they change.
        pub shown_facts: RefCell<Vec<String>>,
        pub action: RefCell<Option<MainAction>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GetGamesAppPage {
        const NAME: &'static str = "PinduGetGamesAppPage";
        type Type = super::GetGamesAppPage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for GetGamesAppPage {}
    impl WidgetImpl for GetGamesAppPage {}
    impl NavigationPageImpl for GetGamesAppPage {}
}

glib::wrapper! {
    /// An app's page in Get Games.
    pub struct GetGamesAppPage(ObjectSubclass<imp::GetGamesAppPage>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl GetGamesAppPage {
    /// A page for the app `id`, empty until [`show`](Self::show) fills it.
    pub fn new(id: &str) -> Self {
        let page: Self = glib::Object::new();
        page.imp().id.replace(id.to_owned());
        page
    }

    /// The app the page is for.
    pub fn app_id(&self) -> String {
        self.imp().id.borrow().clone()
    }

    /// What the main button does now.
    pub fn main_action(&self) -> MainAction {
        self.imp()
            .action
            .borrow()
            .clone()
            .unwrap_or(MainAction::Nothing)
    }

    /// Shows the page as fullscreen mode does, without its header bar, or as the desktop
    /// window does.
    pub fn set_fullscreen(&self, fullscreen: bool) {
        self.imp().toolbar.set_reveal_top_bars(!fullscreen);
    }

    /// Gives focus to the main button.
    pub fn focus_main(&self) -> bool {
        self.imp().main_button.grab_focus()
    }

    /// Shows `view`: on the first call the app's name, icon, screenshots and description,
    /// on every call its facts, main action, Remove and the job running for it.
    pub fn show(&self, view: AppView<'_>) {
        let imp = self.imp();
        let app = view.app;
        if !imp.filled.replace(true) {
            self.set_title(&app.name);
            imp.name.set_label(&app.name);
            imp.summary.set_label(&app.summary);
            if let Some(path) = icon_path(app, view.folder) {
                load_icon(&imp.icon, path);
            }
            self.fill_description(&app.description);
            imp.shots.replace(app.screenshots.clone());
            imp.screenshots.set_visible(!app.screenshots.is_empty());
            self.show_screenshot();
        }
        self.show_facts(&facts(app, view.sizes));
        let busy = view.running.is_some() || view.queued;
        let action = if busy {
            if matches!(
                view.running.map(|running| &running.job),
                Some(Job::Remove { .. })
            ) {
                MainAction::Nothing
            } else {
                MainAction::Cancel
            }
        } else if let Some(store) = view.store.filter(|_| !view.installed) {
            MainAction::AddStore(store)
        } else if !view.installed {
            MainAction::Install
        } else if let Some(game) = view.game.filter(|_| !view.extra) {
            MainAction::Play(game.clone())
        } else if let Some(opens) = view.opens {
            MainAction::Open(opens.to_owned())
        } else {
            MainAction::Nothing
        };
        let (label, suggested) = match &action {
            MainAction::Install | MainAction::AddStore(_) => (gettext("Install"), true),
            MainAction::Cancel => (gettext("Cancel"), false),
            MainAction::Play(_) => (gettext("Play"), true),
            MainAction::Open(_) => (gettext("Open"), true),
            MainAction::Nothing if view.running.is_some() => (gettext("Removing…"), false),
            // Installed, but the library or Apps has not seen it yet.
            MainAction::Nothing => (gettext("Installed"), false),
        };
        imp.main_button.set_label(&label);
        let was_sensitive = imp.main_button.is_sensitive();
        imp.main_button.set_sensitive(action != MainAction::Nothing);
        if suggested {
            imp.main_button.add_css_class("suggested-action");
        } else {
            imp.main_button.remove_css_class("suggested-action");
        }
        imp.remove_button.set_visible(view.installed && !busy);
        // Focus left for Remove while the button waited, as for the library to show a
        // game just installed: it comes back once Play or Open is there.
        if !was_sensitive
            && imp.main_button.is_sensitive()
            && (imp.remove_button.has_focus() || !imp.remove_button.is_visible())
        {
            imp.main_button.grab_focus();
        }
        imp.action.replace(Some(action));
        self.show_job(view);
    }

    /// Shows how the job for the app goes, or how its last one ended.
    fn show_job(&self, view: AppView<'_>) {
        let imp = self.imp();
        let (progress, text) = match (view.running, view.queued, view.outcome) {
            (Some(running), ..) => {
                let text = match (&running.job, running.step, running.cancelling) {
                    (_, _, true) => gettext("Cancelling…"),
                    (Job::Remove { .. }, ..) => gettext("Removing…"),
                    (_, Some((operation, operations)), _) => {
                        // Translators: which of an install's downloads runs, such as
                        // "Installing 2 of 3": the app, its runtime and their extensions.
                        gettext("Installing {operation} of {operations}")
                            .replace("{operation}", &operation.to_string())
                            .replace("{operations}", &operations.to_string())
                    }
                    (_, None, _) if running.checking => gettext("Checking the space…"),
                    _ => gettext("Installing…"),
                };
                (Some(running.progress), text)
            }
            (None, true, _) => (None, gettext("Waiting for the job before it")),
            (None, false, Some(Outcome::Failed { error, network })) => (
                None,
                if *network {
                    network_failure()
                } else {
                    error.clone()
                },
            ),
            (None, false, Some(Outcome::NoSpace { needed, free })) => {
                (None, no_space(*needed, *free))
            }
            _ => {
                imp.job.set_visible(false);
                return;
            }
        };
        imp.job.set_visible(true);
        match progress {
            Some(Some(fraction)) => {
                imp.progress.set_visible(true);
                imp.progress.set_fraction(fraction);
            }
            Some(None) => {
                imp.progress.set_visible(true);
                imp.progress.pulse();
            }
            None => imp.progress.set_visible(false),
        }
        imp.job_label.set_label(&text);
    }

    fn show_facts(&self, facts: &[String]) {
        let imp = self.imp();
        if *imp.shown_facts.borrow() == facts {
            return;
        }
        imp.facts.remove_all();
        for fact in facts {
            imp.facts.append(
                &gtk::Label::builder()
                    .label(fact)
                    .css_classes(["get-games-fact", "caption"])
                    .build(),
            );
        }
        imp.shown_facts.replace(facts.to_vec());
    }

    fn fill_description(&self, blocks: &[Block]) {
        let imp = self.imp();
        for block in blocks {
            let text = match block {
                Block::Paragraph(text) => text.clone(),
                Block::Item(text) => format!("• {text}"),
            };
            imp.description.append(
                &gtk::Label::builder()
                    .label(&text)
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .selectable(false)
                    .build(),
            );
        }
    }

    /// Turns to the screenshot `by` places on, as the shoulder buttons and the arrows
    /// beside it do; `false` when there is none that way.
    pub fn turn_screenshot(&self, by: i32) -> bool {
        let imp = self.imp();
        let count = imp.shots.borrow().len();
        let Some(shot) = imp
            .shot
            .get()
            .checked_add_signed(by as isize)
            .filter(|shot| *shot < count)
        else {
            return false;
        };
        imp.shot.set(shot);
        self.show_screenshot();
        true
    }

    /// Shows the screenshot the page is on, fetching it the first time.
    fn show_screenshot(&self) {
        let imp = self.imp();
        let shots = imp.shots.borrow();
        let index = imp.shot.get();
        let Some(shot) = shots.get(index) else {
            return;
        };
        imp.previous_shot.set_sensitive(index > 0);
        imp.next_shot.set_sensitive(index + 1 < shots.len());
        // Translators: which screenshot shows, such as "2 of 5".
        imp.screenshot_count.set_label(
            &gettext("{shot} of {shots}")
                .replace("{shot}", &(index + 1).to_string())
                .replace("{shots}", &shots.len().to_string()),
        );
        imp.screenshot.set_alternative_text(Some(
            // Translators: {name} is an app's name.
            &gettext("Screenshot of {name}").replace("{name}", &imp.name.label()),
        ));
        let address = shot.image.clone();
        if let Some(texture) = cached_screenshot(&address) {
            imp.screenshot.set_paintable(Some(&texture));
            return;
        }
        imp.screenshot.set_paintable(gdk::Paintable::NONE);
        let page = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(image) = fetch_image(&address).await else {
                return;
            };
            let Ok(texture) = textures::decode_bytes(&image.bytes).await else {
                return;
            };
            keep_screenshot(&address, &texture);
            if let Some(page) = page.upgrade()
                && page
                    .imp()
                    .shots
                    .borrow()
                    .get(page.imp().shot.get())
                    .map(|shot| &shot.image)
                    == Some(&address)
            {
                page.imp().screenshot.set_paintable(Some(&texture));
            }
        });
    }

    /// Whether the app has more than one screenshot to turn through.
    pub fn has_screenshots(&self) -> bool {
        self.imp().shots.borrow().len() > 1
    }
}

/// The facts the page shows about `app`, in words.
fn facts(app: &App, sizes: Option<Sizes>) -> Vec<String> {
    let mut facts = Vec::new();
    if let Some(developer) = &app.developer {
        // Translators: who makes an app, such as "By SuperTuxKart Team".
        facts.push(gettext("By {developer}").replace("{developer}", developer));
    }
    facts.push(if app.verified {
        gettext("Verified on Flathub")
    } else {
        gettext("Not verified on Flathub")
    });
    facts.push(match &app.licence {
        Licence::Free(_) => gettext("Free software"),
        Licence::Proprietary => gettext("Proprietary"),
        Licence::Unknown => gettext("Licence unknown"),
    });
    match app.age {
        Some(0) => facts.push(gettext("All ages")),
        // Translators: the youngest age an app's content suits, such as "Ages 13+".
        Some(age) => facts.push(gettext("Ages {age}+").replace("{age}", &age.to_string())),
        None => {}
    }
    if app.gamepad {
        facts.push(gettext("Gamepad support"));
    }
    if let Some(sizes) = sizes {
        // Translators: {size} is a size, such as "1.2 GB".
        facts
            .push(gettext("{size} download").replace("{size}", &glib::format_size(sizes.download)));
        facts.push(
            // Translators: {size} is a size, such as "3.4 GB".
            gettext("{size} installed").replace("{size}", &glib::format_size(sizes.installed)),
        );
    }
    facts
}

/// The screenshot at `address`, when it was fetched this session.
fn cached_screenshot(address: &str) -> Option<gdk::Texture> {
    let shown = SHOWN.with(|shown| {
        shown.set(shown.get() + 1);
        shown.get()
    });
    SCREENSHOTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let (texture, last) = cache.get_mut(address)?;
        *last = shown;
        Some(texture.clone())
    })
}

/// Keeps the screenshot fetched from `address`, dropping the one shown longest ago once
/// there are [`SCREENSHOT_CACHE`].
fn keep_screenshot(address: &str, texture: &gdk::Texture) {
    let shown = SHOWN.with(|shown| {
        shown.set(shown.get() + 1);
        shown.get()
    });
    SCREENSHOTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= SCREENSHOT_CACHE
            && let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, (_, last))| *last)
                .map(|(address, _)| address.clone())
        {
            cache.remove(&oldest);
        }
        cache.insert(address.to_owned(), (texture.clone(), shown));
    });
}
