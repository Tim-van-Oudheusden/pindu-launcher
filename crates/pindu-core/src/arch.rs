//! The processor architecture of this machine, which the Proton builds it runs and
//! downloads must be made for.

/// A processor architecture, as far as Proton builds tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    /// 64-bit x86 (`x86_64`, also called amd64).
    X86_64,
    /// 64-bit Arm (`aarch64`, also called arm64).
    Aarch64,
    /// Any other architecture, which no Proton build is made for.
    Other,
}

impl Arch {
    /// The architecture of this machine: the one this program was built for, as
    /// `std::env::consts::ARCH` names it.
    pub fn host() -> Self {
        match std::env::consts::ARCH {
            "x86_64" => Self::X86_64,
            "aarch64" => Self::Aarch64,
            _ => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_is_the_architecture_this_program_was_built_for() {
        let expected = if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else {
            Arch::Other
        };
        assert_eq!(Arch::host(), expected);
    }
}
