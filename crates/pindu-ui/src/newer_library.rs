//! What both shells say when the library was saved by a newer version of the app, after a
//! rollback or an older bundle installed over a newer one. The store refuses such a
//! library and leaves it as it is (`pindu_core::store::Store::open`); this alert names the
//! cause and the ways forward: update the app again, or start from the copy kept before
//! the last upgrade when the core found one this version opens
//! (`pindu_core::store::kept_copy_to_open`). Both shells show it as an Adwaita alert,
//! which a controller reads and answers alone in fullscreen mode.

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::library_service::NewerLibrary;
use crate::window::PinduWindow;

impl PinduWindow {
    /// Shows that the library is from a newer version, and offers to start from its kept
    /// copy when there is one.
    pub fn show_newer_library(&self, newer: &NewerLibrary) {
        let dialog = adw::AlertDialog::builder()
            .heading(gettext("Library From a Newer Version"))
            .body(body(newer.kept_copy.is_some()))
            .close_response("close")
            .default_response("close")
            .build();
        dialog.add_response("close", &gettext("_Close"));
        if let Some(copy) = newer.kept_copy.clone() {
            dialog.add_response("kept-copy", &gettext("_Start From Kept Copy"));
            dialog.set_response_appearance("kept-copy", adw::ResponseAppearance::Destructive);
            dialog.connect_response(
                Some("kept-copy"),
                glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |_, _| {
                        let library = window.library_service();
                        let copy = copy.clone();
                        glib::spawn_future_local(async move {
                            match library.start_from_kept_copy(copy).await {
                                Ok(_) => {
                                    window.add_toast(adw::Toast::new(&gettext(
                                        "Started from the copy kept before the upgrade",
                                    )));
                                    WidgetExt::activate_action(&window, "app.import", None)
                                        .expect("the application has app.import");
                                }
                                Err(error) => {
                                    glib::g_warning!(
                                        "pindu",
                                        "Could not start from the kept copy: {error}"
                                    );
                                    window.add_toast(adw::Toast::new(&gettext(
                                        "Could not start from the kept copy",
                                    )));
                                }
                            }
                        });
                    }
                ),
            );
        }
        dialog.present(Some(self));
    }
}

/// The cause, that nothing was changed, and what the user can do.
fn body(kept_copy: bool) -> String {
    let cause = gettext(
        "This library was saved by a newer version of Pindu launcher, which this version cannot open. Nothing in it was changed.",
    );
    let next = if kept_copy {
        gettext(
            "Update Pindu launcher again to open it, or start from the copy kept before the last upgrade, without what changed since. The newer library is then kept aside as it is.",
        )
    } else {
        gettext("Update Pindu launcher again to open it.")
    };
    format!("{cause}\n\n{next}")
}
