//! Fullscreen mode's hold to confirm (`pindu_core::hold`) for the main window and the panels
//! over a game: while the option is on, the confirm button on Power Off, Restart, Forget or
//! Stop shows a popover over the focused widget whose bar fills as the button stays down,
//! and activates the widget once it is full. Letting go early, another button or the widget
//! going away ends the hold with nothing done.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use gettextrs::gettext;
use gtk::glib;
use gtk::prelude::*;
use pindu_core::gamepad::Intent;
use pindu_core::hold::{Destructive, HoldToConfirm, Press, Tick};

use crate::gamepad_service::GamepadService;

/// How often the bar follows the held button.
const FRAME: Duration = Duration::from_millis(16);

/// A window's hold to confirm, and the popover it shows while one is under way.
#[derive(Debug, Default)]
pub(crate) struct HoldProgress {
    state: Cell<HoldToConfirm>,
    shown: RefCell<Option<Shown>>,
}

/// A hold under way: the widget it confirms, the popover over it and the timer that fills
/// its bar.
#[derive(Debug)]
struct Shown {
    target: gtk::Widget,
    popover: gtk::Popover,
    timer: glib::SourceId,
    /// Ends the hold once the widget leaves the screen, as when its list is rebuilt.
    unmapped: glib::SignalHandlerId,
}

impl HoldProgress {
    /// Hands `intent` to the hold, with `focus` the focused widget and the destructive
    /// action it does, if it does one, and `enabled` whether the option is on. Says whether
    /// the intent started a hold, which takes it; any other intent ends a hold under way
    /// and goes on as usual. `confirmed` activates the widget once it has been held long
    /// enough.
    pub(crate) fn intent(
        self: &Rc<Self>,
        intent: Intent,
        focus: Option<(gtk::Widget, Destructive)>,
        enabled: bool,
        gamepad: &GamepadService,
        confirmed: impl Fn(&gtk::Widget) + 'static,
    ) -> bool {
        let mut state = self.state.get();
        let press = state.intent(intent, focus.as_ref().map(|(_, action)| *action), enabled);
        self.state.set(state);
        self.end(false);
        match (press, focus) {
            (Press::Hold(action), Some((target, _))) => {
                self.show(target, action, gamepad.clone(), confirmed);
                true
            }
            _ => false,
        }
    }

    fn show(
        self: &Rc<Self>,
        target: gtk::Widget,
        action: Destructive,
        gamepad: GamepadService,
        confirmed: impl Fn(&gtk::Widget) + 'static,
    ) {
        let bar = gtk::ProgressBar::builder().width_request(240).build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        content.append(&gtk::Label::new(Some(&hold_label(action))));
        content.append(&bar);
        let popover = gtk::Popover::builder()
            .child(&content)
            .autohide(false)
            .can_focus(false)
            .can_target(false)
            .build();
        popover.set_parent(&target);
        let this: Weak<Self> = Rc::downgrade(self);
        let unmapped = target.connect_unmap(glib::clone!(
            #[strong]
            this,
            move |_| {
                if let Some(this) = this.upgrade() {
                    let mut state = this.state.get();
                    state.tick(None);
                    this.state.set(state);
                    this.end(false);
                }
            }
        ));
        let timer = glib::timeout_add_local(FRAME, move || {
            let Some(this) = this.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let mut state = this.state.get();
            let tick = state.tick(gamepad.held_for(Intent::Activate));
            this.state.set(state);
            match tick {
                Tick::Holding(fraction) => {
                    bar.set_fraction(fraction);
                    return glib::ControlFlow::Continue;
                }
                Tick::Confirmed(_) => {
                    if let Some(target) = this.end(true) {
                        confirmed(&target);
                    }
                }
                Tick::LetGo | Tick::Idle => {
                    this.end(true);
                }
            }
            glib::ControlFlow::Break
        });
        popover.popup();
        self.shown.replace(Some(Shown {
            target,
            popover,
            timer,
            unmapped,
        }));
    }

    /// Takes the popover down and gives back the widget it confirmed, if one showed.
    /// `fired` says the timer ends itself, so it is not removed.
    fn end(&self, fired: bool) -> Option<gtk::Widget> {
        let shown = self.shown.take()?;
        if !fired {
            shown.timer.remove();
        }
        shown.target.disconnect(shown.unmapped);
        shown.popover.popdown();
        shown.popover.unparent();
        Some(shown.target)
    }
}

/// What the popover asks the user to do.
fn hold_label(action: Destructive) -> String {
    match action {
        Destructive::PowerOff => gettext("Hold to Power Off"),
        Destructive::Restart => gettext("Hold to Restart"),
        Destructive::Forget => gettext("Hold to Forget"),
        Destructive::Stop => gettext("Hold to Stop"),
    }
}

/// The destructive action `widget` activates, by its action name, if it does one.
pub(crate) fn destructive_action(widget: &gtk::Widget) -> Option<Destructive> {
    let action = widget
        .dynamic_cast_ref::<gtk::Actionable>()?
        .action_name()?;
    Destructive::of_action(&action)
}
