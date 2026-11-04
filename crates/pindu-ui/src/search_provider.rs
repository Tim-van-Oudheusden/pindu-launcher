//! GNOME Shell's search: `org.gnome.Shell.SearchProvider2`, answered from the running
//! app's library model.

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::search::matches_terms;

use crate::application::PinduApplication;
use crate::models::GameObject;
use crate::sources::source_name;

/// Where the provider lives on the app's bus name; the `.search-provider.ini` names it.
pub const OBJECT_PATH: &str = "/io/github/tim_van_oudheusden/pindu_launcher/SearchProvider";

/// The most results one search returns.
const MAX_RESULTS: usize = 25;

const INTERFACE: &str = r#"
<node>
  <interface name="org.gnome.Shell.SearchProvider2">
    <method name="GetInitialResultSet">
      <arg type="as" name="terms" direction="in"/>
      <arg type="as" name="results" direction="out"/>
    </method>
    <method name="GetSubsearchResultSet">
      <arg type="as" name="previous_results" direction="in"/>
      <arg type="as" name="terms" direction="in"/>
      <arg type="as" name="results" direction="out"/>
    </method>
    <method name="GetResultMetas">
      <arg type="as" name="identifiers" direction="in"/>
      <arg type="aa{sv}" name="metas" direction="out"/>
    </method>
    <method name="ActivateResult">
      <arg type="s" name="identifier" direction="in"/>
      <arg type="as" name="terms" direction="in"/>
      <arg type="u" name="timestamp" direction="in"/>
    </method>
    <method name="LaunchSearch">
      <arg type="as" name="terms" direction="in"/>
      <arg type="u" name="timestamp" direction="in"/>
    </method>
  </interface>
</node>"#;

/// Exports the provider on the app's connection.
pub fn register(
    app: &PinduApplication,
    connection: &gio::DBusConnection,
) -> Result<gio::RegistrationId, glib::Error> {
    let node = gio::DBusNodeInfo::for_xml(INTERFACE)?;
    let interface = node
        .lookup_interface("org.gnome.Shell.SearchProvider2")
        .expect("declared above");
    let app = app.downgrade();
    connection
        .register_object(OBJECT_PATH, &interface)
        .method_call(move |_, _, _, _, method, parameters, invocation| {
            let Some(app) = app.upgrade() else {
                invocation.return_dbus_error("org.freedesktop.DBus.Error.Failed", "quitting");
                return;
            };
            let method = method.to_owned();
            glib::spawn_future_local(async move {
                let reply = answer(&app, &method, &parameters).await;
                invocation.return_value(reply.as_ref());
            });
        })
        .build()
}

async fn answer(
    app: &PinduApplication,
    method: &str,
    parameters: &glib::Variant,
) -> Option<glib::Variant> {
    match method {
        "GetInitialResultSet" => {
            let (terms,) = parameters.get::<(Vec<String>,)>()?;
            app.library_service().wait_loaded().await;
            let ids: Vec<String> = games(app)
                .filter(|game| matches_terms(&terms, &game.game().title))
                .take(MAX_RESULTS)
                .map(|game| game.id().as_str().to_owned())
                .collect();
            Some((ids,).to_variant())
        }
        "GetSubsearchResultSet" => {
            let (previous, terms) = parameters.get::<(Vec<String>, Vec<String>)>()?;
            let ids: Vec<String> = previous
                .into_iter()
                .filter(|id| {
                    game(app, id).is_some_and(|game| matches_terms(&terms, &game.game().title))
                })
                .collect();
            Some((ids,).to_variant())
        }
        "GetResultMetas" => {
            let (ids,) = parameters.get::<(Vec<String>,)>()?;
            let metas: Vec<glib::VariantDict> = ids
                .iter()
                .filter_map(|id| game(app, id).map(|game| meta(id, &game)))
                .collect();
            let metas = glib::Variant::array_from_iter_with_type(
                glib::VariantTy::VARDICT,
                metas.iter().map(|meta| meta.end()),
            );
            Some(glib::Variant::tuple_from_iter([metas]))
        }
        "ActivateResult" => {
            let (id, _, _) = parameters.get::<(String, Vec<String>, u32)>()?;
            app.activate_action("launch-game", Some(&id.to_variant()));
            None
        }
        "LaunchSearch" => {
            let (terms, _) = parameters.get::<(Vec<String>, u32)>()?;
            app.activate();
            if let Some(window) = app
                .active_window()
                .and_downcast::<crate::window::PinduWindow>()
            {
                window.search_for(&terms.join(" "));
            }
            None
        }
        _ => None,
    }
}

/// The games search can find: in the library and not hidden.
fn games(app: &PinduApplication) -> impl Iterator<Item = GameObject> {
    app.library_service()
        .model()
        .all_games()
        .into_iter()
        .filter(|game| {
            let game = game.game();
            game.in_library() && !game.hidden
        })
}

fn game(app: &PinduApplication, id: &str) -> Option<GameObject> {
    let id = id.parse().ok()?;
    app.library_service().model().game_object(&id)
}

/// A result's name, launcher and icon: its icon or cover file, else a generic one.
fn meta(id: &str, game: &GameObject) -> glib::VariantDict {
    let meta = glib::VariantDict::new(None);
    let entry = game.entry();
    meta.insert("id", id);
    meta.insert("name", entry.game.title.as_str());
    meta.insert(
        "description",
        gettext("Game in {launcher}")
            .replace("{launcher}", &source_name(entry.game.source()))
            .as_str(),
    );
    let icon: gio::Icon = match entry.art.icon.as_ref().or(entry.art.cover.as_ref()) {
        Some(path) => gio::FileIcon::new(&gio::File::for_path(path)).upcast(),
        None => gio::ThemedIcon::new("applications-games-symbolic").upcast(),
    };
    if let Some(icon) = icon.serialize() {
        meta.insert_value("icon", &icon);
    }
    meta
}
