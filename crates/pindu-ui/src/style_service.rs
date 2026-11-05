//! Fullscreen mode's stylesheets: the chosen design (a theme folder, `pindu_core::themes`)
//! and the user's own `fullscreen.css` on top of it. Each has one CSS provider, added to
//! the display only while it is in force (`pindu_core::custom_style::StyleState`): in
//! fullscreen mode, without high contrast, and for `fullscreen.css` with the
//! `fullscreen-custom-style` setting on. So nothing in either can reach the desktop
//! window. Saving a stylesheet, or another file directly in the design's folder (one it
//! imports, say), reloads it. The design's layout choices (`ThemeLayout`), which CSS
//! cannot make, are in force with its stylesheet; choosing the design again rereads them.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use pindu_core::custom_style::{self, Severity, StyleProblem, StyleState};
use pindu_core::paths;
use pindu_core::themes::{self, Theme, ThemeLayout};

use crate::config::{APP_ID, PKGDATADIR};

/// The setting that turns `fullscreen.css` on.
const CUSTOM_STYLE: &str = "fullscreen-custom-style";
/// The setting that holds the chosen design's id; empty for none.
const DESIGN: &str = "fullscreen-design";

/// How long changes to a stylesheet must settle before it is read again; editors write a
/// file in several steps.
const DEBOUNCE: Duration = Duration::from_millis(300);

type ProblemsHandler = Box<dyn Fn(usize)>;
type ProblemsHandlers = Rc<RefCell<Vec<ProblemsHandler>>>;
type LayoutHandlers = Rc<RefCell<Vec<Box<dyn Fn(ThemeLayout)>>>>;

/// Which changes in a stylesheet's folder reload it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Watch {
    /// Only changes to the stylesheet itself: `fullscreen.css` shares its folder.
    File,
    /// Any change directly in the folder, which holds a design's imported stylesheets;
    /// the watch does not reach into subfolders.
    Folder,
}

#[derive(Default)]
struct SheetState {
    /// The stylesheet, if there is one to show.
    file: Option<PathBuf>,
    /// Whether the provider is on the display.
    installed: bool,
    /// What GTK reported while reading the stylesheet last.
    problems: Vec<StyleProblem>,
    /// Watches the stylesheet's folder while there is a stylesheet.
    monitor: Option<gio::FileMonitor>,
    pending: Option<glib::SourceId>,
}

/// One stylesheet and its provider.
#[derive(Clone)]
struct Sheet {
    provider: gtk::CssProvider,
    priority: u32,
    watch: Watch,
    state: Rc<RefCell<SheetState>>,
    on_problems: ProblemsHandlers,
}

impl Sheet {
    fn new(priority: u32, watch: Watch, on_problems: &ProblemsHandlers) -> Self {
        let sheet = Self {
            provider: gtk::CssProvider::new(),
            priority,
            watch,
            state: Rc::default(),
            on_problems: on_problems.clone(),
        };
        let state = Rc::downgrade(&sheet.state);
        sheet
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
        sheet
    }

    /// Shows `file`, or nothing: watches its folder, and reads it again if it is shown.
    fn set_file(&self, file: Option<PathBuf>) {
        if self.state.borrow().file == file {
            return;
        }
        self.state.borrow_mut().file = file.clone();
        self.watch(file.as_deref());
        if self.state.borrow().installed {
            if file.is_some() {
                self.load();
            } else {
                self.uninstall();
            }
        }
    }

    /// Adds the provider to the display, or removes it.
    fn show(&self, show: bool) {
        let (has_file, installed) = {
            let state = self.state.borrow();
            (state.file.is_some(), state.installed)
        };
        if show && has_file && !installed {
            let Some(display) = gdk::Display::default() else {
                return;
            };
            self.load();
            gtk::style_context_add_provider_for_display(&display, &self.provider, self.priority);
            self.state.borrow_mut().installed = true;
        } else if !(show && has_file) && installed {
            self.uninstall();
        }
    }

