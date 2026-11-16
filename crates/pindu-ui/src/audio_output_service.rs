//! Keeps the Sound Output section (see `pindu_core::system::audio`) for the shells, over
//! [`WirePlumberBackend`]: the outputs and the one in use for them to show, the choice of
//! another for them to call, and a notification when a choice does not take.

use gettextrs::gettext;
use gtk::glib;
use pindu_core::notifications::Kind;
use pindu_core::system::audio::{AudioState, Failure, SoundOutput};

use crate::notification_service::NotificationService;
use crate::wireplumber_backend::WirePlumberBackend;

/// The Sound Output section, read at start-up and again when the shells ask.
#[derive(Clone, Debug)]
pub struct AudioOutputService {
    output: SoundOutput,
}

impl AudioOutputService {
    /// Reads the outputs now, posting a choice that fails to `notifications`. Outputs that
    /// cannot be read only hide the section, and are logged.
    pub fn new(notifications: &NotificationService) -> Self {
        let output = SoundOutput::new(Box::new(WirePlumberBackend::new()));
        let notifications = notifications.clone();
        output.connect_failed(move |failure| match failure {
            Failure::Read(error) => {
                glib::g_warning!("pindu", "Cannot read the sound outputs: {error}");
            }
            Failure::Choose(error) => {
                glib::g_warning!("pindu", "Could not change the sound output: {error}");
                notifications.post(
                    Kind::SoundOutputFailed,
                    &gettext("Could not change the sound output"),
                    error,
                    None,
                );
            }
        });
        output.refresh();
        Self { output }
    }

    /// The section as last read.
    pub fn state(&self) -> AudioState {
        self.output.state()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&AudioState) + 'static) {
        self.output.connect_changed(listener);
    }

    /// Reads the outputs again: Quick Access or its Sound Output opens.
    pub fn refresh(&self) {
        self.output.refresh();
    }

    /// Makes the output with PipeWire object id `id` the default.
    pub fn choose(&self, id: u32) {
        self.output.choose(id);
    }
}
