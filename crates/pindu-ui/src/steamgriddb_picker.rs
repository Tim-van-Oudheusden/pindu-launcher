//! The SteamGridDB picker: images of one layout for a game, to pick one as its art. Asks
//! for the API key first when none is stored.

use std::rc::Rc;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::model::ArtLayout;
use pindu_core::steamgriddb::GameRef;

use crate::file_service::PickedImage;
use crate::steamgriddb_service::{self, SteamGridDb};
use crate::widgets::textures;

/// Where to get an API key.
const KEY_PAGE: &str = "https://www.steamgriddb.com/profile/preferences/api";

/// The game to find images for: by Steam app ID when it has one, else by title.
#[derive(Debug, Clone)]
pub struct PickerGame {
    pub steam_app_id: Option<u32>,
    pub title: String,
}

struct Picker {
    dialog: adw::Dialog,
    stack: gtk::Stack,
    status: adw::StatusPage,
    images: gtk::FlowBox,
    layout: ArtLayout,
    game: PickerGame,
    done: Box<dyn Fn(PickedImage)>,
}

/// Opens the picker over `parent`; `done` gets the chosen image.
pub fn pick(
    parent: &impl IsA<gtk::Widget>,
    layout: ArtLayout,
    game: PickerGame,
    done: impl Fn(PickedImage) + 'static,
) {
    let layout_name = match layout {
        ArtLayout::Cover => gettext("Cover"),
        ArtLayout::Hero => gettext("Banner"),
        ArtLayout::Logo => gettext("Logo"),
        ArtLayout::Icon => gettext("Icon"),
    };
    let stack = gtk::Stack::new();
    stack.add_named(
        &adw::Spinner::builder()
            .width_request(32)
            .height_request(32)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build(),
        Some("loading"),
    );
    let images = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .column_spacing(12)
        .row_spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .valign(gtk::Align::Start)
        .build();
    stack.add_named(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&images)
            .build(),
        Some("images"),
    );
    let status = adw::StatusPage::new();
    stack.add_named(&status, Some("status"));
    let content = adw::ToolbarView::new();
    content.add_top_bar(&adw::HeaderBar::new());
    content.set_content(Some(&stack));
    let dialog = adw::Dialog::builder()
        .title(gettext("SteamGridDB: {layout}").replace("{layout}", &layout_name))
        .content_width(720)
        .content_height(560)
        .child(&content)
        .build();
    let picker = Rc::new(Picker {
        dialog: dialog.clone(),
        stack,
        status,
        images,
        layout,
        game,
        done: Box::new(done),
    });
    dialog.present(Some(parent));
    glib::spawn_future_local(async move {
        match steamgriddb_service::api_key().await {
            Ok(Some(key)) => picker.load(SteamGridDb::new(key)).await,
            Ok(None) => picker.ask_for_key(),
            Err(error) => picker.show_message(&gettext("The Keyring Cannot Be Used"), &error),
        }
    });
}

impl Picker {
    fn show_message(&self, title: &str, description: &str) {
        self.status.set_icon_name(Some("image-missing-symbolic"));
        self.status.set_title(title);
        self.status.set_description(Some(description));
        self.status.set_child(None::<&gtk::Widget>);
        self.stack.set_visible_child_name("status");
    }

