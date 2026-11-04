//! Build-time configuration. Meson passes the install paths to cargo through the environment;
//! plain cargo builds fall back to the `/usr` layout.

pub const APP_ID: &str = "io.github.tim_van_oudheusden.pindu-launcher";
pub const RESOURCE_BASE_PATH: &str = "/io/github/tim_van_oudheusden/pindu-launcher";
pub const GETTEXT_PACKAGE: &str = "pindu-launcher";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const LOCALEDIR: &str = match option_env!("PINDU_LOCALEDIR") {
    Some(dir) => dir,
    None => "/usr/share/locale",
};

pub const PKGDATADIR: &str = match option_env!("PINDU_PKGDATADIR") {
    Some(dir) => dir,
    None => "/usr/share/pindu-launcher",
};
