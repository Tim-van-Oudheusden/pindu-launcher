//! Quick Access over a running game in a `--steam` gamescope: [`PinduQuickAccessOverlay`],
//! a second toplevel that hosts the Quick Access widget (`quick_access`) with its own
//! Blueprint (`data/ui/quick_access_overlay.blp`). The application makes one when the
//! gamescope service speaks the protocol; the service marks it as gamescope's overlay and
//! opens and closes it (Guide + A or the Steam Deck's "…" with a game in front, Guide, the
//! game in front changing), and this window follows: it shows the panel's levels as the
//! window does, for the game in front, and acts on the controller while it holds input.
//!
//! Over a game the panel offers what needs no library page and no flow of the main window
//! ([`PinduQuickAccess::set_over_game`]). Network's passphrase and Bluetooth's PIN and
//! codes are asked here while the panel is open (`secret_dialog`), with the dialog's
//! keyboard taking the controller and its buttons in this window's hint bar. Playing Now's
//! Return and Stop, the volume preview and a notification's link go to the main window,
//! which a link brings to the front. The controller still reaches the game while the panel
//! is open: only the OS can take it away from the game.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::gamepad::{Direction, Intent, Layout, WindowFocus};
use pindu_core::hints::{self, Context, Focus, Hint, Launching};
use pindu_core::launch;
use pindu_core::model::GameId;
use pindu_core::panels::{Panel, Panels, QuickAccessLevel, Under, page_entries};

use crate::application::PinduApplication;
use crate::fullscreen::Sound;
use crate::hint_bar::{fill_hint_bar, focus_kind};
use crate::quick_access::PinduQuickAccess;
use crate::window::PinduWindow;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/quick_access_overlay.ui"
    )]
    pub struct PinduQuickAccessOverlay {
        #[template_child]
        pub shade: TemplateChild<gtk::Box>,
        #[template_child]
        pub quick_access: TemplateChild<PinduQuickAccess>,
        #[template_child]
        pub hint_bar: TemplateChild<gtk::Box>,
        pub application: glib::WeakRef<PinduApplication>,
        /// The main window, which keeps Playing Now's actions and shows what links lead to.
        pub main_window: glib::WeakRef<PinduWindow>,
        /// Quick Access at its level while it is open over the game.
        pub panels: Cell<Panels>,
        /// The hints shown, with the layout they were drawn for.
        pub hints: RefCell<Option<(Layout, Vec<Hint>)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduQuickAccessOverlay {
        const NAME: &'static str = "PinduQuickAccessOverlay";
        type Type = super::PinduQuickAccessOverlay;
        type ParentType = adw::Window;

        fn class_init(klass: &mut Self::Class) {
            PinduQuickAccess::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduQuickAccessOverlay {}

    impl WidgetImpl for PinduQuickAccessOverlay {}

    impl WindowImpl for PinduQuickAccessOverlay {}

    impl AdwWindowImpl for PinduQuickAccessOverlay {}
}

glib::wrapper! {
    /// The window Quick Access opens in over a running game in a `--steam` gamescope.
    pub struct PinduQuickAccessOverlay(ObjectSubclass<imp::PinduQuickAccessOverlay>)
        @extends adw::Window, gtk::Window, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native,
            gtk::Root, gtk::ShortcutManager;
}

impl PinduQuickAccessOverlay {
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
        overlay.add_main_window_actions(&actions);
        overlay.connect_quick_access();
        let gamescope = application.gamescope_service();
        gamescope.connect_quick_access_changed(glib::clone!(
            #[weak]
            overlay,
            move |open| overlay.set_open(open)
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

    /// Does what the panel asks for: close, show a level, leave Activity, or show a game,
    /// which brings the main window to the front on the game's page.
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
            move |_, level| {
                let mut panels = overlay.imp().panels.get();
                panels.show(Panel::QuickAccess(level));
                overlay.set_panels(panels);
            }
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
            application.gamescope_service().set_quick_access_open(false);
        }
    }

    /// Closes the panel and brings the launcher to the front, for what a link leads to.
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

    /// The gamescope service opened or closed the panel: it opens at the notifications
    /// while one is unread, or at its entries, with Performance for the game in front.
    fn set_open(&self, open: bool) {
        let imp = self.imp();
        let mut panels = Panels::new();
        if open {
            let game = imp.application.upgrade().and_then(|application| {
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
        self.set_panels(panels);
    }

    /// Shows Quick Access at the level `panels` has, focusing it once it shows.
    fn set_panels(&self, panels: Panels) {
        let imp = self.imp();
        let before = imp.panels.replace(panels).open();
        let level = match panels.open() {
            Some(Panel::QuickAccess(level)) => Some(level),
            _ => None,
        };
        if before == panels.open() {
            return;
        }
        imp.quick_access.show_level(level);
        if level.is_some() {
            let left = match before {
                Some(Panel::QuickAccess(level)) => Some(level),
                _ => None,
            };
            // Once the level shows and can take focus.
            glib::idle_add_local_once(glib::clone!(
                #[weak(rename_to = overlay)]
                self,
                move || overlay.imp().quick_access.focus_level(left)
            ));
        } else if let Some(dialog) = self.visible_dialog() {
            dialog.force_close();
        }
        self.update_hints();
    }

    /// Acts on a controller intent while the panel is open and holds input: an open
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

    /// Shows the hints for where focus is in the panel while a controller is in use, and
    /// hides the bar otherwise.
    pub(crate) fn update_hints(&self) {
        let imp = self.imp();
        let layout = imp
            .application
            .upgrade()
            .and_then(|application| application.gamepad_service().active_layout());
        imp.hint_bar.set_visible(layout.is_some());
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
            options: false,
            menu: false,
            panel: imp.panels.get().open(),
            launching: Launching::No,
            keyboard: self.secret_dialog_typing(),
        };
        let shown = Some((layout, hints::hints(context, layout)));
        if *imp.hints.borrow() == shown {
            return;
        }
        if let Some((layout, hints)) = &shown {
            fill_hint_bar(&imp.hint_bar, hints, *layout);
        }
        imp.hints.replace(shown);
    }
}
