//! Apps (#830) in both shells: the installed applications that are not games
//! (`pindu_core::apps`, kept by `AppsService`), a tile each with its icon and name, which
//! opens its app (`launch_service::open_app`). The desktop window's sidebar has an Apps
//! item that shows the page in the library's place, with an empty state that says what it
//! lists; fullscreen mode's Home has an Apps tile, while there are apps, that shows it
//! above Home, where A opens the focused app and B goes back.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use pindu_core::apps::App;

use crate::application::PinduApplication;
use crate::home_page::APPS_ICON;
use crate::launch_service;
use crate::widgets::textures;
use crate::window::PinduWindow;

/// The side of an app's icon on its tile, in pixels.
const ICON_SIZE: i32 = 64;

/// How wide an app's name may be on its tile, in characters, before it wraps.
const NAME_CHARS: i32 = 14;

impl PinduWindow {
    /// The Apps page and the sidebar's Apps item, filled now and again whenever the
    /// installed apps change; Home learns how many there are.
    pub(crate) fn setup_apps(&self, application: &PinduApplication) {
        let imp = self.imp();
        imp.apps_grid.connect_child_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, child| window.open_app_at(child.index())
        ));
        application.apps_service().connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move || window.show_installed_apps()
        ));
        self.show_installed_apps();
        // The page names itself where fullscreen mode hides its header bar.
        self.connect_fullscreened_notify(|window| {
            window.imp().apps_title.set_visible(window.is_fullscreen());
        });
    }

    /// Shows the installed apps on the Apps page, or its empty state without any, their
    /// number beside the sidebar's item, and tells Home. Focus on a tile stays at its
    /// place.
    fn show_installed_apps(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let apps = application.apps_service().apps();
        let imp = self.imp();
        let count = u32::try_from(apps.len()).unwrap_or(u32::MAX);
        imp.apps_stack
            .set_visible_child_name(if apps.is_empty() { "empty" } else { "apps" });
        imp.apps_count.set_label(&count.to_string());
        imp.home_page.set_apps(count);
        if *imp.apps_shown.borrow() == apps {
            return;
        }
        let grid = &imp.apps_grid;
        let shown = i32::try_from(imp.apps_shown.borrow().len()).unwrap_or(i32::MAX);
        let focused = (0..shown).find(|&index| {
            grid.child_at_index(index)
                .is_some_and(|child| child.has_focus())
        });
        grid.remove_all();
        for app in &apps {
            grid.append(&app_tile(app));
        }
        imp.apps_shown.replace(apps);
        if let Some(index) = focused {
            self.focus_app(index);
        }
    }

    /// Shows the Apps page: above Home in fullscreen mode, without its header bar, focused
    /// on the app that was opened last; in the library's place in the desktop window, with
    /// the sidebar's Apps item selected.
    pub(crate) fn show_apps(&self) {
        let imp = self.imp();
        let page = imp.apps_page.get();
        if let Some(toolbar) = page.child().and_downcast::<adw::ToolbarView>() {
            toolbar.set_reveal_top_bars(!self.is_fullscreen());
        }
        imp.apps_title.set_visible(self.is_fullscreen());
        if self.is_fullscreen() {
            imp.navigation_view.pop_to_page(&*imp.home_page);
            imp.navigation_view.push(&page);
            self.focus_app(imp.apps_cursor.get());
            return;
        }
        imp.navigation_view.replace(&[page]);
        imp.sidebar.set_selected(imp.apps_item.index());
        if imp.split_view.is_collapsed() {
            imp.split_view.set_show_sidebar(false);
        }
    }

    /// Whether the Apps page shows.
    pub(crate) fn shows_apps(&self) -> bool {
        let imp = self.imp();
        imp.navigation_view.visible_page().as_ref() == Some(&*imp.apps_page)
    }

    /// Focuses the app tile at `index`, or the last one where there are fewer; `false`
    /// without apps.
    pub(crate) fn focus_app(&self, index: i32) -> bool {
        let grid = &self.imp().apps_grid;
        let count = i32::try_from(self.imp().apps_shown.borrow().len()).unwrap_or(i32::MAX);
        if count == 0 {
            return false;
        }
        grid.child_at_index(index.clamp(0, count - 1))
            .is_some_and(|child| child.grab_focus())
    }

    /// Opens the app whose tile is at `index`, as A or a click on it does.
    fn open_app_at(&self, index: i32) {
        let app = usize::try_from(index)
            .ok()
            .and_then(|index| self.imp().apps_shown.borrow().get(index).cloned());
        let (Some(app), Some(application)) = (app, self.application()) else {
            return;
        };
        self.imp().apps_cursor.set(index);
        let window = self.downgrade();
        launch_service::open_app(&application, &app, move |toast| {
            if let Some(window) = window.upgrade() {
                window.add_toast(toast);
            }
        });
    }
}

/// An app's tile on the Apps page (`.app-tile`): its icon, or a generic one where it has
/// none or until it loads, over its name.
fn app_tile(app: &App) -> gtk::FlowBoxChild {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(6)
        .margin_end(6)
        .build();
    // An image keeps the icon's size whatever the file's, where a picture would ask for
    // the file's own; the generic icon shows until the app's has loaded.
    let icon = gtk::Image::builder()
        .icon_name(APPS_ICON)
        .pixel_size(ICON_SIZE)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    if let Some(path) = app.icon.clone() {
        let icon = icon.downgrade();
        glib::spawn_future_local(async move {
            if let Ok(texture) = textures::load(&path).await
                && let Some(icon) = icon.upgrade()
            {
                icon.set_paintable(Some(&texture));
            }
        });
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
    let tile = gtk::FlowBoxChild::builder()
        .child(&content)
        .tooltip_text(&app.name)
        .css_classes(["card", "app-tile"])
        .build();
    tile.update_property(&[gtk::accessible::Property::Label(&app.name)]);
    tile
}
