//! The menu drawer and Quick Access over a running game in a `--steam` gamescope:
//! [`PinduGameOverlay`], a second toplevel that hosts the drawer (`menu_drawer`) on the left
//! and the Quick Access widget (`quick_access`) on the right, with its own Blueprint
//! (`data/ui/game_overlay.blp`). The application makes one when the gamescope service
//! speaks the protocol; the service marks it as gamescope's overlay and opens and closes it
//! (Guide for the drawer, Guide + A or the Steam Deck's "…" for Quick Access, with a game in
//! front, and the game in front changing), and this window follows: it shows the open
//! panel's levels as the main window does, for the game in front, and acts on the
//! controller while it holds input.
//!
//! The drawer over a game holds the Playing Now cards, Home, Library and Collections; an
//! entry other than a card brings the main window to the front, where it does what it
//! does there. Quick Access over a game offers what needs no library page and no flow of
//! the main window ([`PinduQuickAccess::set_over_game`]). Network's passphrase and
//! Bluetooth's PIN and codes are asked here while Quick Access is open (`secret_dialog`),
//! with the dialog's keyboard taking the controller and its buttons in this window's hint
//! bar. Playing Now's Return and Stop, the volume preview and a notification's link go to
//! the main window, which a link brings to the front. The controller still reaches the game
//! while a panel is open: only the OS can take it away from the game.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::gamepad::{Direction, Intent, Layout, WindowFocus};
use pindu_core::gamescope::Xid;
use pindu_core::gamescope::overlay::OverGame;
use pindu_core::hints::{self, Context, Focus, Hint, Launching};
use pindu_core::launch;
use pindu_core::model::GameId;
use pindu_core::panels::{DrawerLevel, Panel, Panels, QuickAccessLevel, Under, page_entries};

