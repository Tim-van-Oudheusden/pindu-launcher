//! The "add a store" flow (`pindu_core::add_store`) in both shells: Quick Access's Add a
//! Store level in fullscreen mode, and the Add a Store dialog on the desktop, which the
//! empty library offers too. Both are a [`PinduAddStoreLevel`], with its own Blueprint
//! (`data/ui/add_store.blp`), filled from [`AddStoreService`]'s state; the window keeps
//! the actions their rows call and the dialog, opens the flow as the mode needs, and Quick
//! Access fills its level. Every row is a list row, so a controller alone gets through the
//! flow with Up, Down, A and B.

use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::add_store::{AddStoreState, Method, Step, StoreApp};
use pindu_core::panels::{Panel, QuickAccessLevel};

use crate::add_store_service::{
    AddStoreService, network_failure, next_step, store_description, store_name,
};
use crate::application::PinduApplication;
use crate::quick_access::PinduQuickAccess;
use crate::window::PinduWindow;

/// What the level's rows show, apart from how far an install is: rows are built again
/// only when this changes, so focus stays on Cancel while the bar moves. The last two are
/// whether the machine is offline and what the level offers.
type Shape = (Method, Vec<StoreApp>, Step, bool, Offers);

/// The actions only fullscreen mode's level offers, which lead on from the flow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Offers {
    /// Network, while the machine is offline or the network failed an install, which
    /// leads to Quick Access's Network level where the system panel has one.
    pub(crate) network: bool,
    /// Open Steam once it installed, which opens it in Big Picture for its first sign-in.
    pub(crate) open_steam: bool,
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/add_store.ui")]
    pub struct PinduAddStoreLevel {
        #[template_child]
        pub add_store_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub add_store_sandbox_note: TemplateChild<gtk::Label>,
        #[template_child]
        pub add_store_stores: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub add_store_status: TemplateChild<gtk::Box>,
        #[template_child]
        pub add_store_status_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub add_store_progress: TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub add_store_status_detail: TemplateChild<gtk::Label>,
        #[template_child]
        pub add_store_actions: TemplateChild<gtk::ListBox>,
        /// What the rows show now.
        pub(super) shape: RefCell<Option<Shape>>,
        /// The actions it offers besides the flow's own.
        pub(super) offers: std::cell::Cell<Offers>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduAddStoreLevel {
        const NAME: &'static str = "PinduAddStoreLevel";
        type Type = super::PinduAddStoreLevel;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduAddStoreLevel {}

    impl WidgetImpl for PinduAddStoreLevel {}

    impl BinImpl for PinduAddStoreLevel {}
}

glib::wrapper! {
    /// The stores to install, the install that runs or how it ended, and what to do next.
    /// It is the `"add-store"` page of Quick Access's stack, and the content of the
    /// desktop's Add a Store dialog.
    pub struct PinduAddStoreLevel(ObjectSubclass<imp::PinduAddStoreLevel>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for PinduAddStoreLevel {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl PinduWindow {
    /// Adds the flow's actions, and fills its level and dialog as the flow changes.
    pub(crate) fn setup_add_store(&self) {
        let open = gio::ActionEntry::builder("add-store")
            .activate(|window: &Self, _, _| window.open_add_store())
            .build();
        let choose = gio::ActionEntry::builder("add-store-choose")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|window: &Self, _, target| {
                let app = target
                    .and_then(|target| target.str())
                    .and_then(|id| StoreApp::ALL.into_iter().find(|app| app.app_id() == id));
                if let (Some(service), Some(app)) = (window.add_store_service(), app) {
                    service.choose(app);
                }
            })
            .build();
        let cancel = gio::ActionEntry::builder("add-store-cancel")
            .activate(|window: &Self, _, _| {
                if let Some(service) = window.add_store_service() {
                    service.cancel();
                }
            })
            .build();
        let back = gio::ActionEntry::builder("add-store-back")
            .activate(|window: &Self, _, _| {
                if let Some(service) = window.add_store_service() {
                    service.dismiss();
                }
            })
            .build();
        let done = gio::ActionEntry::builder("add-store-done")
            .activate(|window: &Self, _, _| window.leave_add_store())
            .build();
        let open_steam = gio::ActionEntry::builder("add-store-open-steam")
            .activate(|window: &Self, _, _| window.open_installed_steam())
            .build();
        self.add_action_entries([open, choose, cancel, back, done, open_steam]);
        let Some(service) = self.add_store_service() else {
            return;
        };
        service.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |state| window.show_add_store_state(state)
        ));
    }

    /// Opens the flow: Quick Access's level in fullscreen mode, a dialog on the desktop.
    fn open_add_store(&self) {
        if self.is_fullscreen() {
            self.show_panel(Panel::QuickAccess(QuickAccessLevel::AddStore));
            return;
        }
        if self.imp().add_store_dialog.borrow().is_some() {
            return;
        }
        let Some(service) = self.add_store_service() else {
            return;
        };
        let level = PinduAddStoreLevel::default();
        level.set_title_visible(false);
        level.set_margin_top(12);
        level.set_margin_bottom(24);
        level.set_margin_start(12);
        level.set_margin_end(12);
        level.fill(&service.state());
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&level));
        let dialog = adw::Dialog::builder()
            .title(gettext("Add a Store"))
            .content_width(480)
            .child(&toolbar)
            .build();
        dialog.connect_closed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.imp().add_store_dialog.replace(None);
            }
        ));
        self.imp().add_store_dialog.replace(Some(level.clone()));
        dialog.present(Some(self));
        if let Some(target) = level.focus_target() {
            target.grab_focus();
        }
        service.refresh();
    }

    /// Show Library: the flow goes back to its stores, and the library shows.
    fn leave_add_store(&self) {
        if let Some(service) = self.add_store_service() {
            service.dismiss();
        }
        let dialog = self
            .imp()
            .add_store_dialog
            .borrow()
            .as_ref()
            .and_then(|level| level.ancestor(adw::Dialog::static_type()))
            .and_downcast::<adw::Dialog>();
        if let Some(dialog) = dialog {
            dialog.close();
        }
        if self.is_fullscreen() {
            self.close_panel();
            self.show_library();
        }
    }

    /// Open Steam, once Add a Store installed it: the flow goes back to its stores, Quick
    /// Access closes, and Steam opens in Big Picture, as Home's tile opens it, for its
    /// first sign-in.
    fn open_installed_steam(&self) {
        if let Some(service) = self.add_store_service() {
            service.dismiss();
        }
        self.close_panel();
        if let Err(error) = WidgetExt::activate_action(self, "win.steam-big-picture", None) {
            glib::g_warning!("pindu", "Could not open Steam: {error}");
        }
    }

    fn add_store_service(&self) -> Option<AddStoreService> {
        self.application()
            .and_downcast::<PinduApplication>()
            .map(|application| application.add_store_service().clone())
    }

    /// Fills the dialog while it shows, keeping focus in it when its rows are built again.
    fn show_add_store_state(&self, state: &AddStoreState) {
        let dialog = self.imp().add_store_dialog.borrow().clone();
        if let Some(level) = dialog {
            let had_focus = level.has_focus();
            if level.fill(state)
                && had_focus
                && let Some(target) = level.focus_target()
            {
                target.grab_focus();
            }
        }
    }
}

