//! Build-time configuration. Meson passes the install paths to cargo through the environment;
//! plain cargo builds fall back to the `/usr` layout.

/// `…-devel` in development builds; see the `profile` Meson option.
pub const APP_ID: &str = match option_env!("PINDU_APP_ID") {
    Some(id) => id,
    None => "io.github.tim_van_oudheusden.pindu-launcher",
};
/// The GResource path the application's resources live under.
pub const RESOURCE_BASE_PATH: &str = "/io/github/tim_van_oudheusden/pindu-launcher";
/// The gettext domain of the translations.
pub const GETTEXT_PACKAGE: &str = "pindu-launcher";
/// The crate version, shown in the About dialog.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the compiled translations are installed.
pub const LOCALEDIR: &str = match option_env!("PINDU_LOCALEDIR") {
    Some(dir) => dir,
    None => "/usr/share/locale",
};

/// Where the application's data, such as `resources.gresource`, is installed.
pub const PKGDATADIR: &str = match option_env!("PINDU_PKGDATADIR") {
    Some(dir) => dir,
    None => "/usr/share/pindu-launcher",
};