use crate::application::PinduApplication;
use crate::fullscreen::Sound;
use crate::hint_bar::{fill_hint_bar, focus_kind};
use crate::menu_drawer::PinduMenuDrawer;
use crate::quick_access::PinduQuickAccess;
use crate::window::PinduWindow;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/game_overlay.ui")]
    pub struct PinduGameOverlay {
        #[template_child]
        pub drawer_pane: TemplateChild<gtk::Box>,
        #[template_child]
        pub drawer: TemplateChild<PinduMenuDrawer>,
        #[template_child]
        pub drawer_hint_bar: TemplateChild<gtk::Box>,
        #[template_child]
        pub shade: TemplateChild<gtk::Box>,
        #[template_child]
        pub quick_access_pane: TemplateChild<gtk::Box>,
        #[template_child]
        pub quick_access: TemplateChild<PinduQuickAccess>,
        #[template_child]
        pub hint_bar: TemplateChild<gtk::Box>,
        pub application: glib::WeakRef<PinduApplication>,
        /// The main window, which keeps Playing Now's actions and shows what links lead to.
        pub main_window: glib::WeakRef<PinduWindow>,
        /// The drawer or Quick Access at its level while it is open over the game.
        pub panels: Cell<Panels>,
        /// The hints shown, with whether they are the drawer's and the layout they were
        /// drawn for.
        pub hints: RefCell<Option<(bool, Layout, Vec<Hint>)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduGameOverlay {
        const NAME: &'static str = "PinduGameOverlay";
        type Type = super::PinduGameOverlay;
        type ParentType = adw::Window;

        fn class_init(klass: &mut Self::Class) {
            PinduMenuDrawer::ensure_type();
            PinduQuickAccess::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduGameOverlay {}

    impl WidgetImpl for PinduGameOverlay {}

    impl WindowImpl for PinduGameOverlay {}

    impl AdwWindowImpl for PinduGameOverlay {}
}

glib::wrapper! {
    /// The window the menu drawer and Quick Access open in over a running game in a
    /// `--steam` gamescope.
    pub struct PinduGameOverlay(ObjectSubclass<imp::PinduGameOverlay>)
        @extends adw::Window, gtk::Window, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native,
            gtk::Root, gtk::ShortcutManager;
}

impl PinduGameOverlay {
    /// The overlay for `application`, whose main window is `window`. It is no window of the
    /// application's, which would make it the active one, so it reaches the `app.*` actions
    /// itself and has its own `win.*` group; it goes with the main window.
    pub fn new(application: &PinduApplication, window: &PinduWindow) -> Self {
        let overlay: Self = glib::Object::new();
        let imp = overlay.imp();
        imp.application.set(Some(application));
        imp.main_window.set(Some(window));
        overlay.insert_action_group("app", Some(application));
        let actions = gio::SimpleActionGroup::new();
        overlay.insert_action_group("win", Some(&actions));
        imp.quick_access.set_over_game();
        imp.quick_access.setup(application, &actions);
        imp.drawer.set_over_game();
        imp.drawer.follow_playing(
            application,
            glib::clone!(
                #[weak]
                overlay,
                move || {
                    if overlay.imp().panels.get().open().is_some() {
                        overlay.focus_panel(None);
                    }
                }
            ),
        );
        overlay.add_main_window_actions(&actions);
        overlay.add_drawer_actions(&actions);
        overlay.connect_quick_access();
        let gamescope = application.gamescope_service();
        gamescope.connect_over_game_changed(glib::clone!(
            #[weak]
            overlay,
            move |shown| overlay.set_shown(shown)
        ));
        let gamepad = application.gamepad_service();
        gamepad.connect_intent(glib::clone!(
            #[weak]
            overlay,
            move |intent| overlay.handle_intent(intent)
        ));
        gamepad.connect_active_layout_notify(glib::clone!(
            #[weak]
            overlay,
            move |_| overlay.update_hints()
        ));
        overlay.connect_focus_widget_notify(|overlay| overlay.update_hints());
        overlay.connect_notify_local(Some("visible-dialog"), |overlay, _| {
            overlay.update_hints();
        });
        // A click beside the panel, on the game, closes it.
        let click = gtk::GestureClick::new();
        click.connect_released(glib::clone!(
            #[weak]
            overlay,
            move |_, _, _, _| overlay.request_close()
        ));
        imp.shade.add_controller(click);
        window.connect_destroy(glib::clone!(
            #[weak]
            overlay,
            move |_| overlay.destroy()
        ));
        overlay
    }

    /// Playing Now's Return and Stop and the volume preview are the main window's; Return
    /// to the game in front just closes the panel. A notification's Permissions link opens
    /// in the main window, which comes to the front.
    fn add_main_window_actions(&self, actions: &gio::SimpleActionGroup) {
        let main_action = |name: &'static str, parameter: Option<&glib::VariantTy>| {
            let action = gio::SimpleAction::new(name, parameter);
            action.connect_activate(glib::clone!(
                #[weak(rename_to = overlay)]
                self,
                move |_, target| {
                    if name != "preview-navigation-sound" {
                        overlay.request_close();
                    }
                    if let Some(window) = overlay.imp().main_window.upgrade() {
                        ActionGroupExt::activate_action(&window, name, target);
                    }
                }
            ));
            actions.add_action(&action);
        };
        main_action("playing-return", Some(glib::VariantTy::STRING));
        main_action("playing-stop", Some(glib::VariantTy::STRING));
        main_action("preview-navigation-sound", None);
        let permissions = gio::SimpleAction::new("notification-permissions", None);
        permissions.connect_activate(glib::clone!(
            #[weak(rename_to = overlay)]
            self,
            move |_, _| {
                overlay.show_launcher();
                if let Some(application) = overlay.imp().application.upgrade() {
                    application.activate_action("permissions", None);
                }
            }
        ));
        actions.add_action(&permissions);
    }

    /// The drawer's entries: Collections leads to its level here; Home, Library and a
    /// collection bring the main window to the front, which goes there; a window of the
    /// game shows that window and closes the drawer.
    fn add_drawer_actions(&self, actions: &gio::SimpleActionGroup) {
        let launcher_action = |name: &'static str, parameter: Option<&glib::VariantTy>| {
            let action = gio::SimpleAction::new(name, parameter);
            action.connect_activate(glib::clone!(
                #[weak(rename_to = overlay)]
                self,
                move |_, target| {
                    overlay.show_launcher();
                    if let Some(window) = overlay.imp().main_window.upgrade() {
                        ActionGroupExt::activate_action(&window, name, target);
                    }
                }
            ));
            actions.add_action(&action);
        };
        launcher_action("drawer-home", None);
        launcher_action("drawer-library", None);
        launcher_action("drawer-collection", Some(glib::VariantTy::INT64));
        let collections = gio::SimpleAction::new("drawer-collections", None);
        collections.connect_activate(glib::clone!(
            #[weak(rename_to = overlay)]
            self,
            move |_, _| overlay.show_panel(Panel::Drawer(DrawerLevel::Collections))
        ));
        actions.add_action(&collections);
        let window = gio::SimpleAction::new("game-window", Some(glib::VariantTy::UINT64));
        window.connect_activate(glib::clone!(
            #[weak(rename_to = overlay)]
            self,
            move |_, target| {
                let xid = target
                    .and_then(u64::from_variant)
                    .and_then(|xid| Xid::try_from(xid).ok());
                overlay.request_close();
                if let (Some(xid), Some(application)) = (xid, overlay.imp().application.upgrade()) {
                    application.gamescope_service().show_window(xid);
                }
            }
        ));
        actions.add_action(&window);
    }

    /// Does what Quick Access asks for: close, show a level, leave Activity, or show a
    /// game, which brings the main window to the front on the game's page.
    fn connect_quick_access(&self) {
        let quick_access = &self.imp().quick_access;
        quick_access.connect_close_requested(glib::clone!(
            #[weak(rename_to = overlay)]
            self,
            move |_| overlay.request_close()
        ));
        quick_access.connect_level_requested(glib::clone!(
            #[weak(rename_to = overlay)]
            self,
            move |_, level| overlay.show_panel(Panel::QuickAccess(level))
        ));
        quick_access.connect_activity_ended(glib::clone!(
            #[weak(rename_to = overlay)]
            self,
            move |_| {
                let mut panels = overlay.imp().panels.get();
                panels.activity_ended();
                overlay.set_panels(panels);
            }
        ));
        quick_access.connect_game_requested(glib::clone!(
            #[weak(rename_to = overlay)]
            self,
            move |_, id| overlay.show_game(id)
        ));
    }

    /// Asks the gamescope service to close the panel, which hands input back to the game.
    fn request_close(&self) {
        if let Some(application) = self.imp().application.upgrade() {
            application.gamescope_service().set_over_game(None);
        }
    }

    /// Closes the panel and brings the launcher to the front, for what an entry or a link
    /// leads to.
    fn show_launcher(&self) {
        if let Some(application) = self.imp().application.upgrade() {
            application.gamescope_service().show_launcher();
        }
    }

    /// Brings the main window to the front on the page of the game with `id`.
    fn show_game(&self, id: &GameId) {
        self.show_launcher();
        if let Some(window) = self.imp().main_window.upgrade() {
            window.close_panel();
            window.return_to_game(id);
        }
    }

    /// The gamescope service opened a panel over the game, or closed it: the drawer opens
    /// at its entries, with the game's card first; Quick Access at the notifications while
    /// one is unread, or at its entries, with Performance for the game in front. A
    /// controller opened it, so focus shows at once.
    fn set_shown(&self, shown: Option<OverGame>) {
        let imp = self.imp();
        let mut panels = Panels::new();
        let application = imp.application.upgrade();
        if shown.is_some() {
            self.set_focus_visible(true);
        }
        match shown {
            Some(OverGame::Menu) => {
                if let Some(application) = &application {
                    imp.drawer
                        .fill(&application.library_service().model().collections(), false);
                    imp.drawer
                        .show_windows(&application.gamescope_service().front_windows());
                }
                panels.show(Panel::Drawer(DrawerLevel::Main));
            }
            Some(OverGame::QuickAccess) => {
                let game = application.and_then(|application| {
                    let id = application.gamescope_service().game_in_front()?;
                    application.library_service().model().game_object(&id)
                });
                let under = Under {
                    game: game.is_some(),
                    own_game: game
                        .as_ref()
                        .is_some_and(|game| launch::offers_performance(&game.game())),
                    ..Under::default()
                };
                imp.quick_access
                    .set_page(&page_entries(under), game.as_ref());
                let level = if imp.quick_access.has_unread_notifications() {
                    QuickAccessLevel::Notifications
                } else {
                    QuickAccessLevel::Main
                };
                panels.show(Panel::QuickAccess(level));
            }
            None => {}
        }
        self.set_panels(panels);
    }

    fn show_panel(&self, panel: Panel) {
        let mut panels = self.imp().panels.get();
        panels.show(panel);
        self.set_panels(panels);
    }

    /// Shows the drawer or Quick Access at the level `panels` has, focusing it once it
    /// shows.
    fn set_panels(&self, panels: Panels) {
        let imp = self.imp();
        let before = imp.panels.replace(panels).open();
        let open = panels.open();
        if before == open {
            return;
        }
        let quick_access_level = match open {
            Some(Panel::QuickAccess(level)) => Some(level),
            _ => None,
        };
        let drawer_level = match open {
            Some(Panel::Drawer(level)) => Some(level),
            _ => None,
        };
        imp.drawer_pane.set_visible(drawer_level.is_some());
        imp.drawer.cards().set_shown(drawer_level.is_some());
        if let Some(level) = drawer_level {
            imp.drawer.show_level(level);
        }
        imp.quick_access_pane
            .set_visible(quick_access_level.is_some());
        imp.quick_access.show_level(quick_access_level);
        if open.is_some() {
            // Once the level shows and can take focus.
            glib::idle_add_local_once(glib::clone!(
                #[weak(rename_to = overlay)]
                self,
                move || overlay.focus_panel(before)
            ));
        } else if let Some(dialog) = self.visible_dialog() {
            dialog.force_close();
        }
        self.update_hints();
    }

    /// Focuses the open panel: the entry that led to the level being left, or else its
    /// first item.
    fn focus_panel(&self, left: Option<Panel>) {
        let imp = self.imp();
        match (imp.panels.get().open(), left) {
            (Some(Panel::QuickAccess(_)), left) => {
                let left = match left {
                    Some(Panel::QuickAccess(level)) => Some(level),
                    _ => None,
                };
                imp.quick_access.focus_level(left);
            }
            (Some(Panel::Drawer(level)), left) => {
                let left = match left {
                    Some(Panel::Drawer(level)) => Some(level),
                    _ => None,
                };
                if let Some(target) = imp.drawer.focus_target(level, left) {
                    target.grab_focus();
                }
            }
            (Some(Panel::Options(_)) | None, _) => {}
        }
    }

    /// Acts on a controller intent while a panel is open and holds input: an open
    /// passphrase dialog's keyboard takes every button, directions move focus as in the
    /// main window, A activates, and B closes a dialog, steps back a level or closes the
    /// panel. Guide, Guide + A and Misc1 are the gamescope service's; R3 and the page's
    /// buttons are the game's.
    fn handle_intent(&self, intent: Intent) {
        if self.imp().panels.get().open().is_none() || !self.takes_controller() {
            return;
        }
        self.set_focus_visible(true);
        if self.secret_dialog_intent(intent) {
            return;
        }
        match intent {
            Intent::Navigate(direction) => {
                if !self.adjust_slider(direction) {
                    self.navigate(direction);
                    self.play_sound(Sound::Move);
                }
            }
            Intent::Activate => {
                if let Some(focus) = gtk::prelude::GtkWindowExt::focus(self) {
                    focus.activate();
                }
                self.play_sound(Sound::Select);
            }
            Intent::Back => {
                if let Some(dialog) = self.visible_dialog() {
                    dialog.close();
                } else {
                    let mut panels = self.imp().panels.get();
                    panels.handle(intent, Under::default());
                    if panels.open().is_none() {
                        self.request_close();
                    } else {
                        self.set_panels(panels);
                    }
                }
                self.play_sound(Sound::Back);
            }
            _ => {}
        }
    }

    /// Whether this window has focus, so the controller is its (see `WindowFocus`).
    fn takes_controller(&self) -> bool {
        let compositor_focused = self
            .surface()
            .and_downcast::<gtk::gdk::Toplevel>()
            .is_some_and(|toplevel| toplevel.state().contains(gtk::gdk::ToplevelState::FOCUSED));
        WindowFocus {
            compositor_focused,
            keyboard_focused: self.is_active(),
        }
        .takes_controller()
    }

    /// Left and right turn the focused slider a step, as in the main window; says whether
    /// they did.
    fn adjust_slider(&self, direction: Direction) -> bool {
        let step = match direction {
            Direction::Left => gtk::ScrollType::StepLeft,
            Direction::Right => gtk::ScrollType::StepRight,
            Direction::Up | Direction::Down => return false,
        };
        let Some(slider) = gtk::prelude::GtkWindowExt::focus(self).and_downcast::<gtk::Range>()
        else {
            return false;
        };
        slider.emit_move_slider(step);
        true
    }

    /// Moves focus as the arrow keys would, keeping it where it was when nothing lies that
    /// way.
    fn navigate(&self, direction: Direction) {
        let previous = gtk::prelude::GtkWindowExt::focus(self);
        self.emit_move_focus(match direction {
            Direction::Up => gtk::DirectionType::Up,
            Direction::Down => gtk::DirectionType::Down,
            Direction::Left => gtk::DirectionType::Left,
            Direction::Right => gtk::DirectionType::Right,
        });
        if gtk::prelude::GtkWindowExt::focus(self).is_none()
            && let Some(previous) = previous
        {
            previous.grab_focus();
        }
    }

    /// Plays a navigation sound as the main window does.
    pub(crate) fn play_sound(&self, sound: Sound) {
        if let Some(window) = self.imp().main_window.upgrade() {
            window.play_sound(sound);
        }
    }

    /// Shows the hints for where focus is in the open panel while a controller is in use,
    /// under that panel, and hides the bars otherwise.
    pub(crate) fn update_hints(&self) {
        let imp = self.imp();
        let layout = imp
            .application
            .upgrade()
            .and_then(|application| application.gamepad_service().active_layout());
        let panel = imp.panels.get().open();
        let on_drawer = matches!(panel, Some(Panel::Drawer(_)));
        imp.drawer_hint_bar
            .set_visible(layout.is_some() && on_drawer);
        imp.hint_bar.set_visible(layout.is_some() && !on_drawer);
        let Some(layout) = layout else {
            return;
        };
        let context = Context {
            focus: gtk::prelude::GtkWindowExt::focus(self)
                .as_ref()
                .map_or(Focus::Nothing, focus_kind),
            dialog: self.visible_dialog().is_some(),
            page_below: false,
            search: false,
            tabs: false,
            dialog_tabs: false,
            options: false,
            menu: false,
            panel,
            launching: Launching::No,
            keyboard: self.secret_dialog_typing(),
        };
        let shown = Some((on_drawer, layout, hints::hints(context, layout)));
        if *imp.hints.borrow() == shown {
            return;
        }
        if let Some((_, layout, hints)) = &shown {
            let bar = if on_drawer {
                &imp.drawer_hint_bar
            } else {
                &imp.hint_bar
            };
            fill_hint_bar(bar, hints, *layout);
        }
        imp.hints.replace(shown);
    }
}