    fn uninstall(&self) {
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_remove_provider_for_display(&display, &self.provider);
        }
        self.state.borrow_mut().installed = false;
    }

    /// Watches the folder of `file`, which need not exist yet; stops watching for `None`.
    fn watch(&self, file: Option<&Path>) {
        let old = self.state.borrow_mut().monitor.take();
        if let Some(old) = old {
            old.cancel();
        }
        let Some((file, folder)) = file.and_then(|file| Some((file, file.parent()?))) else {
            return;
        };
        match gio::File::for_path(folder)
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        {
            Ok(monitor) => {
                let sheet = self.clone();
                let file = file.to_owned();
                monitor.connect_changed(move |_, changed, other, _| {
                    let other = other.and_then(|other| other.path());
                    let concerns = sheet.watch == Watch::Folder
                        || changed.path().is_some_and(|changed| {
                            custom_style::concerns_style(&file, &changed, other.as_deref())
                        });
                    if concerns {
                        sheet.changed();
                    }
                });
                self.state.borrow_mut().monitor = Some(monitor);
            }
            Err(error) => glib::g_debug!("pindu", "Cannot watch {folder:?}: {error}"),
        }
    }

    /// The stylesheet changed: read it again once changes have settled, if it is shown.
    fn changed(&self) {
        if let Some(pending) = self.state.borrow_mut().pending.take() {
            pending.remove();
        }
        let sheet = self.clone();
        let id = glib::timeout_add_local_once(DEBOUNCE, move || {
            sheet.state.borrow_mut().pending = None;
            if sheet.state.borrow().installed {
                sheet.load();
            }
        });
        self.state.borrow_mut().pending = Some(id);
    }

    /// Reads the stylesheet into the provider, and reports what GTK found wrong with it;
    /// a missing file means no rules.
    fn load(&self) {
        self.state.borrow_mut().problems.clear();
        let file = self.state.borrow().file.clone();
        match file.filter(|file| file.is_file()) {
            Some(file) => self.provider.load_from_path(&file),
            None => self.provider.load_from_string(""),
        }
        let state = self.state.borrow();
        let name = state.file.as_deref().unwrap_or(Path::new("")).display();
        for problem in &state.problems {
            glib::g_info!("pindu", "{name}: {problem}");
        }
        let errors = custom_style::error_count(&state.problems);
        if errors > 0 {
            for handler in self.on_problems.borrow().iter() {
                handler(errors);
            }
        }
    }
}

#[derive(Clone)]
pub struct StyleService {
    /// The user's own `fullscreen.css`, which has the last word.
    custom: Sheet,
    /// The chosen design, under `fullscreen.css`.
    design: Sheet,
    custom_path: Rc<PathBuf>,
    /// Where designs are found: the shipped ones, then the user's, which win.
    theme_folders: Rc<[PathBuf; 2]>,
    settings: gio::Settings,
    style: Rc<RefCell<StyleState>>,
    on_problems: ProblemsHandlers,
    /// The chosen design's layout, in force while the design is.
    design_layout: Rc<Cell<ThemeLayout>>,
    /// The layout last reported to the handlers.
    layout: Rc<Cell<ThemeLayout>>,
    on_layout: LayoutHandlers,
}

impl std::fmt::Debug for StyleService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StyleService")
            .field("custom_path", &self.custom_path)
            .field("theme_folders", &self.theme_folders)
            .finish_non_exhaustive()
    }
}

impl StyleService {
    /// Follows the settings, the system style and the motion preference; the stylesheets
    /// are in force once the window says it is in fullscreen mode.
    pub fn new() -> Self {
        let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let home = std::env::home_dir().unwrap_or_default();
        let on_problems = ProblemsHandlers::default();
        let service = Self {
            custom: Sheet::new(gtk::STYLE_PROVIDER_PRIORITY_USER, Watch::File, &on_problems),
            design: Sheet::new(
                gtk::STYLE_PROVIDER_PRIORITY_USER - 1,
                Watch::Folder,
                &on_problems,
            ),
            custom_path: Rc::new(paths::fullscreen_style_path(
                xdg_config_home.as_deref(),
                &home,
            )),
            theme_folders: Rc::new([
                Path::new(PKGDATADIR).join("themes"),
                paths::themes_dir(xdg_config_home.as_deref(), &home),
            ]),
            settings: gio::Settings::new(APP_ID),
            style: Rc::default(),
            on_problems,
            design_layout: Rc::default(),
            layout: Rc::default(),
            on_layout: LayoutHandlers::default(),
        };
        service.follow_settings();
        service.follow_style_manager();
        service.follow_reduced_motion();
        service
    }

    /// Calls `handler` with the number of errors whenever a stylesheet is read with any.
    pub fn connect_problems(&self, handler: impl Fn(usize) + 'static) {
        self.on_problems.borrow_mut().push(Box::new(handler));
    }

