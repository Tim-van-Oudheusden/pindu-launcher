//! The user's own stylesheet for fullscreen mode, `fullscreen.css` in the app's config
//! folder. One CSS provider is added to the display only while the stylesheet is in force
//! (`pindu_core::custom_style::StyleState`): with the `fullscreen-custom-style` setting
//! on, in fullscreen mode, and without high contrast. So nothing in it can reach the
//! desktop window. Saving the file reloads it.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use pindu_core::custom_style::{self, Severity, StyleProblem, StyleState};
use pindu_core::paths;

use crate::config::APP_ID;

/// The setting that turns the stylesheet on.
const SETTING: &str = "fullscreen-custom-style";

/// How long changes to the file must settle before it is read again; editors write a file
/// in several steps.
const DEBOUNCE: Duration = Duration::from_millis(300);

type ProblemsHandler = Box<dyn Fn(usize)>;

#[derive(Default)]
struct State {
    style: StyleState,
    /// Whether the provider is on the display.
    installed: bool,
    /// What GTK reported while reading the file last.
    problems: Vec<StyleProblem>,
    /// Watches the file's folder while the setting is on.
    monitor: Option<gio::FileMonitor>,
    pending: Option<glib::SourceId>,
    on_problems: Vec<ProblemsHandler>,
}

#[derive(Clone)]
pub struct StyleService {
    provider: gtk::CssProvider,
    path: Rc<PathBuf>,
    settings: gio::Settings,
    state: Rc<RefCell<State>>,
}

