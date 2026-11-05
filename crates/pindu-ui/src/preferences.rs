//! The Preferences dialog: General (covers, launching, controller, permissions), Sources
//! (import, folder, sidebar), Artwork (SteamGridDB) and Fullscreen (start-up, sounds).
//! Everything is GSettings, except the SteamGridDB key, which lives in the keyring, and each
//! source's import switch and folder, which live with the library's other source state.

use std::cell::OnceCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::model::{Source, SourceState};
use pindu_core::sources::sync::IMPORTED_SOURCES;

use crate::config::APP_ID;
use crate::file_service;
use crate::permission_service;
use crate::sources::{source_icon, source_name};
use crate::steamgriddb_service;
use crate::sync_service::SyncService;

/// Tile sizes in the order of the cover size row (and the menu drawer's toggles).
pub(crate) const TILE_SIZES: [&str; 3] = ["small", "medium", "large"];

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/preferences.ui")]
    pub struct PreferencesDialog {
        #[template_child]
        pub host_status: TemplateChild<gtk::Label>,
        #[template_child]
        pub command_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub exit_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub controller_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub start_fullscreen_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub sounds_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub tile_size_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub titles_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub badges_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub sources_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub key_row: TemplateChild<adw::PasswordEntryRow>,
        #[template_child]
        pub key_status_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub prefer_row: TemplateChild<adw::SwitchRow>,
        pub settings: OnceCell<gio::Settings>,
        pub sync: OnceCell<SyncService>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferencesDialog {
        const NAME: &'static str = "PinduPreferencesDialog";
        type Type = super::PreferencesDialog;
        type ParentType = adw::PreferencesDialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[gtk::template_callbacks]
    impl PreferencesDialog {
        #[template_callback]
        fn copy_command(&self) {
            let dialog = self.obj();
            dialog
                .clipboard()
                .set_text(&permission_service::override_command());
            dialog.add_toast(adw::Toast::new(&gettext("Command copied")));
        }

        #[template_callback]
        fn save_key(&self) {
            self.obj().save_key();
        }
    }

    impl ObjectImpl for PreferencesDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let settings = gio::Settings::new(APP_ID);
            settings
                .bind("exit-after-launch", &*self.exit_row, "active")
                .build();
            settings
                .bind("controller-navigation", &*self.controller_row, "active")
                .build();
            settings
                .bind("start-in-fullscreen", &*self.start_fullscreen_row, "active")
                .build();
            settings
                .bind("navigation-sounds", &*self.sounds_row, "active")
                .build();
            settings
                .bind("show-titles", &*self.titles_row, "active")
                .build();
            settings
                .bind("show-badges", &*self.badges_row, "active")
                .build();
            settings
                .bind("prefer-steamgriddb", &*self.prefer_row, "active")
                .build();
            let size = settings.string("tile-size");
            let position = TILE_SIZES.iter().position(|s| *s == size.as_str());
            self.tile_size_row
                .set_selected(position.unwrap_or(1) as u32);
            self.tile_size_row.connect_selected_notify(glib::clone!(
                #[strong]
                settings,
                move |row| {
                    if let Some(size) = TILE_SIZES.get(row.selected() as usize) {
                        let _ = settings.set_string("tile-size", size);
                    }
                }
            ));
            self.settings.set(settings).expect("constructed once");

            let allowed = permission_service::host_spawn_allowed();
            self.host_status.set_label(&if allowed {
                gettext("Allowed")
            } else {
                gettext("Not allowed")
            });
            self.command_row.set_visible(!allowed);
            self.command_row
                .set_subtitle(&permission_service::override_command());
            self.obj().show_key_status();
        }
    }
    impl WidgetImpl for PreferencesDialog {}
    impl AdwDialogImpl for PreferencesDialog {}
    impl PreferencesDialogImpl for PreferencesDialog {}
}