    /// The layout of the design in force: the chosen design's while it applies (fullscreen
    /// mode, no high contrast), `ThemeLayout::default()` otherwise.
    pub fn layout(&self) -> ThemeLayout {
        if self.style.borrow().design_applies() {
            self.design_layout.get()
        } else {
            ThemeLayout::default()
        }
    }

    /// Calls `handler` with the new layout whenever `layout()` changes.
    pub fn connect_layout_changed(&self, handler: impl Fn(ThemeLayout) + 'static) {
        self.on_layout.borrow_mut().push(Box::new(handler));
    }

    /// The window entered or left fullscreen mode.
    pub fn set_fullscreen(&self, fullscreen: bool) {
        self.style.borrow_mut().fullscreen = fullscreen;
        self.update();
    }

    fn sheets(&self) -> [&Sheet; 2] {
        [&self.custom, &self.design]
    }

    fn follow_settings(&self) {
        let service = self.clone();
        self.settings
            .connect_changed(Some(CUSTOM_STYLE), move |_, _| {
                service.follow_custom_style()
            });
        let service = self.clone();
        self.settings
            .connect_changed(Some(DESIGN), move |_, _| service.follow_design());
        self.follow_custom_style();
        self.follow_design();
    }

    fn follow_custom_style(&self) {
        let enabled = self.settings.boolean(CUSTOM_STYLE);
        self.style.borrow_mut().enabled = enabled;
        self.custom
            .set_file(enabled.then(|| self.custom_path.as_ref().clone()));
        self.update();
    }

    fn follow_design(&self) {
        let id = self.settings.string(DESIGN);
        let theme = self.design_theme(&id);
        self.design.set_file(theme.as_ref().map(Theme::stylesheet));
        self.design_layout
            .set(theme.map(|theme| theme.layout).unwrap_or_default());
        self.update();
    }

    /// The design `id`, if it is installed and this Pindu launcher can apply it.
    fn design_theme(&self, id: &str) -> Option<Theme> {
        if id.is_empty() {
            return None;
        }
        let folders = self.theme_folders.each_ref().map(PathBuf::as_path);
        let found = themes::discover(&folders);
        for (dir, reason) in &found.skipped {
            glib::g_info!("pindu", "Skipped the theme in {}: {reason}", dir.display());
        }
        let Some(theme) = found.themes.into_iter().find(|theme| theme.id == id) else {
            glib::g_info!("pindu", "The fullscreen design “{id}” is not installed");
            return None;
        };
        if !theme.supported() {
            glib::g_info!(
                "pindu",
                "The fullscreen design “{id}” needs a newer Pindu launcher (contract {})",
                theme.contract
            );
            return None;
        }
        Some(theme)
    }

    /// Dark and light for `prefers-color-scheme`, as libadwaita does for its own
    /// stylesheet; high contrast suspends the stylesheets, so `prefers-contrast` keeps its
    /// default.
    fn follow_style_manager(&self) {
        let manager = adw::StyleManager::default();
        let update = {
            let service = self.clone();
            move |manager: &adw::StyleManager| {
                let scheme = if manager.is_dark() {
                    gtk::InterfaceColorScheme::Dark
                } else {
                    gtk::InterfaceColorScheme::Light
                };
                for sheet in service.sheets() {
                    sheet.provider.set_prefers_color_scheme(scheme);
                }
                service.style.borrow_mut().high_contrast = manager.is_high_contrast();
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
        let service = self.clone();
        let update = move |settings: &gtk::Settings| {
            let reduce = !settings.is_gtk_enable_animations()
                || settings.gtk_interface_reduced_motion() == gtk::ReducedMotion::Reduce;
            let motion = if reduce {
                gtk::ReducedMotion::Reduce
            } else {
                gtk::ReducedMotion::NoPreference
            };
            for sheet in service.sheets() {
                sheet.provider.set_prefers_reduced_motion(motion);
            }
        };
        update(&settings);
        settings.connect_gtk_enable_animations_notify(update.clone());
        settings.connect_gtk_interface_reduced_motion_notify(update);
    }

    /// Shows each stylesheet while it is in force, and reports a change of layout.
    fn update(&self) {
        let style = *self.style.borrow();
        self.design.show(style.design_applies());
        self.custom.show(style.applies());
        let layout = self.layout();
        if self.layout.replace(layout) != layout {
            for handler in self.on_layout.borrow().iter() {
                handler(layout);
            }
        }
    }
}

impl Default for StyleService {
    fn default() -> Self {
        Self::new()
    }
}
