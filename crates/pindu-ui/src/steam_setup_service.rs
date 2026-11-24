//! Steam's first start behind the Setting Up Steam screen (`steam_setup_dialog`): a Steam
//! that has never run downloads and installs its own update for minutes before it shows its
//! sign-in. Each of the ways to a first start, Home's Big Picture tile, the set-up
//! checklist's Sign In and Add a Store's Open, goes through `win.steam-big-picture`
//! (`big_picture`), which shows the screen, hands Big Picture to Steam and lets this follow
//! the set-up: it reads Steam's bootstrap log every second (`pindu_core::steam_setup`) and
//! shows its step and download, says so when the set-up fails or nothing moved for minutes
//! and offers Try Again, which hands Big Picture to Steam again.
//!
//! In a `gamescope --steam` session the screen stays in front, with Steam's own small
//! update windows behind it (`BigPicture::first_start`), until Steam's sign-in window takes
//! the launcher's place. Elsewhere the launcher cannot see Steam's windows: the screen goes
//! once Steam's log says that its client starts, and Steam's windows show as the desktop or
//! gamescope places them. Cancel, Escape or B give up on the screen and return to where the
//! user was; Steam goes on setting itself up in the background.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};
use pindu_core::steam_setup::{BootstrapLog, State, SteamSetup};

use crate::steam_setup_dialog::SteamSetupDialog;

/// How often the screen reads Steam's log.
const CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// What hands Big Picture to Steam, with what to call when that failed.
type HandOff = Box<dyn Fn(Box<dyn FnOnce()>)>;

/// What the screen's end does: `true` once Steam is ready, `false` once the user gave up.
type Finished = Box<dyn FnOnce(bool)>;

/// Steam's first start, as the Setting Up Steam screen follows it.
#[derive(Clone)]
pub struct SteamSetupFlow {
    inner: Rc<Inner>,
}

impl fmt::Debug for SteamSetupFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamSetupFlow")
            .field("setup", &self.inner.setup.get())
            .finish_non_exhaustive()
    }
}

struct Inner {
    dialog: SteamSetupDialog,
    setup: Cell<SteamSetup>,
    /// Where Steam's bootstrap log can be.
    paths: Vec<PathBuf>,
    /// The log as far as it was read; `None` while a read is under way.
    log: RefCell<Option<BootstrapLog>>,
    ticker: RefCell<Option<glib::SourceId>>,
    hand_off: HandOff,
    /// Taken once the screen ends.
    finished: RefCell<Option<Finished>>,
    /// Whether Big Picture's session was there since the last hand-off.
    saw_big_picture: Cell<bool>,
}

impl SteamSetupFlow {
    /// Shows the screen over `parent` and follows Steam's bootstrap log in `paths`;
    /// [`Self::hand_off`] then hands Big Picture to Steam with `hand_off`. Where the
    /// launcher `follows_windows` (a `gamescope --steam` session), Steam's sign-in ends the
    /// screen (see [`Self::big_picture_changed`]). `finished` learns how the screen ended.
    pub fn show(
        parent: &impl IsA<gtk::Widget>,
        follows_windows: bool,
        paths: Vec<PathBuf>,
        hand_off: impl Fn(Box<dyn FnOnce()>) + 'static,
        finished: impl FnOnce(bool) + 'static,
    ) -> Self {
        let inner = Rc::new_cyclic(|weak: &Weak<Inner>| {
            let weak = weak.clone();
            let dialog = SteamSetupDialog::new(move || {
                if let Some(inner) = weak.upgrade() {
                    SteamSetupFlow { inner }.try_again();
                }
            });
            Inner {
                dialog,
                setup: Cell::new(SteamSetup::new(Instant::now(), follows_windows)),
                log: RefCell::new(Some(BootstrapLog::open(paths.clone()))),
                paths,
                ticker: RefCell::new(None),
                hand_off: Box::new(hand_off),
                finished: RefCell::new(Some(Box::new(finished))),
                saw_big_picture: Cell::new(false),
            }
        });
        let flow = Self { inner };
        let weak = Rc::downgrade(&flow.inner);
        flow.inner.dialog.connect_closed(move |_| {
            if let Some(inner) = weak.upgrade() {
                SteamSetupFlow { inner }.closed();
            }
        });
        let weak = Rc::downgrade(&flow.inner);
        let ticker = glib::timeout_add_local(CHECK_INTERVAL, move || {
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            SteamSetupFlow { inner }.check();
            glib::ControlFlow::Continue
        });
        flow.inner.ticker.replace(Some(ticker));
        glib::g_info!("pindu", "Steam sets itself up for its first start");
        flow.inner.dialog.present(Some(parent));
        flow
    }

