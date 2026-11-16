//! Fullscreen mode's system panel (`docs/system-panel.md`): what lets the owner of a console
//! run the machine with a controller only. Each section has its pure logic and the trait
//! of the backend a UI service drives here, so every flow is tested with a fake backend.
//!
//! This module holds the rules of "What is available" that the sections share: a backend
//! reports what it found out about the service behind a section, or one of its actions, as
//! [`ServiceFacts`], and [`ServiceFacts::availability`] says whether it is hidden, shown,
//! or shown insensitive because the system does not allow it. A missing service or
//! permission is never shown as an error.

pub mod audio;
pub mod bluetooth;
pub mod network;
pub mod power;
pub mod update;

/// What polkit answers, directly or through the service, about an action the session may
/// take: logind's `CanPowerOff` and friends answer `yes`, `challenge`, `no` or `na`, and
/// NetworkManager's `GetPermissions` `yes`, `auth` or `no`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Permission {
    /// Allowed (`yes`).
    #[default]
    Yes,
    /// Allowed once a password is given (`challenge`, `auth`). It counts as allowed: on a
    /// desktop its polkit agent asks, and on Pindu OS its rules make the action a `yes`.
    Challenge,
    /// Not allowed (`no`).
    No,
    /// Not possible on this machine at all (`na`), such as suspending without the
    /// firmware for it.
    Na,
}

impl Permission {
    /// The answer `answer` gives. Anything it does not know, such as no answer yet, counts
    /// as allowed: the action can still be tried, and says why if it is refused.
    pub fn parse(answer: &str) -> Self {
        match answer {
            "challenge" | "auth" => Self::Challenge,
            "no" => Self::No,
            "na" => Self::Na,
            _ => Self::Yes,
        }
    }
}

/// What a backend found out about the service behind a section, or one of its actions.
/// The default is nothing found: no system bus, which is the Flatpak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ServiceFacts {
    /// Whether the system bus can be reached: never in the Flatpak, whose `finish-args`
    /// grant none.
    pub system_bus: bool,
    /// Whether the service has an owner on the system bus, or can be activated
    /// (`ListActivatableNames`).
    pub service: bool,
    /// Whether the hardware the section is about is there, such as a Bluetooth adapter;
    /// `true` for one that needs none.
    pub hardware: bool,
    /// What polkit answers for the action.
    pub permission: Permission,
}

/// Whether a section, or one of its entries, shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Availability {
    /// It does not show: there is no system bus, no service, no hardware, or the action is
    /// not possible on this machine.
    #[default]
    Hidden,
    /// The service is there, but polkit does not let the session use it: the entry shows,
    /// insensitive, saying so.
    Denied,
    /// It shows.
    Shown,
}

impl ServiceFacts {
    /// Whether what the facts describe shows, by the rules of "What is available": hidden
    /// without a system bus, a service, the hardware, or with polkit answering `na`;
    /// insensitive with polkit answering `no`; shown with `yes` or `challenge`. A radio
    /// that is switched off still shows: the section shows its state and its switch.
    pub fn availability(&self) -> Availability {
        if !self.system_bus || !self.service || !self.hardware {
            return Availability::Hidden;
        }
        match self.permission {
            Permission::Yes | Permission::Challenge => Availability::Shown,
            Permission::No => Availability::Denied,
            Permission::Na => Availability::Hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A service that is there, with its hardware, answering `permission`.
    fn there(permission: Permission) -> ServiceFacts {
        ServiceFacts {
            system_bus: true,
            service: true,
            hardware: true,
            permission,
        }
    }

    #[test]
    fn polkit_answers_are_read_by_both_services_words() {
        assert_eq!(Permission::parse("yes"), Permission::Yes);
        assert_eq!(Permission::parse("challenge"), Permission::Challenge);
        assert_eq!(Permission::parse("auth"), Permission::Challenge);
        assert_eq!(Permission::parse("no"), Permission::No);
        assert_eq!(Permission::parse("na"), Permission::Na);
    }

    #[test]
    fn an_answer_not_known_counts_as_allowed() {
        assert_eq!(Permission::parse(""), Permission::Yes);
        assert_eq!(Permission::parse("maybe"), Permission::Yes);
    }

    #[test]
    fn the_flatpak_without_a_system_bus_hides_everything() {
        let nothing = ServiceFacts::default();
        assert_eq!(nothing.availability(), Availability::Hidden);
        let flatpak = ServiceFacts {
            system_bus: false,
            ..there(Permission::Yes)
        };
        assert_eq!(flatpak.availability(), Availability::Hidden);
    }

    #[test]
    fn a_service_that_is_not_there_or_cannot_be_activated_is_hidden() {
        let missing = ServiceFacts {
            service: false,
            ..there(Permission::Yes)
        };
        assert_eq!(missing.availability(), Availability::Hidden);
    }

    #[test]
    fn a_service_without_its_hardware_is_hidden() {
        let no_adapter = ServiceFacts {
            hardware: false,
            ..there(Permission::Yes)
        };
        assert_eq!(no_adapter.availability(), Availability::Hidden);
    }

    #[test]
    fn yes_and_challenge_show_no_is_insensitive_and_na_hides() {
        assert_eq!(there(Permission::Yes).availability(), Availability::Shown);
        assert_eq!(
            there(Permission::Challenge).availability(),
            Availability::Shown
        );
        assert_eq!(there(Permission::No).availability(), Availability::Denied);
        assert_eq!(there(Permission::Na).availability(), Availability::Hidden);
    }

    #[test]
    fn a_denied_action_without_its_service_is_still_hidden() {
        let missing = ServiceFacts {
            service: false,
            ..there(Permission::No)
        };
        assert_eq!(missing.availability(), Availability::Hidden);
    }
}
