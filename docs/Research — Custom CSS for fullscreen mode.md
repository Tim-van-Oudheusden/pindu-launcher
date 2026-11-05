# Research — Custom CSS for fullscreen mode

Question: can Pindu launcher let users restyle **fullscreen (controller/TV) mode** with their own CSS, the way Bazaar
lets curators restyle its Curated page, while the desktop window stays stock Adwaita?

Researched 2026-11-05 against Pindu `main` at `91f61ba`, Bazaar `main` at
[`3d347d5`](https://github.com/kolunmi/bazaar/tree/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33) (2026-08-18), GTK `main` at
[`cb73e88`](https://gitlab.gnome.org/GNOME/gtk/-/tree/cb73e8847591cfaf400cf5c961319759a5b9b02e) and libadwaita `main`
at [`295aa0e`](https://gitlab.gnome.org/GNOME/libadwaita/-/tree/295aa0e849169f9fd92c2f12f128c5037a82bba0). Claims not
backed by a source or a command run here are marked **[INFERENCE]**.

Verified locally:

- `org.gnome.Sdk//51` ships GTK **4.24.1** and libadwaita **1.10.0** (`pkg-config --modversion gtk4 libadwaita-1` inside
  the SDK).
- Inside the installed `io.github.tim_van_oudheusden.pindu-launcher-devel` Flatpak, `$XDG_CONFIG_HOME` is
  `~/.var/app/io.github.tim_van_oudheusden.pindu-launcher-devel/config`, and the directory exists.
- Inside the same sandbox, `gio cat https://example.com/` and `gio info https://example.com/x.png` both fail with
  `Operation not supported`.

---

## TL;DR

**It is feasible, with no new crate, no new Flatpak permission and no manifest change.** Every piece already exists in
the platform and in Pindu's current dependencies:

- **Load:** `gtk::CssProvider::load_from_path` reads `$XDG_CONFIG_HOME/pindu-launcher/fullscreen.css`, which Flatpak
  maps to `~/.var/app/<id>/config/…`. The sandbox can always read that directory.
- **Install:** `gtk::style_context_add_provider_for_display` adds the provider at `STYLE_PROVIDER_PRIORITY_USER`.
- **Live reload:** `gio::FileMonitor`, using the same pattern as Pindu's `watch_service.rs`.
- **Errors:** `CssProvider::connect_parsing_error`.

**Caveats**

1. **Bazaar does not do this today, and it never read a user CSS file.** From 2025-05-30 to 2026-06-23, Bazaar accepted
   a `css:` string inside *distributor-supplied* curated YAML under `/run/host/etc/bazaar`. It installed that CSS
   display-wide at `PRIORITY_USER`, live-reloaded it through a `GFileMonitor` on the YAML, and documented a list of CSS
   names. The "Rework Curated System" commit removed that feature. Bazaar's real legacy is the *pattern*: free-form CSS
   plus a documented list of node names. It is not a user-theming feature to copy.
2. **CSS providers are display-wide.** Scoping CSS to fullscreen mode is a convention (`window.fullscreen-mode …`) plus a
   mechanism: install the provider only while the window is in fullscreen mode. Pindu's fullscreen mode is the **same
   window** with a `fullscreen-mode` class, not a separate window.
3. **A user provider does not follow dark mode, high contrast or reduced motion automatically.** GtkCssProvider's
   `prefers-*` properties default to light, no preference and no preference. libadwaita syncs them only for the
   providers it owns, so Pindu has to sync its own provider.
4. **The class contract has to be stable.** Today's class names in `data/style.css` are implementation details. A
   feature like this commits Pindu to a documented, versioned contract.
5. **This is a policy question for the maintainer.** The spec says "Adwaita first … no custom theme". An opt-in, user-
   authored stylesheet limited to fullscreen mode does not ship a custom theme, but the spec should say so explicitly.
6. **Zero-code baseline [INFERENCE, not exercised]:** GTK already loads `$XDG_CONFIG_HOME/gtk-4.0/gtk.css` at
   `PRIORITY_USER`. In the sandbox that file is `~/.var/app/<id>/config/gtk-4.0/gtk.css`. A user can therefore already
   write `window.fullscreen-mode .home-carousel { … }` there today, with no live reload, no error reporting and no
   contract.

---

## 1. Bazaar's implementation

### 1.1 What existed: curated-config `css:` blocks (2025-05-30 → 2026-06-23)

- **Origin.** The feature arrived in commit
  [`5cf113d` "ContentProvider: add custom css stuff"](https://github.com/kolunmi/bazaar/commit/5cf113d789eef8fb6c4432c51b70e087eeefe4c4)
  on 2025-05-30. It was restored in
  [`8b54d45` "Curated: restore css functionality"](https://github.com/kolunmi/bazaar/commit/8b54d45e15a58002f8c9538e261aae08270f7821)
  on 2025-11-17.
- **Source of the CSS.** The CSS is a string property of the root curated config,
  [`property=css char G_TYPE_STRING string`](https://github.com/kolunmi/bazaar/blob/ad89ba4c2a453297914fb132f91e52b8679853b0/src/bz-root-curated-config.txt#L9).
  It is written as a YAML `css: |` block inside a curated config file. The docs describe it this way: "curated configs
  allow you to define a css block from which you can reference classes inside sections and change the way gtk renders
  the content"
  ([overview.md L378-L380](https://github.com/kolunmi/bazaar/blob/ad89ba4c2a453297914fb132f91e52b8679853b0/docs/overview.md#L378-L380)).
- **Where the file lives.** No XDG directory, no GSettings key and no user file name is involved. The main YAML config's
  `curated-config-paths:` lists the files, for example `/run/host/etc/bazaar/curated.yaml`, annotated "Flatpak path with
  host-etc permission"
  ([overview.md L105-L109](https://github.com/kolunmi/bazaar/blob/ad89ba4c2a453297914fb132f91e52b8679853b0/docs/overview.md#L105-L109)).
  The main config's own path is a build option, `hardcoded_main_config_path`. Flathub builds it as
  `/run/host/etc/bazaar/bazaar.yaml` "so that distributors can override `host-etc` perms"
  ([Flathub manifest L45-L48](https://github.com/flathub/io.github.kolunmi.Bazaar/blob/dcb456dc671e6f10b3f1b7b3ff6c1a36c02c3ffd/io.github.kolunmi.Bazaar.yaml#L45-L48)).
  Bazaar's docs add that "the build on Flathub doesn't have this permission by default"
  ([overview.md L389](https://github.com/kolunmi/bazaar/blob/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33/docs/overview.md#L389)).
  The audience is therefore distributors such as Bluefin, Aurora and Bazzite
  ([overview.md L386-L388](https://github.com/kolunmi/bazaar/blob/ad89ba4c2a453297914fb132f91e52b8679853b0/docs/overview.md#L386-L388)),
  not end users.
- **API and priority.** For each config, Bazaar created one `gtk_css_provider_new()`, loaded it with
  `gtk_css_provider_load_from_string()` and added it with `gtk_style_context_add_provider_for_display(…,
  GTK_STYLE_PROVIDER_PRIORITY_USER)`. Providers sat in a `GPtrArray` that tracked the model's `items-changed`
  ([bz-curated-view.c L260-L292](https://github.com/kolunmi/bazaar/blob/ad89ba4c2a453297914fb132f91e52b8679853b0/src/bz-curated-view.c#L260-L292)).
  On removal, Bazaar called `gtk_style_context_remove_provider_for_display`
  ([L320-L327](https://github.com/kolunmi/bazaar/blob/ad89ba4c2a453297914fb132f91e52b8679853b0/src/bz-curated-view.c#L320-L327)).
- **Live reload.** Curated files "are constantly monitored for filesystem events, changes immediately reflected in the
  GUI" ([overview.md L17-L18](https://github.com/kolunmi/bazaar/blob/ad89ba4c2a453297914fb132f91e52b8679853b0/docs/overview.md#L17-L18)).
  This used `g_file_monitor_file()`, reloading on `CHANGED`, `CREATED` or `DELETED`
  ([bz-content-provider.c L477-L506](https://github.com/kolunmi/bazaar/blob/ad89ba4c2a453297914fb132f91e52b8679853b0/src/bz-content-provider.c#L477-L506)).
  The monitor watches the YAML, and the CSS is reloaded as part of the reparsed config.
- **Theming surface.** Bazaar gave each Curated widget a stable **CSS node name** through Blueprint `css-name:`, for
  example `app-tile-title`, `app-tile-verified-check` and `app-tile-installed-pill`
  ([bz-app-tile.blp](https://github.com/kolunmi/bazaar/blob/ad89ba4c2a453297914fb132f91e52b8679853b0/src/bz-app-tile.blp#L38-L75)).
  The YAML example lists them as "css names at your disposal" and uses the libadwaita variable
  `var(--accent-bg-color)` ([overview.md L392-L435](https://github.com/kolunmi/bazaar/blob/ad89ba4c2a453297914fb132f91e52b8679853b0/docs/overview.md#L392-L435)).
  Curators defined their own section classes (`.background-1`, `.main-section`). There are no custom CSS variables of
  Bazaar's own and no version number on the list.
- **Error reporting.** None. No code connects `parsing-error`. `git grep parsing-error` at both
  [`ad89ba4`](https://github.com/kolunmi/bazaar/tree/ad89ba4c2a453297914fb132f91e52b8679853b0) and HEAD returns nothing.
  Bad CSS is dropped silently, apart from GTK's console warnings.
- **Dark, light and accent.** The user CSS does nothing to follow appearance. Curators were expected to use libadwaita
  variables, which follow the style manager.

### 1.2 What exists at HEAD (`3d347d5`)

- [`6f37572` "Rework Curated System (#1655)"](https://github.com/kolunmi/bazaar/commit/6f375726838f987252a3e8e515ef098a69f4b9ed)
  on 2026-06-23 removed the `css:` block, the provider loop and the documentation section. At HEAD, `css_providers` is
  still allocated and released but never filled
  ([bz-curated-view.c L37, L208, L317-L325](https://github.com/kolunmi/bazaar/blob/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33/src/bz-curated-view.c#L317-L325)).
- **Structured replacement.** Banners take `light-color` and `dark-color` values that may be any CSS background, for
  example `linear-gradient(…)` or even `url('file:///…') center / cover`
  ([example.yaml L11-L13](https://github.com/kolunmi/bazaar/blob/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33/docs/example.yaml#L11-L13)).
  Bazaar picks the value through `adw_style_manager_get_dark()`, gives the widget a unique class (`bz-banner-%p`) and
  loads a one-rule provider at `PRIORITY_USER`
  ([bz-banner-view.c L259-L306](https://github.com/kolunmi/bazaar/blob/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33/src/bz-banner-view.c#L259-L306)).
  Dark and light are handled by the app choosing the value, not by media queries.
- **Built-in themes.** The only theme picker is a GSettings enum, `global-progress-bar-theme`, with `accent-color` and
  pride-flag choices ([gschema L24-L52](https://github.com/kolunmi/bazaar/blob/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33/data/io.github.kolunmi.Bazaar.gschema.xml#L24-L52)).
  Code generates its CSS
  ([pride.c L142](https://github.com/kolunmi/bazaar/blob/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33/src/progress-bar-designs/pride/pride.c#L142)),
  and Bazaar installs it at `PRIORITY_APPLICATION`
  ([bz-application.c L3649-L3652](https://github.com/kolunmi/bazaar/blob/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33/src/bz-application.c#L3649-L3652)).
- **Performance lesson.** [`a52137b` "Fix lag in search by removing css compilation"](https://github.com/kolunmi/bazaar/commit/a52137bf9935dfadf38ae6609053410a75475241)
  replaced per-tile CSS providers with custom drawing. Creating many display-wide providers at runtime cost enough to
  cause visible lag.

**Takeaway for Pindu:** copy Bazaar's *documented node-name list* and its *GFileMonitor reload*. Do not copy the gaps
(no error reporting, no appearance sync, an unversioned list), and keep a single provider.

---

## 2. GTK and libadwaita mechanism

### 2.1 GtkCssProvider

- **Loaders.** `load_from_path`, `load_from_file`, `load_from_resource`, `load_from_string` (since 4.12) and
  `load_from_bytes` (since 4.12). `load_from_data` is deprecated since 4.12, and `load_named` since 4.20
  ([GtkCssProvider](https://docs.gtk.org/gtk4/class.CssProvider.html)). `load_from_path` "clears any previously loaded
  information" ([load_from_path](https://docs.gtk.org/gtk4/method.CssProvider.load_from_path.html)), so a reload
  replaces the old rules in place.
- **Errors.** In GTK's words, "Parsing errors are never fatal, so the parsing will resume after the error". The signal
  "may be emitted at any time" because parsing can be deferred, and `GtkCssParserWarning` values "should not be treated
  as fatal" ([parsing-error](https://docs.gtk.org/gtk4/signal.CssProvider.parsing-error.html)). The signal carries a
  `GtkCssSection` with start and end locations (line and character).
- **GTK's own user file.** GTK loads `$XDG_CONFIG_HOME/gtk-4.0/gtk.css` at `PRIORITY_USER` and binds its `prefers-*`
  properties to `GtkSettings` `gtk-interface-*`
  ([gtksettings.c L1186-L1212](https://gitlab.gnome.org/GNOME/gtk/-/blob/cb73e8847591cfaf400cf5c961319759a5b9b02e/gtk/gtksettings.c#L1186-L1212)).
  It loads the file once (`if (G_UNLIKELY (!css_provider))`), with no monitor.

### 2.2 Priorities and display-wide providers

| Constant | Value | Meaning (docs.gtk.org) |
|---|---|---|
| [`PRIORITY_THEME`](https://docs.gtk.org/gtk4/const.STYLE_PROVIDER_PRIORITY_THEME.html) | 200 | theme style information |
| [`PRIORITY_SETTINGS`](https://docs.gtk.org/gtk4/const.STYLE_PROVIDER_PRIORITY_SETTINGS.html) | 400 | GtkSettings, overrides themes |
| [`PRIORITY_APPLICATION`](https://docs.gtk.org/gtk4/const.STYLE_PROVIDER_PRIORITY_APPLICATION.html) | 600 | application-specific style |
| [`PRIORITY_USER`](https://docs.gtk.org/gtk4/const.STYLE_PROVIDER_PRIORITY_USER.html) | 800 | `gtk-4.0/gtk.css`; "You should not use priorities higher than this, to give the user the last word." |

- **Where Adwaita and Pindu's CSS sit.** libadwaita installs Adwaita, the accent and fonts at `PRIORITY_THEME`
  ([adw-style-manager.c L458-L472](https://gitlab.gnome.org/GNOME/libadwaita/-/blob/295aa0e849169f9fd92c2f12f128c5037a82bba0/src/adw-style-manager.c#L458-L472)).
  The app's `style.css` resource goes in at `PRIORITY_APPLICATION`
  ([adw-application.c L146-L156, L278-L281](https://gitlab.gnome.org/GNOME/libadwaita/-/blob/295aa0e849169f9fd92c2f12f128c5037a82bba0/src/adw-application.c#L146-L156)).
  A user provider at `PRIORITY_USER` therefore overrides both Adwaita and Pindu's `data/style.css`.
- **No per-widget providers.** `gtk_style_context_add_provider()` is **deprecated since 4.10** ("Use style classes
  instead") ([docs](https://docs.gtk.org/gtk4/method.StyleContext.add_provider.html)). The supported route is
  [`gtk_style_context_add_provider_for_display()`](https://docs.gtk.org/gtk4/type_func.StyleContext.add_provider_for_display.html),
  which is "used in style construction for all `GtkStyleContexts` under `display`". Scoping therefore has to come from
  selectors or from when the provider is installed (§3).

### 2.3 CSS features on GNOME 51 (GTK 4.24)

- **Custom properties.** `--x`, `var()` and fallbacks have worked since **4.16**. `:root` plus `var()` replaces
  `@define-color`
  ([css-properties: Custom Properties](https://docs.gtk.org/gtk4/css-properties.html#custom-properties)). `:root` is a
  supported selector ([css-overview](https://docs.gtk.org/gtk4/css-overview.html)).
- **Media queries.** Supported **since 4.20**, with `prefers-color-scheme`, `prefers-contrast` and
  `prefers-reduced-motion` ([css-properties: Media Queries](https://docs.gtk.org/gtk4/css-properties.html#media-queries)).
  The matching provider properties arrived in 4.20 and 4.22
  ([prefers-color-scheme](https://docs.gtk.org/gtk4/property.CssProvider.prefers-color-scheme.html),
  [prefers-contrast](https://docs.gtk.org/gtk4/property.CssProvider.prefers-contrast.html),
  [prefers-reduced-motion](https://docs.gtk.org/gtk4/property.CssProvider.prefers-reduced-motion.html)).
- **Media queries need a sync.** `prefers-*` are **per-provider properties** whose defaults are light, no preference and
  no preference ([gtkcssprovider.c L656-L657](https://gitlab.gnome.org/GNOME/gtk/-/blob/cb73e8847591cfaf400cf5c961319759a5b9b02e/gtk/gtkcssprovider.c#L656-L657)).
  AdwApplication pushes `AdwStyleManager:dark`, `:high-contrast` and `GtkSettings:gtk-interface-reduced-motion` into
  **its own** provider
  ([adw-application.c L160-L190](https://gitlab.gnome.org/GNOME/libadwaita/-/blob/295aa0e849169f9fd92c2f12f128c5037a82bba0/src/adw-application.c#L160-L190)).
  Pindu must do the same for the user provider, or `@media (prefers-color-scheme: dark)` in user CSS never matches.
- **Nesting.** CSS nesting (`&`) is not in GTK's selector table
  ([css-overview](https://docs.gtk.org/gtk4/css-overview.html#selectors)). **[INFERENCE]** It is not supported, so every
  user rule must spell out its `window.fullscreen-mode` prefix.
- **Images.** `url()` resolves to a `GFile` and loads through `gdk_texture_new_from_file()`. `data:` URIs are decoded
  inline ([gtkcssimageurl.c L35-L76, L182-L205](https://gitlab.gnome.org/GNOME/gtk/-/blob/cb73e8847591cfaf400cf5c961319759a5b9b02e/gtk/gtkcssimageurl.c#L35-L76)).
  `@import` reads any `GFile` through `g_file_load_bytes()`
  ([gtkcssprovider.c L1014-L1090](https://gitlab.gnome.org/GNOME/gtk/-/blob/cb73e8847591cfaf400cf5c961319759a5b9b02e/gtk/gtkcssprovider.c#L1014-L1090)).

### 2.4 libadwaita

- **Variables are meant to be overridden.** "Applications can override these colors by re-declaring them", for example
  `:root { --accent-bg-color: #e01b24; }`. The helper variables (opacities) "should not be overridden"
  ([CSS Variables](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.10/css-variables.html)).
- **Accent.** Apps may override the accent globally or per widget. When they do, `AdwStyleManager` "will still return
  the system color" ([Styles & Appearance](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.10/styles-and-appearance.html)).
- **High contrast.** "Libadwaita applications automatically support it; applications cannot disable it" (same page). A
  user stylesheet at `PRIORITY_USER` can still *visually* defeat it (§6).
- **style-dark.css and style-hc.css.** Deprecated since 1.9 in favour of media queries in `style.css`
  ([AdwApplication](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.10/class.Application.html)).
- **Rust API.** [`adw::StyleManager`](https://docs.rs/libadwaita/0.9.2/libadwaita/struct.StyleManager.html) exposes
  `is_dark()` and `is_high_contrast()` with notify signals, which drive the provider sync.

### 2.5 gtk-rs API (gtk4 0.11.5, gio and glib 0.22.10, from `Cargo.lock`)

Checked against the published crate sources:

- **CssProvider.** [`gtk::CssProvider`](https://docs.rs/gtk4/0.11.5/gtk4/struct.CssProvider.html) has `load_from_path`
  (`src/auto/css_provider.rs:79`), `load_from_file` (`:69`) and `load_from_string` (`:101`, `#[cfg(feature =
  "v4_12")]`, enabled because Pindu uses `v4_24`). It also has `set_prefers_color_scheme`, `set_prefers_contrast`,
  `set_prefers_reduced_motion`, and `connect_parsing_error(Fn(&CssProvider, &CssSection, &glib::Error))` (`:169`).
  `CssSection::start_location().lines()` gives the error line.
- **Display functions.** [`gtk::style_context_add_provider_for_display`](https://docs.rs/gtk4/0.11.5/gtk4/fn.style_context_add_provider_for_display.html)
  and [`gtk::style_context_remove_provider_for_display`](https://docs.rs/gtk4/0.11.5/gtk4/fn.style_context_remove_provider_for_display.html)
  are free functions (`src/functions.rs:268, :285`). `gtk::STYLE_PROVIDER_PRIORITY_USER` is defined at `src/lib.rs:43`.
- **Monitoring.** [`gio::prelude::FileExt::monitor_file` / `monitor_directory`](https://docs.rs/gio/0.22.10/gio/prelude/trait.FileExt.html)
  and [`FileMonitorExt::connect_changed` / `set_rate_limit`](https://docs.rs/gio/0.22.10/gio/prelude/trait.FileMonitorExt.html).
  [`glib::user_config_dir`](https://docs.rs/glib/0.22.10/glib/fn.user_config_dir.html) exists but is not needed (§5).
- **No new crate.** All of these live in `gtk4`, `gio` and `glib`, which `pindu-ui` already depends on
  (`crates/pindu-ui/Cargo.toml`), plus `libadwaita` for the StyleManager.

---

## 3. Scoping to fullscreen mode only

Repo facts:

- Fullscreen mode is **the same `PinduWindow`**. `show_fullscreen_mode()` adds or removes the `fullscreen-mode` window
  class as `fullscreened` changes (`crates/pindu-ui/src/fullscreen.rs:21-22, 136-146`).
- `data/style.css` already scopes its fullscreen rules as `window.fullscreen-mode …` (`data/style.css:80-94, 153-165`).
- The window also carries `reduced-motion` (`crates/pindu-ui/src/window.rs:1176-1193, 1211`).

Two complementary measures:

1. **Mechanism (recommended):** add the user provider to the display only while the window is in fullscreen mode and
   the setting is on. Remove it when either is false. Hook this into `show_fullscreen_mode()`, the one place the mode
   changes. Nothing the user writes can then reach the desktop shell, whatever selectors they use. This also covers
   anything that might not sit under the window's CSS node, such as dialogs and popovers **[INFERENCE: node placement
   of AdwDialog/GtkPopover not verified]**. Cost: one display-wide restyle per mode switch, which also happens on any
   theme change **[INFERENCE]**.
2. **Convention (documented):** write rules as `window.fullscreen-mode <contract class> { … }`, and put variable
   overrides on `window.fullscreen-mode { --accent-bg-color: …; }` rather than `:root`. Selectors then stay correct
   if the mechanism changes later, and they compose with Pindu's own `window.fullscreen-mode` rules.

Because fullscreen mode is a single window class, there is no `:root` and no separate window to target. Measure 1 makes
`:root { … }` safe for users anyway.

---

## 4. Sandbox and Flathub

- **Readable without finish-args.** Inside a Flatpak, "`$XDG_CONFIG_HOME` … `~/.var/app/<app-id>/config`", always set
  by Flatpak ([Flatpak conventions](https://docs.flatpak.org/en/latest/conventions.html)). The sandbox can always reach
  "`~/.var/app/$FLATPAK_ID`" ([Sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)).
  The XDG spec makes `$HOME/.config` the default when the variable is unset or empty, and says to ignore relative paths
  ([XDG Base Directory spec](https://specifications.freedesktop.org/basedir-spec/latest/)). Verified here:
  `$XDG_CONFIG_HOME` inside the devel Flatpak is `~/.var/app/io.github.tim_van_oudheusden.pindu-launcher-devel/config`.
- **Not HostDirs.** Pindu's `HostDirs::from_vars` deliberately resolves the **host's** config home, through
  `HOST_XDG_CONFIG_HOME` (`crates/pindu-core/src/sources/location.rs:35-51`). That is the wrong base for the app's own
  file. Use the sandbox's `XDG_CONFIG_HOME`, as `paths::data_home` does for data (`crates/pindu-core/src/paths.rs:8-15`).
- **Picking a file.** `gtk::FileDialog`, already used in `application.rs:521` and `game_dialog.rs:330`, goes through the
  FileChooser portal. GTK supports portals transparently
  ([Sandbox permissions → Portals](https://docs.flatpak.org/en/latest/sandbox-permissions.html)). Flathub makes a
  portal mandatory where one covers the use case
  ([Flathub requirements → Permissions](https://docs.flathub.org/docs/for-app-authors/requirements)). The picked file
  arrives as a document-portal path, which Pindu already understands (`paths::document_portal_parts`,
  `crates/pindu-core/src/paths.rs:60-78`). **Recommendation:** have "Import Style…" copy the picked file into the config
  folder rather than keep a portal path. That avoids depending on document-store persistence **[INFERENCE: persistence
  semantics not tested]** and means `url()` and `@import` resolve inside the config folder.
- **Opening the folder.** "Open Style Folder" through `gtk::FileLauncher` uses the OpenURI portal
  ([GtkFileLauncher](https://docs.gtk.org/gtk4/class.FileLauncher.html)) **[INFERENCE: portal use under Flatpak not
  exercised here]**.
- **No permission is needed. Nothing to flag for a manifest change.** Avoid Bazaar's route: `/etc` needs `host-etc`
  ([Sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)).
- **Flathub policy.** The Flathub requirements and quality guidelines say nothing about in-app theming. The quality
  guidelines cover icons, screenshots and metadata
  ([quality guidelines](https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines/quality-guidelines)).
  **[INFERENCE]** An opt-in user stylesheet does not affect Flathub review.
- **GNOME Circle and the HIG.** The app criteria require "Follows the Human Interface Guidelines", "Dark mode works" and
  "High-contrast mode works"
  ([AppCriteria.md L32-L66](https://gitlab.gnome.org/Teams/Releng/AppOrganization/-/blob/bd049d3cd775af997dccc229f4a949bd0a04b2da/AppCriteria.md#L32-L66)).
  The HIG says "keep the amount of this custom styling to a minimum" and asks apps to use "existing style classes and
  color variables … since these automatically adjust for the light, dark and high-contrast styles"
  ([HIG: UI Styling](https://developer.gnome.org/hig/guidelines/ui-styling.html#custom-styling)). Circle reviewers judge
  the shipped app. **[INFERENCE]** A default-off user override that never touches the desktop shell keeps the stock
  experience reviewable, but a reviewer could still question it.

---

## 5. Proposed design for Pindu

Architecture rule: "Shells never read files" (`AGENTS.md:109-110`). Path resolution and any pure logic go in
**pindu-core**. The I/O, monitor and provider live in a **pindu-ui service**, alongside `watch_service.rs` and
`file_service.rs`, not in `fullscreen.rs`. The fullscreen shell only tells the service the mode changed.

| Concern | Design |
|---|---|
| **Location** | `$XDG_CONFIG_HOME/pindu-launcher/fullscreen.css`, so the file is `~/.var/app/<id>/config/pindu-launcher/fullscreen.css` in Flatpak. Core adds `paths::config_home(xdg_config_home, home)` and `paths::fullscreen_style_path(…)`, mirroring `data_home` (XDG rules: unset, empty or relative → `~/.config`). `url("bg.png")` and `@import "x.css"` resolve next to it. |
| **Opt-in** | New GSettings key `fullscreen-custom-style` (`b`, default `false`) next to `start-in-fullscreen` (`data/…gschema.xml.in:75-79`). The UI is a switch row in Preferences → Fullscreen (`data/ui/preferences.blp:210-218`). |
| **Install** | One `gtk::CssProvider`. `style_context_add_provider_for_display(display, &p, STYLE_PROVIDER_PRIORITY_USER)` only while `fullscreen-mode` is active and the key is true; otherwise `remove_provider_for_display` (§3). |
| **Appearance sync** | Set `prefers-color-scheme` and `prefers-contrast` from `adw::StyleManager` (`dark`, `high-contrast`), and `prefers-reduced-motion` from `gtk::Settings`, on change. This mirrors `adw-application.c` `update_css_provider`. |
| **Live reload** | `monitor_directory` on the folder with `FileMonitorFlags::WATCH_MOVES` ([flags](https://docs.gtk.org/gio/flags.FileMonitorFlags.html)), filtered to the file name and debounced as in `watch_service.rs:18-25`. Watching the directory rather than the file catches editors that save by rename **[INFERENCE]**. On delete, `load_from_string("")`. |
| **Error surfacing** | `connect_parsing_error` collects `(line, message)`, cleared before each load. Warnings (`GtkCssParserWarning`) are listed but not counted as failures. Preferences shows "Custom style: N problems" with the first few `line N: message`. Fullscreen mode shows one toast per reload with problems, so a controller user sees that something broke. Formatting is pure, so it lives in core with tests. |
| **Reset and escape hatch** | Turning the switch off removes the provider immediately. Leaving fullscreen through Esc, F11, the Guide button or the drawer is driven by actions and keys (`fullscreen.rs:39-51`), not by CSS, so a broken style cannot trap the user **[INFERENCE: CSS can hide the drawer button but not disable the key/Guide paths]**. A "Reset" button turns the key off and renames the file to `fullscreen.css.bak` rather than deleting it. |
| **Class contract** | A documented, versioned list of what user CSS may target: `window.fullscreen-mode`, `window.reduced-motion`, `.home`, `.home .hero-gradient`, `.home-carousel`, `.library-tile`, `.library-tabs`, `gridview.library`, `.cover-tile`, `cover-art` (node name), `.cover-tile .badge`, `.cover-tile .play-button`, `.game-hero`, `.game-actions`, `.game-play`, `.playing-card`, `.launch-overlay`, `.menu-drawer`, `.options-sheet`, `.search-keyboard`, `.osk-key`, `.osk-key.cursor`, `.hint-bar`, and libadwaita variables. Sources: `data/ui/*.blp`, `search_keyboard.rs:18,31`, `widgets/cover_art.rs:39`. Add a contract version (`/* pindu-style-contract: 1 */` in the docs) and treat renames as breaking changes listed in the changelog. Missing hooks to add before publishing the contract: classes on hint-bar items and glyphs, the tab labels, and the fullscreen game-page root. |
| **Example** | Ship one neutral, documented sample with no console look (`AGENTS.md:100-101`), using only contract classes and `var(--accent-bg-color)` / `var(--window-bg-color)`. Keep it as documentation rather than a built-in theme picker, which would be a custom theme. |

---

## 6. Risks and limits

- **Styling only, no structure.** "Every widget has one or more of these CSS nodes, and determines their name, state,
  style classes and how they are laid out" ([css-overview](https://docs.gtk.org/gtk4/css-overview.html#css-nodes)).
  User CSS can change colours, borders, radii, padding, margins, min sizes, fonts, transitions, animations, transforms
  and background images, but cannot add, remove or reorder widgets
  ([css-properties](https://docs.gtk.org/gtk4/css-properties.html)). Layout requests ("move the hint bar to the top")
  remain code changes.
- **Performance on TV hardware.** Large `box-shadow` blur, `filter` and infinite `animation`s are drawn every frame
  **[INFERENCE]**. Bazaar's lag from many providers
  ([a52137b](https://github.com/kolunmi/bazaar/commit/a52137bf9935dfadf38ae6609053410a75475241)) argues for a
  **single** provider and a debounced reload.
- **Breakage across releases.** Any class rename silently drops user rules, because GTK does not warn about selectors
  that match nothing **[INFERENCE]**. Hence the versioned contract and changelog entries. Class names in `data/style.css`
  become public API once listed.
- **Accessibility.** At `PRIORITY_USER`, user colours override libadwaita's high-contrast palette. User `transition` and
  `animation` bypass Pindu's `window.reduced-motion` rules unless the author scopes them. Mitigations: sync `prefers-*`
  (§5) and document `@media (prefers-contrast: more)` / `(prefers-reduced-motion: reduce)` in the sample. Optionally,
  suspend the user provider while high contrast is on (decision for the maintainer). The HIG warns against colour-only
  cues and flashing ([HIG accessibility considerations](https://developer.gnome.org/hig/guidelines/ui-styling.html#accessibility-considerations)).
- **Security.**
  - `url()` and `@import` read any `GFile` the sandbox can open
    ([gtkcssimageurl.c](https://gitlab.gnome.org/GNOME/gtk/-/blob/cb73e8847591cfaf400cf5c961319759a5b9b02e/gtk/gtkcssimageurl.c#L50),
    [gtkcssprovider.c L1079](https://gitlab.gnome.org/GNOME/gtk/-/blob/cb73e8847591cfaf400cf5c961319759a5b9b02e/gtk/gtkcssprovider.c#L1079)).
    The result is only rendered as an image or parsed as CSS, with no way to send it anywhere **[INFERENCE]**.
  - GTK has no HTTP code of its own. Remote URIs would need GVfs, which Flatpak gates behind
    `--talk-name=org.gtk.vfs.*` and `xdg-run/gvfsd`
    ([Sandbox permissions → gvfs access](https://docs.flatpak.org/en/latest/sandbox-permissions.html)). Pindu has
    neither, and `gio cat https://…` inside the Pindu devel sandbox fails with `Operation not supported` (run here).
    **No network fetch from user CSS in the Flatpak.** Outside Flatpak, with gvfsd running, remote `url()` may resolve
    **[INFERENCE]**.
  - The file is user-owned in the app's own config folder, so the trust boundary is the user's own account.
- **Policy.** The spec reads "Adwaita first. Stock libadwaita widgets, system accent colour and typography; no custom
  theme" (`docs/Adwaita Game Library — Main Spec.pdf`, Principles). This feature needs an explicit spec amendment:
  "user-authored, opt-in, fullscreen-only stylesheet; nothing shipped".

---

## 7. Proposed issue split (M4 Fullscreen mode, or post-M4)

1. **Resolve the fullscreen style path in core** (`area:core`, `type:feature`).
   - Add `paths::config_home` and `paths::fullscreen_style_path`, following the XDG rules, with tests.
   - Add pure formatting of CSS parse problems for display (`line N: message`), with tests.
2. **Add a fullscreen style service** (`area:ui`, `type:feature`; depends on 1).
   - One `CssProvider` at `PRIORITY_USER`, installed only in fullscreen mode while the setting is on.
   - `prefers-*` synced from `adw::StyleManager` and `gtk::Settings`.
   - Debounced directory monitor and `parsing-error` collection.
   - A toast in fullscreen mode when a reload has problems.
3. **Add the custom-style setting and Preferences rows** (`area:ui`, `type:feature`; depends on 2).
   - GSettings key `fullscreen-custom-style` with a switch row.
   - Rows for the path, "Open Style Folder" (FileLauncher), "Import Style…" (FileDialog, copies the file into the folder)
     and "Reset", plus a problem summary.
4. **Publish the fullscreen style contract** (`type:chore`; docs plus small UI class additions).
   - A versioned class list and a neutral sample stylesheet showing media queries.
   - Add the missing hooks (hint-bar items, tab labels, game-page root) and a changelog rule for renames.
5. **Spike/decision (maintainer): amend the spec's "no custom theme" principle** for an opt-in, fullscreen-only user
   stylesheet, and decide whether user CSS is suspended under high contrast.

No issue needs a Flatpak permission or a manifest change.

---

## Sources

**Bazaar**
- HEAD tree: <https://github.com/kolunmi/bazaar/tree/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33>
- Pre-rework tree: <https://github.com/kolunmi/bazaar/tree/ad89ba4c2a453297914fb132f91e52b8679853b0>
- Commits: [5cf113d](https://github.com/kolunmi/bazaar/commit/5cf113d789eef8fb6c4432c51b70e087eeefe4c4),
  [8b54d45](https://github.com/kolunmi/bazaar/commit/8b54d45e15a58002f8c9538e261aae08270f7821),
  [a52137b](https://github.com/kolunmi/bazaar/commit/a52137bf9935dfadf38ae6609053410a75475241),
  [6f37572](https://github.com/kolunmi/bazaar/commit/6f375726838f987252a3e8e515ef098a69f4b9ed)
- Flathub manifest: <https://github.com/flathub/io.github.kolunmi.Bazaar/blob/dcb456dc671e6f10b3f1b7b3ff6c1a36c02c3ffd/io.github.kolunmi.Bazaar.yaml>

**GTK**
- GtkCssProvider: <https://docs.gtk.org/gtk4/class.CssProvider.html>
- parsing-error: <https://docs.gtk.org/gtk4/signal.CssProvider.parsing-error.html>
- add_provider_for_display: <https://docs.gtk.org/gtk4/type_func.StyleContext.add_provider_for_display.html>
- add_provider (deprecated 4.10): <https://docs.gtk.org/gtk4/method.StyleContext.add_provider.html>
- Priority constants: <https://docs.gtk.org/gtk4/const.STYLE_PROVIDER_PRIORITY_USER.html> (and `_APPLICATION`,
  `_SETTINGS`, `_THEME`)
- CSS overview: <https://docs.gtk.org/gtk4/css-overview.html>
- CSS properties: <https://docs.gtk.org/gtk4/css-properties.html>
- GtkFileDialog: <https://docs.gtk.org/gtk4/class.FileDialog.html>
- GtkFileLauncher: <https://docs.gtk.org/gtk4/class.FileLauncher.html>
- GFileMonitorFlags: <https://docs.gtk.org/gio/flags.FileMonitorFlags.html>
- g_file_monitor_file: <https://docs.gtk.org/gio/method.File.monitor_file.html>
- GTK source at `cb73e88`: `gtk/gtksettings.c`, `gtk/gtkcssprovider.c`, `gtk/gtkcssimageurl.c` (links inline)

**libadwaita**
- CSS Variables: <https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.10/css-variables.html>
- Styles & Appearance: <https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.10/styles-and-appearance.html>
- AdwApplication: <https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.10/class.Application.html>
- Source at `295aa0e`: `src/adw-application.c`, `src/adw-style-manager.c` (links inline)

**gtk-rs**
- <https://docs.rs/gtk4/0.11.5/gtk4/struct.CssProvider.html>
- <https://docs.rs/gtk4/0.11.5/gtk4/fn.style_context_add_provider_for_display.html>
- <https://docs.rs/gtk4/0.11.5/gtk4/fn.style_context_remove_provider_for_display.html>
- <https://docs.rs/gio/0.22.10/gio/prelude/trait.FileExt.html>
- <https://docs.rs/gio/0.22.10/gio/prelude/trait.FileMonitorExt.html>
- <https://docs.rs/glib/0.22.10/glib/fn.user_config_dir.html>
- <https://docs.rs/libadwaita/0.9.2/libadwaita/struct.StyleManager.html>

**GNOME policy**
- HIG, UI Styling: <https://developer.gnome.org/hig/guidelines/ui-styling.html>
- App Criteria: <https://gitlab.gnome.org/Teams/Releng/AppOrganization/-/blob/bd049d3cd775af997dccc229f4a949bd0a04b2da/AppCriteria.md>
- Circle review procedure: <https://gitlab.gnome.org/Teams/Circle/-/blob/228513a5d1c1043eb1335015a885d7c398b49358/review_procedure.md#L45-L49>

**Flatpak, Flathub and XDG**
- <https://docs.flatpak.org/en/latest/conventions.html>
- <https://docs.flatpak.org/en/latest/sandbox-permissions.html>
- <https://docs.flathub.org/docs/for-app-authors/requirements>
- <https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines/quality-guidelines>
- <https://specifications.freedesktop.org/basedir-spec/latest/>

**Pindu (at `91f61ba`)**
- `crates/pindu-ui/src/fullscreen.rs`, `window.rs`, `watch_service.rs`, `file_service.rs`, `search_keyboard.rs`,
  `widgets/cover_art.rs`
- `crates/pindu-core/src/paths.rs`, `sources/location.rs`
- `data/style.css`, `data/ui/*.blp`, `data/io.github.tim_van_oudheusden.pindu-launcher.gschema.xml.in`
- `AGENTS.md`, `docs/Adwaita Game Library — Main Spec.pdf`