impl std::fmt::Debug for StyleService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StyleService")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl StyleService {
    /// Follows the setting, the system style and the motion preference; the stylesheet is
    /// in force once the window says it is in fullscreen mode.
    pub fn new() -> Self {
        let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let home = std::env::home_dir().unwrap_or_default();
        let service = Self {
            provider: gtk::CssProvider::new(),
            path: Rc::new(paths::fullscreen_style_path(
                xdg_config_home.as_deref(),
                &home,
            )),
            settings: gio::Settings::new(APP_ID),
            state: Rc::default(),
        };
        let state = Rc::downgrade(&service.state);
        service
            .provider
            .connect_parsing_error(move |_, section, error| {
                let Some(state) = state.upgrade() else {
                    return;
                };
                let severity = if error.kind::<gtk::CssParserWarning>().is_some() {
                    Severity::Warning
                } else {
                    Severity::Error
                };
                state.borrow_mut().problems.push(StyleProblem {
                    line: section.start_location().lines(),
                    message: error.message().to_owned(),
                    severity,
                });
            });
        service.follow_setting();
        service.follow_style_manager();
        service.follow_reduced_motion();
        service
    }

    /// Calls `handler` with the number of errors whenever the stylesheet is read with any.
    pub fn connect_problems(&self, handler: impl Fn(usize) + 'static) {
        self.state.borrow_mut().on_problems.push(Box::new(handler));
    }

    /// The window entered or left fullscreen mode.
    pub fn set_fullscreen(&self, fullscreen: bool) {
        self.state.borrow_mut().style.fullscreen = fullscreen;
        self.update();
    }

    fn follow_setting(&self) {
        let service = self.clone();
        self.settings
            .connect_changed(Some(SETTING), move |settings, _| {
                service.state.borrow_mut().style.enabled = settings.boolean(SETTING);
                service.update();
            });
        self.state.borrow_mut().style.enabled = self.settings.boolean(SETTING);
        self.update();
    }

    /// Dark and light for `prefers-color-scheme`, as libadwaita does for its own
    /// stylesheet; high contrast suspends the stylesheet, so `prefers-contrast` keeps its
    /// default.
    fn follow_style_manager(&self) {
        let manager = adw::StyleManager::default();
        let update = {
            let service = self.clone();
            move |manager: &adw::StyleManager| {
                service
                    .provider
                    .set_prefers_color_scheme(if manager.is_dark() {
                        gtk::InterfaceColorScheme::Dark
                    } else {
                        gtk::InterfaceColorScheme::Light
                    });
                service.state.borrow_mut().style.high_contrast = manager.is_high_contrast();
                service.update();
            }
        };
        update(&manager);
        manager.connect_dark_notify(update.clone());
        manager.connect_high_contrast_notify(update);
    }

    /// `prefers-reduced-motion` follows the same preference as the window's
    /// `reduced-motion` class: animations off, or reduced motion asked for.
    fn follow_reduced_motion(&self) {
        let Some(settings) = gtk::Settings::default() else {
            return;
        };
        let provider = self.provider.clone();
        let update = move |settings: &gtk::Settings| {
            let reduce = !settings.is_gtk_enable_animations()
                || settings.gtk_interface_reduced_motion() == gtk::ReducedMotion::Reduce;
            provider.set_prefers_reduced_motion(if reduce {
                gtk::ReducedMotion::Reduce
            } else {
                gtk::ReducedMotion::NoPreference
            });
        };
        update(&settings);
        settings.connect_gtk_enable_animations_notify(update.clone());
        settings.connect_gtk_interface_reduced_motion_notify(update);
    }

    /// Adds or removes the provider and the folder's monitor to match the state.
    fn update(&self) {
        let (style, installed, watching) = {
            let state = self.state.borrow();
            (state.style, state.installed, state.monitor.is_some())
        };
        if style.enabled != watching {
            self.watch(style.enabled);
        }
        let Some(display) = gdk::Display::default() else {
            return;
        };
        if style.applies() && !installed {
            self.load();
            gtk::style_context_add_provider_for_display(
                &display,
                &self.provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
            self.state.borrow_mut().installed = true;
            self.report();
        } else if !style.applies() && installed {
            gtk::style_context_remove_provider_for_display(&display, &self.provider);
            self.state.borrow_mut().installed = false;
        }
    }

    /// Starts or stops watching the file's folder, which need not exist yet.
    fn watch(&self, watch: bool) {
        let old = self.state.borrow_mut().monitor.take();
        if let Some(old) = old {
            old.cancel();
        }
        if !watch {
            return;
        }
        let Some(folder) = self.path.parent() else {
            return;
        };
        match gio::File::for_path(folder)
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        {
            Ok(monitor) => {
                let service = self.clone();
                monitor.connect_changed(move |_, file, other, _| {
                    let other = other.and_then(|other| other.path());
                    if file.path().is_some_and(|file| {
                        custom_style::concerns_style(&service.path, &file, other.as_deref())
                    }) {
                        service.changed();
                    }
                });
                self.state.borrow_mut().monitor = Some(monitor);
            }
            Err(error) => glib::g_debug!("pindu", "Cannot watch {folder:?}: {error}"),
        }
    }

    /// The file changed: read it again once changes have settled, if it is in force.
    fn changed(&self) {
        if let Some(pending) = self.state.borrow_mut().pending.take() {
            pending.remove();
        }
        let service = self.clone();
        let id = glib::timeout_add_local_once(DEBOUNCE, move || {
            service.state.borrow_mut().pending = None;
            if service.state.borrow().installed {
                service.load();
                service.report();
            }
        });
        self.state.borrow_mut().pending = Some(id);
    }

    /// Reads the file into the provider; no file means no rules.
    fn load(&self) {
        self.state.borrow_mut().problems.clear();
        if self.path.is_file() {
            self.provider.load_from_path(self.path.as_path());
        } else {
            self.provider.load_from_string("");
        }
    }

    fn report(&self) {
        let state = self.state.borrow();
        let errors = custom_style::error_count(&state.problems);
        for problem in &state.problems {
            glib::g_info!("pindu", "{}: {problem}", self.path.display());
        }
        if errors > 0 {
            for handler in &state.on_problems {
                handler(errors);
            }
        }
    }
}

impl Default for StyleService {
    fn default() -> Self {
        Self::new()
    }
}