impl PinduQuickAccess {
    /// Fills the level as the flow changes while it is open.
    pub(crate) fn setup_add_store(&self, application: &PinduApplication) {
        application
            .add_store_service()
            .connect_changed(glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move |state| quick_access.show_add_store_state(state)
            ));
    }

    /// The level opens: it shows the flow as it is, and the installed stores are looked
    /// for again.
    pub(crate) fn show_add_store(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let service = application.add_store_service();
        let level = &self.imp().add_store_level;
        level.set_offers(self.add_store_offers());
        level.fill(&service.state());
        service.refresh();
    }

    /// Fills the level while it is open, keeping focus in it when its rows are built
    /// again.
    fn show_add_store_state(&self, state: &AddStoreState) {
        if self.level() != Some(QuickAccessLevel::AddStore) {
            return;
        }
        let level = &self.imp().add_store_level;
        level.set_offers(self.add_store_offers());
        let had_focus = level.has_focus();
        if level.fill(state) && had_focus {
            self.focus_level(None);
        }
    }

    /// What fullscreen mode's level offers: Open Steam, and Network where the system
    /// panel has it.
    fn add_store_offers(&self) -> Offers {
        Offers {
            network: self.imp().shown.get().network,
            open_steam: true,
        }
    }
}

impl PinduAddStoreLevel {
    /// The level's lists, whose Up and Down go on to the next list at their ends.
    pub(crate) fn lists(&self) -> [gtk::ListBox; 2] {
        let imp = self.imp();
        [imp.add_store_stores.get(), imp.add_store_actions.get()]
    }

