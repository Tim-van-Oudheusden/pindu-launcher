//! Where Quick Access asks its questions in a `--steam` gamescope: a Wi-Fi passphrase, a
//! Bluetooth PIN or passkey, or a code to confirm. Quick Access lives both in the main
//! window and in the overlay over a running game, and both show Network and Bluetooth, so
//! one of them has to ask: the one the player sees.

/// A window that hosts Quick Access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptHost {
    /// The launcher's main window.
    MainWindow,
    /// Quick Access over a running game (see [`super::overlay`]).
    Overlay,
}

/// The window that asks what Network and Bluetooth want while Quick Access is
/// `overlay_open` over a game, or not: the overlay while it is open, and the main window
/// otherwise, as it does outside gamescope. A question still unanswered as Quick Access
/// closes over the game goes back to the main window, which asks it once it is in front.
pub fn prompt_host(overlay_open: bool) -> PromptHost {
    if overlay_open {
        PromptHost::Overlay
    } else {
        PromptHost::MainWindow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_access_over_the_game_asks_while_it_is_open() {
        assert_eq!(prompt_host(true), PromptHost::Overlay);
    }

    #[test]
    fn the_main_window_asks_while_quick_access_is_closed_over_the_game() {
        assert_eq!(prompt_host(false), PromptHost::MainWindow);
    }
}