glib::wrapper! {
    pub struct PreferencesDialog(ObjectSubclass<imp::PreferencesDialog>)
        @extends adw::PreferencesDialog, adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl PreferencesDialog {
    pub fn new(sync: &SyncService) -> Self {
        let dialog: Self = glib::Object::new();
        dialog.imp().sync.set(sync.clone()).expect("new dialog");
        dialog.show_sources();
        dialog
    }

    fn settings(&self) -> &gio::Settings {
        self.imp().settings.get().expect("constructed")
    }

    /// One expander per source: import on or off, its folder, and its sidebar row.
    fn show_sources(&self) {
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            let sync = dialog.imp().sync.get().expect("set").clone();
            let states = sync.source_states().await;
            for &source in IMPORTED_SOURCES {
                let state = states
                    .iter()
                    .find(|state| state.source == source)
                    .cloned()
                    .unwrap_or_else(|| SourceState::new(source));
                dialog.imp().sources_group.add(&dialog.source_row(&state));
            }
        });
    }

    fn source_row(&self, state: &SourceState) -> adw::ExpanderRow {
        let source = state.source;
        let found = state.resolved_location.as_ref().map_or_else(
            || gettext("Not found on this computer"),
            |path| path.display().to_string(),
        );
        let row = adw::ExpanderRow::builder()
            .title(source_name(source))
            .subtitle(&found)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(source_icon(source)));
        let sync = self.imp().sync.get().expect("set").clone();

        let enabled = adw::SwitchRow::builder()
            .title(gettext("Import"))
            .active(state.enabled)
            .build();
        enabled.connect_active_notify(glib::clone!(
            #[strong]
            sync,
            move |row| sync.set_source_enabled(source, row.is_active())
        ));
        row.add_row(&enabled);

        if source != Source::Desktop {
            let folder = adw::ActionRow::builder()
                .title(gettext("Folder"))
                .subtitle(state.user_override.as_ref().map_or_else(
                    || gettext("Found automatically"),
                    |path| path.display().to_string(),
                ))
                .build();
            let choose = gtk::Button::builder()
                .label(gettext("_Choose…"))
                .use_underline(true)
                .valign(gtk::Align::Center)
                .build();
            let reset = gtk::Button::builder()
                .icon_name("edit-undo-symbolic")
                .tooltip_text(gettext("Find Automatically"))
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .visible(state.user_override.is_some())
                .build();
            folder.add_suffix(&reset);
            folder.add_suffix(&choose);
            choose.connect_clicked(glib::clone!(
                #[weak(rename_to = dialog)]
                self,
                #[weak]
                folder,
                #[weak]
                reset,
                move |_| dialog.choose_source_folder(source, &folder, &reset)
            ));
            reset.connect_clicked(glib::clone!(
                #[strong]
                sync,
                #[weak]
                folder,
                move |button| {
                    sync.set_source_override(source, None);
                    folder.set_subtitle(&gettext("Found automatically"));
                    button.set_visible(false);
                }
            ));
            row.add_row(&folder);
        }

        let in_sidebar = adw::SwitchRow::builder()
            .title(gettext("Show in Sidebar"))
            .active(!self.hidden_sources().iter().any(|s| s == source.as_str()))
            .build();
        in_sidebar.connect_active_notify(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            move |row| dialog.set_in_sidebar(source, row.is_active())
        ));
        row.add_row(&in_sidebar);
        row
    }

    /// Picks a source's folder through the portal; the picked path is what the sandbox
    /// can read.
    fn choose_source_folder(&self, source: Source, folder: &adw::ActionRow, reset: &gtk::Button) {
        let chooser = gtk::FileDialog::builder()
            .title(
                gettext("Choose the Folder of {source}").replace("{source}", &source_name(source)),
            )
            .modal(true)
            .build();
        let root = self.root().and_downcast::<gtk::Window>();
        let (dialog, folder, reset) = (self.downgrade(), folder.downgrade(), reset.downgrade());
        glib::spawn_future_local(async move {
            let Ok(file) = chooser.select_folder_future(root.as_ref()).await else {
                return;
            };
            let (Some(dialog), Some(path)) = (dialog.upgrade(), file.path()) else {
                return;
            };
            let shown = file_service::host_path(&file)
                .await
                .unwrap_or_else(|| path.clone());
            dialog
                .imp()
                .sync
                .get()
                .expect("set")
                .set_source_override(source, Some(path));
            if let (Some(folder), Some(reset)) = (folder.upgrade(), reset.upgrade()) {
                folder.set_subtitle(&shown.display().to_string());
                reset.set_visible(true);
            }
        });
    }

    fn hidden_sources(&self) -> Vec<String> {
        self.settings()
            .strv("hidden-sources")
            .iter()
            .map(|source| source.to_string())
            .collect()
    }

    fn set_in_sidebar(&self, source: Source, shown: bool) {
        let mut hidden = self.hidden_sources();
        hidden.retain(|s| s != source.as_str());
        if !shown {
            hidden.push(source.as_str().to_owned());
        }
        let hidden: Vec<&str> = hidden.iter().map(String::as_str).collect();
        let _ = self
            .settings()
            .set_strv("hidden-sources", hidden.as_slice());
    }

    /// Says whether a SteamGridDB key is stored, without showing it.
    fn show_key_status(&self) {
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let status = match steamgriddb_service::api_key().await {
                Ok(Some(_)) => gettext("A key is stored in your keyring"),
                Ok(None) => gettext("None yet"),
                Err(error) => {
                    gettext("The keyring cannot be used: {error}").replace("{error}", &error)
                }
            };
            if let Some(dialog) = dialog.upgrade() {
                dialog.imp().key_status_row.set_subtitle(&status);
            }
        });
    }

    fn save_key(&self) {
        let key = self.imp().key_row.text().trim().to_owned();
        if key.is_empty() {
            return;
        }
        let dialog = self.downgrade();
        glib::spawn_future_local(async move {
            let saved = steamgriddb_service::set_api_key(&key).await;
            let Some(dialog) = dialog.upgrade() else {
                return;
            };
            match saved {
                Ok(()) => {
                    dialog.imp().key_row.set_text("");
                    dialog.add_toast(adw::Toast::new(&gettext("Key saved")));
                    dialog.show_key_status();
                }
                Err(error) => dialog.add_toast(adw::Toast::new(
                    &gettext("The key was not saved: {error}").replace("{error}", &error),
                )),
            }
        });
    }
}