    /// Shows or hides the level's own title: the dialog has it in its header bar.
    pub(crate) fn set_title_visible(&self, visible: bool) {
        self.imp().add_store_title.set_visible(visible);
    }

    /// Offers the actions in `offers` from the next fill.
    pub(crate) fn set_offers(&self, offers: Offers) {
        self.imp().offers.set(offers);
    }

    /// Where focus goes: the first action, such as Cancel or Show Library, or else the
    /// first store that can be chosen, or else the first store.
    pub(crate) fn focus_target(&self) -> Option<gtk::Widget> {
        let imp = self.imp();
        let actions = &imp.add_store_actions;
        if actions.is_visible()
            && let Some(row) = actions.row_at_index(0)
        {
            return Some(row.upcast());
        }
        let stores = &imp.add_store_stores;
        let rows = || {
            std::iter::successors(stores.row_at_index(0), |row| {
                stores.row_at_index(row.index() + 1)
            })
        };
        rows()
            .find(|row| row.is_sensitive())
            .or_else(|| rows().next())
            .map(|row| row.upcast())
    }

    /// Whether focus is on one of the level's rows, which filling it again takes away.
    pub(crate) fn has_focus(&self) -> bool {
        self.root()
            .and_then(|root| gtk::prelude::RootExt::focus(&root))
            .is_some_and(|focus| self.lists().iter().any(|list| focus.is_ancestor(list)))
    }

    /// Shows `state`; `true` when its rows were built again, which takes focus away.
    pub(crate) fn fill(&self, state: &AddStoreState) -> bool {
        let imp = self.imp();
        if let Step::Installing { progress, .. } = state.step {
            imp.add_store_progress.set_fraction(progress);
        }
        let shape = (
            state.method,
            state.installed.clone(),
            shape_step(&state.step),
            state.offline,
            imp.offers.get(),
        );
        if imp.shape.borrow().as_ref() == Some(&shape) {
            return false;
        }
        imp.shape.replace(Some(shape));
        imp.add_store_sandbox_note
            .set_visible(state.method == Method::SoftwareCentre);
        let stores = &imp.add_store_stores;
        stores.remove_all();
        for app in StoreApp::ALL {
            stores.append(&store_row(app, state));
        }
        self.fill_status(state);
        true
    }

