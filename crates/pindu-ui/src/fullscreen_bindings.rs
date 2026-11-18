//! The menu drawer's Controls (`pindu_core::bindings`): its entry, and its level listing the
//! functions a button can be bound to, each with its button on the controller in use, and
//! Reset to Defaults. Choosing a function waits for the next press of a button that can be
//! bound and binds the function to it, swapping with the function that button had
//! (`GamepadService::start_binding`); Guide, the D-pad, the stick or leaving the level ends
//! the wait. The bindings are kept in the `button-bindings` setting, so they apply at once
//! to the intents, the hints and Quick Access's Controls, and outlast a restart.

use adw::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::bindings::function_named;
use pindu_core::gamepad::{DeviceKind, Layout};
use pindu_core::panels::{DrawerLevel, Panel};

use crate::application::PinduApplication;
use crate::gamepad_service::GamepadService;
use crate::window::PinduWindow;

impl PinduWindow {
    /// Adds the entry's and the level's actions, and fills the level again as the
    /// controller in use, the bindings or the wait for a button change.
    pub(crate) fn setup_bindings(&self, application: &PinduApplication) {
        let controls = gio::ActionEntry::builder("drawer-controls")
            .activate(|window: &Self, _, _| {
                window.show_panel(Panel::Drawer(DrawerLevel::Controls));
            })
            .build();
        let bind = gio::ActionEntry::builder("bind-button")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|window: &Self, _, target| {
                let function = target.and_then(|name| name.str()).and_then(function_named);
                if let (Some(function), Some(service)) = (function, window.gamepad_service()) {
                    service.start_binding(function);
                }
            })
            .build();
        let reset = gio::ActionEntry::builder("reset-bindings")
            .activate(|window: &Self, _, _| {
                if let Some(service) = window.gamepad_service() {
                    service.reset_bindings();
                }
            })
            .build();
        self.add_action_entries([controls, bind, reset]);
        let service = application.gamepad_service();
        // After the row that started the wait or reset the bindings is done activating:
        // filling the level again replaces its rows.
        let refill = |window: &Self| {
            glib::idle_add_local_once(glib::clone!(
                #[weak]
                window,
                move || window.fill_bindings()
            ));
        };
        service.connect_active_layout_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| refill(&window)
        ));
        service.connect_binding_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| refill(&window)
        ));
    }

    /// Fills the Controls level while it is open: each function's button on the
    /// controller in use, or on a generic pad without one.
    pub(crate) fn fill_bindings(&self) {
        if self.open_panel() != Some(Panel::Drawer(DrawerLevel::Controls)) {
            return;
        }
        let Some(service) = self.gamepad_service() else {
            return;
        };
        let layout = service
            .active_layout()
            .unwrap_or_else(|| Layout::resolve(DeviceKind::Generic, service.controller_settings()));
        self.imp()
            .drawer_panel
            .fill_bindings(layout, service.binding());
    }

    /// The level was left: a function waiting for a button waits no more.
    pub(crate) fn stop_binding(&self) {
        if let Some(service) = self.gamepad_service() {
            service.stop_binding();
        }
    }

    fn gamepad_service(&self) -> Option<GamepadService> {
        self.application()
            .and_downcast::<PinduApplication>()
            .map(|application| application.gamepad_service().clone())
    }
}
