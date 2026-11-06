//! Quick Access's notifications: the Notifications entry with the unread count, and its
//! level, which lists what happened in the background this session (see
//! `notification_service`), newest first, and marks it read once shown. An entry with a
//! target opens it on A: a game's page, or Preferences at its Permissions group.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::notifications::{Kind, Notification, Target};
use pindu_core::panels::{Panel, QuickAccessLevel};

use crate::application::PinduApplication;
use crate::window::PinduWindow;

impl PinduWindow {
    /// The entries' targets, and following the notifications: the entry's count, and the
    /// level while it shows.
    pub(crate) fn setup_notifications(&self, application: &PinduApplication) {
        self.add_game_action("notification-game", |window, id| {
            window.close_panel();
            window.return_to_game(id);
        });
        let permissions = gio::ActionEntry::builder("notification-permissions")
            .activate(|window: &Self, _, _| {
                window.close_panel();
                if let Some(application) = window.application() {
                    application.activate_action("permissions", None);
                }
            })
            .build();
        self.add_action_entries([permissions]);
        application
            .notification_service()
            .connect_changed(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || window.notifications_changed()
            ));
        self.show_unread_count();
    }

    /// Whether a notification is unread, which has Quick Access open at its notifications.
    pub(crate) fn has_unread_notifications(&self) -> bool {
        self.application()
            .and_downcast::<PinduApplication>()
            .is_some_and(|application| application.notification_service().unread() > 0)
    }

    /// Fills the Notifications level, newest first, and marks them read: the user sees
    /// them now. Unread ones stand out until the level is filled again.
    pub(crate) fn show_notifications(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let list = self.imp().quick_access_notifications.get();
        let had_focus =
            gtk::prelude::GtkWindowExt::focus(self).is_some_and(|focus| focus.is_ancestor(&list));
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

    /// A posted notification, or the log read: the count follows, and the level shows a
    /// new one while it is open.
    fn notifications_changed(&self) {
        let open = self.open_panel() == Some(Panel::QuickAccess(QuickAccessLevel::Notifications));
        if open && self.has_unread_notifications() {
            self.show_notifications();
        }
        self.show_unread_count();
    }

    /// The Notifications entry's count of unread notifications, hidden at none.
    fn show_unread_count(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let unread = application.notification_service().unread();
        let label = &self.imp().quick_access_unread;
        label.set_visible(unread > 0);
        label.set_label(&unread.to_string());
        let row = &self.imp().quick_access_notifications_row;
        // Translators: how many notifications are unread; {count} is a number.
        let description = gettext("{count} unread").replace("{count}", &unread.to_string());
        if unread > 0 {
            row.update_property(&[gtk::accessible::Property::Description(&description)]);
        } else {
            row.reset_property(gtk::AccessibleProperty::Description);
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
        if let Ok(time) = glib::DateTime::from_unix_local(entry.time) {
            let label = gtk::Label::builder()
                .label(self.clock_text_at(&time))
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
        Kind::Download => "folder-download-symbolic",
        Kind::DownloadFailed => "dialog-warning-symbolic",
        Kind::Permission => "system-lock-screen-symbolic",
    }
}
