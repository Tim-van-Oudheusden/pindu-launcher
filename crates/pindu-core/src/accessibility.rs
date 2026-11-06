//! Fullscreen mode's accessibility: when it moves less, and when its type is larger.

/// What decides fullscreen mode's motion and type size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accessibility {
    /// GTK's `gtk-enable-animations`, the desktop's own animations setting.
    pub animations_enabled: bool,
    /// GTK's `gtk-interface-reduced-motion` asks for reduced motion.
    pub system_reduced_motion: bool,
    /// The `fullscreen-reduce-motion` setting.
    pub reduce_motion: bool,
    /// The `fullscreen-large-text` setting.
    pub large_text: bool,
    /// The window is in fullscreen mode.
    pub fullscreen: bool,
}

impl Accessibility {
    /// Whether Pindu moves less: whenever the desktop's animations are off or the system
    /// asks for reduced motion, and in fullscreen mode with the Reduce Motion setting on.
    pub fn reduces_motion(self) -> bool {
        !self.animations_enabled
            || self.system_reduced_motion
            || (self.fullscreen && self.reduce_motion)
    }

    /// Whether fullscreen mode's type is larger: in fullscreen mode with the Larger Text
    /// setting on. The desktop window follows the system's text size alone.
    pub fn enlarges_text(self) -> bool {
        self.fullscreen && self.large_text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IN_FULLSCREEN: Accessibility = Accessibility {
        animations_enabled: true,
        system_reduced_motion: false,
        reduce_motion: false,
        large_text: false,
        fullscreen: true,
    };

    #[test]
    fn motion_is_full_when_nothing_asks_for_less() {
        assert!(!IN_FULLSCREEN.reduces_motion());
    }

    #[test]
    fn turning_off_the_desktops_animations_reduces_motion() {
        let state = Accessibility {
            animations_enabled: false,
            ..IN_FULLSCREEN
        };

        assert!(state.reduces_motion());
    }

    #[test]
    fn the_systems_reduced_motion_preference_reduces_motion() {
        let state = Accessibility {
            system_reduced_motion: true,
            ..IN_FULLSCREEN
        };

        assert!(state.reduces_motion());
    }

    #[test]
    fn the_reduce_motion_setting_reduces_motion_in_fullscreen_mode_only() {
        let state = |fullscreen| Accessibility {
            reduce_motion: true,
            fullscreen,
            ..IN_FULLSCREEN
        };

        assert!(state(true).reduces_motion());
        // The desktop window keeps its motion: the setting is fullscreen mode's.
        assert!(!state(false).reduces_motion());
    }

    #[test]
    fn the_large_text_setting_enlarges_type_in_fullscreen_mode_only() {
        let state = |large_text, fullscreen| Accessibility {
            large_text,
            fullscreen,
            ..IN_FULLSCREEN
        };

        assert!(state(true, true).enlarges_text());
        assert!(!state(false, true).enlarges_text());
        assert!(!state(true, false).enlarges_text());
    }
}