    /// Follows Big Picture's session in a `gamescope --steam` session: Steam's sign-in
    /// window shows once the session is there and no longer opening, and a session that
    /// ended by other means ends the screen too.
    pub fn big_picture_changed(&self, in_big_picture: bool, opening: bool) {
        if in_big_picture {
            self.inner.saw_big_picture.set(true);
            if !opening {
                self.change(SteamSetup::sign_in_shown);
            }
        } else if self.inner.saw_big_picture.get() {
            self.change(SteamSetup::cancel);
        }
    }

    /// Hands Big Picture to Steam; a hand-off that failed ends the screen (the hand-off
    /// says why).
    pub fn hand_off(&self) {
        self.inner.saw_big_picture.set(false);
        let weak = Rc::downgrade(&self.inner);
        (self.inner.hand_off)(Box::new(move || {
            if let Some(inner) = weak.upgrade() {
                SteamSetupFlow { inner }.change(SteamSetup::cancel);
            }
        }));
    }

    /// Try Again: the wait starts over, from where Steam's log ends now, and Big Picture
    /// goes to Steam again.
    fn try_again(&self) {
        glib::g_info!("pindu", "Steam's set-up: trying again");
        self.change(|setup| setup.try_again(Instant::now()));
        self.inner
            .log
            .replace(Some(BootstrapLog::open(self.inner.paths.clone())));
        self.hand_off();
    }

    /// Reads what Steam wrote to its log since the last look, then shows where the set-up
    /// is.
    fn check(&self) {
        let Some(mut log) = self.inner.log.take() else {
            // The last read is still under way.
            return;
        };
        let weak = Rc::downgrade(&self.inner);
        glib::spawn_future_local(async move {
            let read = gio::spawn_blocking(move || {
                let lines = log.new_lines();
                (log, lines)
            })
            .await;
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let flow = SteamSetupFlow { inner };
            let Ok((log, lines)) = read else {
                return;
            };
            // Try Again may have opened the log afresh meanwhile.
            if flow.inner.log.borrow().is_none() {
                flow.inner.log.replace(Some(log));
            }
            let now = Instant::now();
            flow.change(|setup| {
                for line in &lines {
                    glib::g_debug!("pindu", "Steam's bootstrap log: {line}");
                    setup.log_line(line, now);
                }
                setup.check(now);
            });
        });
    }

    /// Changes the set-up with `change`, then shows it, or ends the screen once Steam is
    /// ready or the user gave up.
    fn change(&self, change: impl FnOnce(&mut SteamSetup)) {
        let mut setup = self.inner.setup.get();
        let before = setup.state();
        change(&mut setup);
        self.inner.setup.set(setup);
        let state = setup.state();
        if state != before {
            glib::g_info!("pindu", "Steam's set-up: {state:?}");
        }
        match state {
            State::Ready => self.finish(true),
            State::Cancelled => self.finish(false),
            State::Working(_) | State::Failed(_) | State::TimedOut => {
                let progress = setup.progress();
                self.inner.dialog.show_state(state, progress);
                if progress.is_none() && matches!(state, State::Working(_)) {
                    self.inner.dialog.pulse();
                }
            }
        }
    }

    /// The screen was closed: Cancel, Escape or B, or its end.
    fn closed(&self) {
        self.change(SteamSetup::cancel);
    }

    /// Ends the screen, telling whether Steam is `ready`.
    fn finish(&self, ready: bool) {
        let Some(finished) = self.inner.finished.take() else {
            return;
        };
        if let Some(ticker) = self.inner.ticker.take() {
            ticker.remove();
        }
        self.inner.dialog.force_close();
        finished(ready);
    }
}
