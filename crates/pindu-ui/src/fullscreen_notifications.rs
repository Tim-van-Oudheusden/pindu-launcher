//! Quick Access's notifications: the unread count on the Notifications section's toggle,
//! and the section's list of what happened in the background this session (see
//! `notification_service`), newest first, marked read once shown. An entry with a target
//! opens it on A: a game's page, or Preferences at its Permissions group.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::model::GameId;
use pindu_core::notifications::{Kind, Notification, Target};
use pindu_core::panels::QuickAccessSection;

use crate::application::PinduApplication;
use crate::quick_access::PinduQuickAccess;

impl PinduQuickAccess {
    /// The entries' targets, and following the notifications: the toggle's count, and the
    /// list while Notifications shows.
    pub(crate) fn setup_notifications(
        &self,
        application: &PinduApplication,
        actions: &impl IsA<gio::ActionMap>,
    ) {
        self.add_host_action(
            actions,
            "notification-game",
            Some(glib::VariantTy::STRING),
            |quick_access, target| {
                let id = target
                    .and_then(|target| target.str())
                    .and_then(|id| id.parse::<GameId>().ok());
                if let Some(id) = id {
                    quick_access.request_game(&id);
                }
            },
        );
        self.add_host_action(
            actions,
            "notification-permissions",
            None,
            |quick_access, _| {
                quick_access.request_close();
                if let Some(application) = quick_access.application() {
                    application.activate_action("permissions", None);
                }
            },
        );
        application
            .notification_service()
            .connect_changed(glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move || quick_access.notifications_changed()
            ));
        self.show_unread_count();
    }

    /// Fills the Notifications list, newest first, and marks them read: the user sees them
    /// now. Unread ones stand out until the list is filled again.
    pub(crate) fn show_notifications(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let list = self.imp().notifications.get();
        let had_focus = self
            .focus_widget()
            .is_some_and(|focus| focus.is_ancestor(&list));
        list.remove_all();
        let service = application.notification_service();
        let entries = service.entries();
        if entries.is_empty() {
            let row = adw::ActionRow::builder()
                .title(gettext("No Notifications"))
                .subtitle(gettext(
                    "Games found by a sync, artwork, Proton downloads and games that need a permission show here",
                ))
                .build();
            list.append(&row);
        }
        for entry in &entries {
            list.append(&self.notification_row(entry));
        }
        service.mark_all_read();
        if had_focus && let Some(row) = list.row_at_index(0) {
            row.grab_focus();
        }
    }

    /// A posted notification, or the log read: the count follows, and the section shows a
    /// new one while it shows.
    fn notifications_changed(&self) {
        let open = self.shows_section(QuickAccessSection::Notifications);
        if open && self.has_unread_notifications() {
            self.show_notifications();
        }
        self.show_unread_count();
    }

    /// The Notifications toggle's count of unread notifications, hidden at none.
    fn show_unread_count(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let unread = application.notification_service().unread();
        let label = &self.imp().unread;
        label.set_visible(unread > 0);
        label.set_label(&unread.to_string());
        let tab = &self.imp().notifications_tab;
        // Translators: how many notifications are unread; {count} is a number.
        let description = gettext("{count} unread").replace("{count}", &unread.to_string());
        if unread > 0 {
            tab.update_property(&[gtk::accessible::Property::Description(&description)]);
        } else {
            tab.reset_property(gtk::AccessibleProperty::Description);
        }
    }

    /// A row for `entry`: its icon, title, body and time, marked while unread, and opening
    /// its target, if it has one, when activated.
    fn notification_row(&self, entry: &Notification) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(&entry.title)
            .subtitle(&entry.body)
            .use_markup(false)
            .activatable(entry.target.is_some())
            .css_classes(["notification"])
            .build();
        let icon = gtk::Image::builder()
            .icon_name(kind_icon(entry.kind))
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build();
        row.add_prefix(&icon);
        if !entry.read {
            row.add_css_class("unread");
            let dot = gtk::Image::builder()
                .valign(gtk::Align::Center)
                .css_classes(["unread-dot"])
                .build();
            dot.update_property(&[gtk::accessible::Property::Label(&gettext("Unread"))]);
            row.add_prefix(&dot);
        }
        if let (Ok(time), Some(clock)) = (
            glib::DateTime::from_unix_local(entry.time),
            self.imp().time.get(),
        ) {
            let label = gtk::Label::builder()
                .label(clock.text_at(&time))
                .valign(gtk::Align::Center)
                .css_classes(["dim-label", "numeric"])
                .build();
            row.add_suffix(&label);
        }
        if let Some(target) = &entry.target {
            let arrow = gtk::Image::builder()
                .icon_name("go-next-symbolic")
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build();
            row.add_suffix(&arrow);
            match target {
                Target::Game(id) => {
                    row.set_action_name(Some("win.notification-game"));
                    row.set_action_target_value(Some(&id.as_str().to_variant()));
                }
                Target::Permissions => {
                    row.set_action_name(Some("win.notification-permissions"));
                }
                Target::OpenStore(app) => {
                    row.set_action_name(Some("win.add-store-open"));
                    row.set_action_target_value(Some(&app.app_id().to_variant()));
                }
            }
        }
        row
    }
}

/// The icon of a notification of `kind`.
fn kind_icon(kind: Kind) -> &'static str {
    match kind {
        Kind::Library => "applications-games-symbolic",
        Kind::Artwork => "image-x-generic-symbolic",
        Kind::ArtworkFailed => "image-missing-symbolic",
        Kind::Metadata => "text-x-generic-symbolic",
        Kind::MetadataFailed => "dialog-warning-symbolic",
        Kind::Download => "folder-download-symbolic",
        Kind::DownloadFailed => "dialog-warning-symbolic",
        Kind::Permission => "system-lock-screen-symbolic",
        Kind::SystemFailed => "dialog-warning-symbolic",
        Kind::BluetoothFailed => "bluetooth-disabled-symbolic",
        Kind::NetworkFailed => "network-offline-symbolic",
        Kind::PowerInhibited => "system-shutdown-symbolic",
        Kind::SoundOutputFailed => "dialog-warning-symbolic",
        Kind::StoreInstalled => "system-software-install-symbolic",
        Kind::StoreFailed => "dialog-warning-symbolic",
        Kind::SaveCopyFailed => "dialog-warning-symbolic",
    }
}
