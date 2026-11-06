//! Fullscreen mode's hint bar: what each controller button does right now, with the active
//! controller's glyphs. Which hints show is `pindu_core::hints`; this follows the focused
//! widget, the shown page, open dialogs and the active controller, and draws the bar.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::hints::{self, Action, Context, Focus, GlyphName};

use crate::application::PinduApplication;
use crate::window::PinduWindow;

/// How large glyphs draw, to read from across the room.
const GLYPH_SIZE: i32 = 32;

impl PinduWindow {
    /// Keeps the hint bar up to date while the window is fullscreen with a controller.
    pub(crate) fn setup_hint_bar(&self, application: &PinduApplication) {
        application
            .gamepad_service()
            .connect_active_kind_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.update_hints()
            ));
        self.connect_fullscreened_notify(|window| window.update_hints());
        self.connect_focus_widget_notify(|window| window.update_hints());
        // Play turns into Stop while the window is in the background.
        self.connect_is_active_notify(|window| window.update_hints());
        self.connect_notify_local(Some("visible-dialog"), |window, _| window.update_hints());
        self.imp()
            .navigation_view
            .connect_visible_page_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.update_hints()
            ));
        self.update_hints();
    }

    /// Shows the hints for where focus is now, in fullscreen mode while a controller is in
    /// use, and hides the bar otherwise.
    pub(crate) fn update_hints(&self) {
        let imp = self.imp();
        let kind = self
            .application()
            .and_downcast::<PinduApplication>()
            .and_then(|application| application.gamepad_service().active_kind())
            .filter(|_| self.is_fullscreen());
        imp.hint_toolbar.set_reveal_bottom_bars(kind.is_some());
        let Some(kind) = kind else {
            return;
        };
        let shown = Some((kind, hints::hints(self.hint_context(), kind)));
        if *imp.hints.borrow() == shown {
            return;
        }
        while let Some(child) = imp.hint_bar.first_child() {
            imp.hint_bar.remove(&child);
        }
        for hint in shown.iter().flat_map(|(_, hints)| hints) {
            // Named for fullscreen designs: `.hint` holds `.hint-glyph`s and a `.hint-label`.
            let item = gtk::Box::builder().spacing(8).css_classes(["hint"]).build();
            for button in std::iter::once(hint.button).chain(hint.pair) {
                let glyph = gtk::Image::builder()
                    .icon_name(hints::glyph(kind, button))
                    .pixel_size(GLYPH_SIZE)
                    .css_classes(["hint-glyph"])
                    .build();
                glyph.update_property(&[gtk::accessible::Property::Label(&glyph_label(
                    hints::glyph_name(kind, button),
                ))]);
                item.append(&glyph);
            }
            let label = gtk::Label::builder()
                .label(action_label(hint.action))
                .css_classes(["hint-label"])
                .build();
            item.append(&label);
            imp.hint_bar.append(&item);
        }
        imp.hints.replace(shown);
    }

    /// Where the user is, for the hints: the focused widget, an open dialog, whether a
    /// page lies below the shown one, whether it searches (Home and the library), whether
    /// it has tabs (the library in fullscreen mode), whether a game has options, whether
    /// Start opens the menu drawer, an open panel, whether the launch overlay is up and
    /// the open search keyboard.
    fn hint_context(&self) -> Context {
        let imp = self.imp();
        let navigation_view = &imp.navigation_view;
        let page = navigation_view.visible_page();
        // Home's second screen, a section's games, goes back to its first like a page.
        let page_below = page
            .as_ref()
            .and_then(|page| navigation_view.previous_page(page))
            .is_some()
            || self.shows_home_section();
        let library = page.as_ref() == Some(&*imp.library_page);
        let home = page.as_ref() == Some(imp.home_page.upcast_ref::<adw::NavigationPage>());
        Context {
            focus: self.hint_focus(),
            // An open menu closes on B like a dialog, and keeps the page's buttons too.
            dialog: self.visible_dialog().is_some() || self.open_menu().is_some(),
            page_below,
            search: library || home,
            tabs: library && self.is_fullscreen(),
            options: self.focused_game().is_some(),
            menu: self.is_fullscreen(),
            panel: self.open_panel(),
            launching: self.launching(),
            keyboard: self.search_keyboard_typing(),
        }
    }

    /// What A does to the focused widget.
    fn hint_focus(&self) -> Focus {
        let Some(focus) = gtk::prelude::GtkWindowExt::focus(self) else {
            return Focus::Nothing;
        };
        if self.focused_tile().is_some() || focus.has_css_class("jump-card") {
            return Focus::Game;
        }
        if let Some(button) = focus.downcast_ref::<gtk::Button>() {
            match button.action_name().as_deref() {
                Some("app.play") => return Focus::Play,
                Some("app.stop") => return Focus::Stop,
                _ => {}
            }
        }
        // Menu buttons and view switcher tabs are toggle buttons that open or pick
        // something rather than switch it.
        let picks = focus.ancestor(gtk::MenuButton::static_type()).is_some()
            || focus.ancestor(adw::ViewSwitcher::static_type()).is_some();
        if focus.is::<gtk::Switch>()
            || focus.is::<gtk::CheckButton>()
            || focus.is::<adw::SwitchRow>()
            || (focus.is::<gtk::ToggleButton>() && !picks)
        {
            return Focus::Toggle;
        }
        match focus.downcast_ref::<gtk::ListBoxRow>() {
            Some(row) if !row.is_activatable() => Focus::Nothing,
            // A notification with a target opens it.
            Some(row) if row.has_css_class("notification") => Focus::Link,
            // A task the user can cancel, in Quick Access's Activity.
            Some(row) if row.has_css_class("task") => Focus::Cancel,
            _ => Focus::Other,
        }
    }
}

/// What the hint bar calls an action.
fn action_label(action: Action) -> String {
    match action {
        Action::Open => gettext("Open"),
        Action::Play => gettext("Play"),
        Action::Stop => gettext("Stop"),
        Action::Cancel => gettext("Cancel"),
        Action::Toggle => gettext("Toggle"),
        Action::Select => gettext("Select"),
        Action::Back => gettext("Back"),
        Action::Close => gettext("Close"),
        Action::Search => gettext("Search"),
        Action::Tabs => gettext("Tabs"),
        Action::Sort => gettext("Sort"),
        Action::Options => gettext("Options"),
        Action::HoldToCancel => gettext("Hold to Cancel"),
        Action::Menu => gettext("Menu"),
        Action::QuickAccess => gettext("Quick Access"),
        Action::Type => gettext("Type"),
        Action::Delete => gettext("Delete"),
        Action::Done => gettext("Done"),
    }
}

/// What a screen reader calls a button's glyph.
fn glyph_label(name: GlyphName) -> String {
    match name {
        GlyphName::Label(letters) => letters.to_owned(),
        GlyphName::Cross => gettext("Cross"),
        GlyphName::Circle => gettext("Circle"),
        GlyphName::Square => gettext("Square"),
        GlyphName::Triangle => gettext("Triangle"),
        GlyphName::View => gettext("View"),
        GlyphName::Menu => gettext("Menu"),
        GlyphName::Minus => gettext("Minus"),
        GlyphName::Plus => gettext("Plus"),
        GlyphName::Home => gettext("Home"),
        GlyphName::More => gettext("More"),
        GlyphName::DpadUp => gettext("D-pad Up"),
        GlyphName::DpadDown => gettext("D-pad Down"),
        GlyphName::DpadLeft => gettext("D-pad Left"),
        GlyphName::DpadRight => gettext("D-pad Right"),
    }
}