    /// Asks for the API key, stores it in the keyring, then loads the images.
    fn ask_for_key(self: &Rc<Self>) {
        let entry = adw::PasswordEntryRow::builder()
            .title(gettext("API Key"))
            .build();
        let list = gtk::ListBox::builder()
            .css_classes(["boxed-list"])
            .selection_mode(gtk::SelectionMode::None)
            .build();
        list.append(&entry);
        let save = gtk::Button::builder()
            .label(gettext("_Save Key"))
            .use_underline(true)
            .halign(gtk::Align::Center)
            .css_classes(["pill", "suggested-action"])
            .build();
        let link = gtk::LinkButton::with_label(KEY_PAGE, &gettext("Get a Key on SteamGridDB"));
        let column = gtk::Box::new(gtk::Orientation::Vertical, 18);
        column.append(&list);
        column.append(&save);
        column.append(&link);
        let clamp = adw::Clamp::builder()
            .maximum_size(400)
            .child(&column)
            .build();
        self.status.set_icon_name(Some("dialog-password-symbolic"));
        self.status
            .set_title(&gettext("SteamGridDB Needs Your API Key"));
        self.status.set_description(Some(&gettext(
            "The key is kept in your keyring, never in plain settings.",
        )));
        self.status.set_child(Some(&clamp));
        self.stack.set_visible_child_name("status");
        let picker = self.clone();
        save.connect_clicked(move |_| {
            let key = entry.text().trim().to_owned();
            if key.is_empty() {
                return;
            }
            let picker = picker.clone();
            glib::spawn_future_local(async move {
                picker.stack.set_visible_child_name("loading");
                match steamgriddb_service::set_api_key(&key).await {
                    Ok(()) => picker.load(SteamGridDb::new(key)).await,
                    Err(error) => picker.show_message(&gettext("The Key Was Not Saved"), &error),
                }
            });
        });
    }

    /// Finds the game, lists its images as thumbnails, and hands over the one picked.
    async fn load(self: &Rc<Self>, client: SteamGridDb) {
        self.stack.set_visible_child_name("loading");
        let game = match self.game.steam_app_id {
            Some(app_id) => Ok(Some(GameRef::Steam(app_id))),
            None => client
                .search(&self.game.title)
                .await
                .map(|hits| hits.first().map(|hit| GameRef::Grid(hit.id))),
        };
        let images = match game {
            Ok(Some(game)) => client.images(self.layout, &game).await,
            Ok(None) => Ok(Vec::new()),
            Err(error) => Err(error),
        };
        let images = match images {
            Ok(images) if images.is_empty() => {
                self.show_message(
                    &gettext("No Images Found"),
                    &gettext("SteamGridDB has no images of this kind for the game"),
                );
                return;
            }
            Ok(images) => images,
            // A rejected key is asked for again.
            Err(error) if error == steamgriddb_service::KEY_REJECTED => {
                self.ask_for_key();
                self.status.set_description(Some(&gettext(
                    "SteamGridDB did not accept the stored key. Enter a valid one; it is kept in your keyring.",
                )));
                return;
            }
            Err(error) => {
                self.show_message(&gettext("SteamGridDB Cannot Be Reached"), &error);
                return;
            }
        };
        self.stack.set_visible_child_name("images");
        let (width, height) = match self.layout {
            ArtLayout::Cover => (150, 225),
            ArtLayout::Hero => (320, 104),
            _ => (160, 160),
        };
        let total = images.len();
        for (index, image) in images.into_iter().enumerate() {
            let picture = gtk::Picture::builder()
                .content_fit(gtk::ContentFit::Contain)
                .width_request(width)
                .height_request(height)
                .can_shrink(true)
                .build();
            let button = gtk::Button::builder()
                .child(&picture)
                .tooltip_text(format!("{}×{}", image.width, image.height))
                .css_classes(["flat"])
                .build();
            let name = gettext("Image {number} of {total}, {width} by {height} pixels")
                .replace("{number}", &(index + 1).to_string())
                .replace("{total}", &total.to_string())
                .replace("{width}", &image.width.to_string())
                .replace("{height}", &image.height.to_string());
            button.update_property(&[gtk::accessible::Property::Label(&name)]);
            self.images.append(&button);
            let picker = self.clone();
            let chosen = client.clone();
            let url = image.url.clone();
            button.connect_clicked(move |_| {
                let (picker, client, url) = (picker.clone(), chosen.clone(), url.clone());
                glib::spawn_future_local(async move {
                    picker.stack.set_visible_child_name("loading");
                    match client.image(&url).await {
                        Ok(image) => {
                            (picker.done)(image);
                            picker.dialog.close();
                        }
                        Err(error) => {
                            picker.show_message(&gettext("The Image Cannot Be Downloaded"), &error)
                        }
                    }
                });
            });
            // Thumbnails load one by one through the shared request slot.
            if let Ok(path) = client.image_file(&image.thumb).await
                && let Ok(texture) = textures::load(&path).await
            {
                picture.set_paintable(Some(&texture));
            }
        }
    }
}
