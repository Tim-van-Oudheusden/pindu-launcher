//! Fullscreen mode's accessibility: when it moves less, and how large its type is.

/// The GSettings key of fullscreen mode's text size, a `TextSize`'s setting.
pub const TEXT_SIZE_KEY: &str = "fullscreen-text-size";

/// How large fullscreen mode's type is, as Preferences and Quick Access set it. Each step
/// adds about a quarter of fullscreen mode's own size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextSize {
    /// Fullscreen mode's own size.
    #[default]
    Default,
    /// About a quarter larger, the size the former Larger Text setting gave.
    Large,
    /// About half as large again.
    Larger,
    /// About three quarters larger.
    Largest,
}

impl TextSize {
    /// Every size, smallest first.
    pub const ALL: [Self; 4] = [Self::Default, Self::Large, Self::Larger, Self::Largest];

    /// The value the setting holds for it.
    pub fn setting(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Large => "large",
            Self::Larger => "larger",
            Self::Largest => "largest",
        }
    }

    /// The size the setting `value` holds; `Default` for an unknown one.
    pub fn from_setting(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|size| size.setting() == value)
            .unwrap_or_default()
    }

    /// The window's style classes in fullscreen mode at this size, which `style.css` and
    /// designs scale type with: `large-text` for every size above the default, and the
    /// size's own class.
    pub fn classes(self) -> &'static [&'static str] {
        match self {
            Self::Default => &[],
            Self::Large => &[LARGE_TEXT_CLASS, "text-large"],
            Self::Larger => &[LARGE_TEXT_CLASS, "text-larger"],
            Self::Largest => &[LARGE_TEXT_CLASS, "text-largest"],
        }
    }
}

/// The window's style class at every text size above the default.
const LARGE_TEXT_CLASS: &str = "large-text";

/// Every class `TextSize::classes` gives the window.
pub const TEXT_SIZE_CLASSES: [&str; 4] = [
    LARGE_TEXT_CLASS,
    "text-large",
    "text-larger",
    "text-largest",
];

/// The GSettings key of the former Larger Text setting, a single step that Text Size
/// replaces. The schema keeps it so the choice of an existing install carries over.
pub const LEGACY_LARGE_TEXT_KEY: &str = "fullscreen-large-text";

/// The text size to set once for the former Larger Text setting: `Large`, the size it
/// gave, while `large_text` is on and the Text Size setting is not `text_size_chosen`
/// yet; `None` when there is nothing to carry over.
pub fn migrated_text_size(large_text: bool, text_size_chosen: bool) -> Option<TextSize> {
    (large_text && !text_size_chosen).then_some(TextSize::Large)
}

/// What decides fullscreen mode's motion and type size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accessibility {
    /// GTK's `gtk-enable-animations`, the desktop's own animations setting.
    pub animations_enabled: bool,
    /// GTK's `gtk-interface-reduced-motion` asks for reduced motion.
    pub system_reduced_motion: bool,
    /// The `fullscreen-reduce-motion` setting.
    pub reduce_motion: bool,
    /// The `fullscreen-text-size` setting.
    pub text_size: TextSize,
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

    /// How large fullscreen mode's type is: the Text Size setting's size in fullscreen
    /// mode. The desktop window follows the system's text size alone.
    pub fn text_size(self) -> TextSize {
        if self.fullscreen {
            self.text_size
        } else {
            TextSize::Default
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IN_FULLSCREEN: Accessibility = Accessibility {
        animations_enabled: true,
        system_reduced_motion: false,
        reduce_motion: false,
        text_size: TextSize::Default,
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
    fn the_text_size_setting_sizes_type_in_fullscreen_mode_only() {
        let state = |text_size, fullscreen| Accessibility {
            text_size,
            fullscreen,
            ..IN_FULLSCREEN
        };

        for size in TextSize::ALL {
            assert_eq!(state(size, true).text_size(), size);
            // The desktop window follows the system's text size alone.
            assert_eq!(state(size, false).text_size(), TextSize::Default);
        }
    }

    #[test]
    fn every_text_size_round_trips_through_its_setting() {
        for size in TextSize::ALL {
            assert_eq!(TextSize::from_setting(size.setting()), size);
        }
        assert_eq!(TextSize::from_setting("huge"), TextSize::Default);
    }

    #[test]
    fn each_larger_size_has_its_own_class_besides_large_text() {
        assert!(TextSize::Default.classes().is_empty());
        assert_eq!(TextSize::Large.classes(), ["large-text", "text-large"]);
        assert_eq!(TextSize::Larger.classes(), ["large-text", "text-larger"]);
        assert_eq!(TextSize::Largest.classes(), ["large-text", "text-largest"]);
        for size in TextSize::ALL {
            assert!(
                size.classes()
                    .iter()
                    .all(|class| TEXT_SIZE_CLASSES.contains(class))
            );
        }
    }

    #[test]
    fn the_former_larger_text_setting_carries_over_as_large() {
        assert_eq!(migrated_text_size(true, false), Some(TextSize::Large));
        // A size already chosen wins, and with Larger Text off there is nothing to carry.
        assert_eq!(migrated_text_size(true, true), None);
        assert_eq!(migrated_text_size(false, false), None);
        assert_eq!(migrated_text_size(false, true), None);
    }
}