    /// The status under the stores, and the actions for it.
    fn fill_status(&self, state: &AddStoreState) {
        let imp = self.imp();
        let actions = &imp.add_store_actions;
        actions.remove_all();
        let (title, detail) = match &state.step {
            Step::Choose if state.offline => {
                if imp.offers.get().network {
                    actions.append(&network_row());
                }
                (
                    Some(gettext("No Network Connection")),
                    Some(gettext(
                        "Stores install from the internet. Connect to a network, then choose a store.",
                    )),
                )
            }
            Step::Choose => (None, None),
            Step::Installing {
                app, cancelling, ..
            } => {
                if *cancelling {
                    actions.append(&busy_row(&gettext("Cancelling…")));
                } else {
                    actions.append(&action_row(
                        &gettext("Cancel"),
                        "win.add-store-cancel",
                        None,
                        "process-stop-symbolic",
                    ));
                }
                (
                    // Translators: {store} is a store's name, such as Steam.
                    Some(gettext("Installing {store}…").replace("{store}", store_name(*app))),
                    Some(gettext(
                        "This can take a while. You can leave this page: the install goes on, and shows in Activity.",
                    )),
                )
            }
            Step::Installed(app) => {
                if *app == StoreApp::Steam && imp.offers.get().open_steam {
                    actions.append(&action_row(
                        // Translators: {store} is a store's name, such as Steam.
                        &gettext("Open {store}").replace("{store}", store_name(*app)),
                        "win.add-store-open-steam",
                        None,
                        "go-next-symbolic",
                    ));
                }
                actions.append(&action_row(
                    &gettext("Show Library"),
                    "win.add-store-done",
                    None,
                    "view-grid-symbolic",
                ));
                (
                    // Translators: {store} is a store's name, such as Steam.
                    Some(gettext("{store} Is Installed").replace("{store}", store_name(*app))),
                    Some(next_step(*app)),
                )
            }
            Step::Opened(app) => {
                actions.append(&action_row(
                    &gettext("Refresh Library"),
                    "app.import",
                    None,
                    "view-refresh-symbolic",
                ));
                actions.append(&action_row(
                    &gettext("Show Library"),
                    "win.add-store-done",
                    None,
                    "view-grid-symbolic",
                ));
                (
                    // Translators: {store} is a store's name, such as Steam.
                    Some(
                        gettext("Install {store} in Your Software Centre")
                            .replace("{store}", store_name(*app)),
                    ),
                    Some(
                        // Translators: {store} is a store's name, such as Steam.
                        gettext(
                            "Once {store} is installed, open it and sign in to your account, then refresh the library.",
                        )
                        .replace("{store}", store_name(*app)),
                    ),
                )
            }
            Step::Failed {
                app,
                error,
                network,
            } => {
                if state.can_choose(*app) {
                    let id = app.app_id().to_variant();
                    actions.append(&action_row(
                        &gettext("Try Again"),
                        "win.add-store-choose",
                        Some(&id),
                        "view-refresh-symbolic",
                    ));
                }
                if (*network || state.offline) && imp.offers.get().network {
                    actions.append(&network_row());
                }
                actions.append(&action_row(
                    &gettext("Back to the Stores"),
                    "win.add-store-back",
                    None,
                    "go-previous-symbolic",
                ));
                let title = match state.method {
                    // Translators: {store} is a store's name, such as Steam.
                    Method::Flatpak => gettext("{store} Did Not Install"),
                    // Translators: {store} is a store's name, such as Steam.
                    Method::SoftwareCentre => {
                        gettext("Could Not Open {store} in Your Software Centre")
                    }
                };
                let detail = if *network {
                    network_failure(*app)
                } else {
                    error.clone()
                };
                (
                    Some(title.replace("{store}", store_name(*app))),
                    Some(detail),
                )
            }
        };
        imp.add_store_status.set_visible(title.is_some());
        imp.add_store_status_title
            .set_label(title.as_deref().unwrap_or_default());
        imp.add_store_status_detail
            .set_label(detail.as_deref().unwrap_or_default());
        imp.add_store_progress
            .set_visible(matches!(state.step, Step::Installing { .. }));
        actions.set_visible(actions.row_at_index(0).is_some());
    }
}

/// `step` without how far an install is.
fn shape_step(step: &Step) -> Step {
    match step {
        Step::Installing {
            app, cancelling, ..
        } => Step::Installing {
            app: *app,
            progress: 0.0,
            cancelling: *cancelling,
        },
        step => step.clone(),
    }
}

/// The row that leads to Quick Access's Network level.
fn network_row() -> adw::ButtonRow {
    action_row(
        &gettext("Network"),
        "win.quick-access-network",
        None,
        "network-wireless-symbolic",
    )
}

/// A store's row: its name and what it is for, chosen with A, or marked installed.
fn store_row(app: StoreApp, state: &AddStoreState) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(store_name(app))
        .subtitle(store_description(app))
        .use_markup(false)
        .build();
    if state.installed.contains(&app) {
        let label = gtk::Label::builder()
            .label(gettext("Installed"))
            .valign(gtk::Align::Center)
            .css_classes(["dim-label"])
            .build();
        row.add_suffix(&label);
        row.set_activatable(false);
        return row;
    }
    let icon = match state.method {
        Method::Flatpak => "folder-download-symbolic",
        Method::SoftwareCentre => "adw-external-link-symbolic",
    };
    row.add_suffix(&gtk::Image::from_icon_name(icon));
    row.set_activatable(true);
    row.set_action_name(Some("win.add-store-choose"));
    row.set_action_target_value(Some(&app.app_id().to_variant()));
    row.set_sensitive(state.can_choose(app));
    row
}

/// A row that does what `action` says.
fn action_row(
    title: &str,
    action: &str,
    target: Option<&glib::Variant>,
    icon: &str,
) -> adw::ButtonRow {
    let row = adw::ButtonRow::builder()
        .title(title)
        .use_markup(false)
        .start_icon_name(icon)
        .action_name(action)
        .build();
    if let Some(target) = target {
        row.set_action_target_value(Some(target));
    }
    row
}

/// A row saying what runs, with a spinner.
fn busy_row(title: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    row.add_suffix(&adw::Spinner::new());
    row
}
