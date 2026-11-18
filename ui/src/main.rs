//! nrzTube – Oberfläche (Dioxus 0.7, WebAssembly) mit Komponenten von nrzUI.
//! Spricht nur mit dem eigenen Server (`/api`, `/img`, `/vi`, `/p`) – nie mit Google.

mod api;
mod components;
mod pages;
mod player;
mod state;
mod ui;
mod util;

use dioxus::prelude::*;
use pages::account::AccountGate;
use pages::channel::ChannelPage;
use pages::feed::{Feed, NotFound};
use pages::home::Home;
use pages::stream::StreamPage;
use pages::history::HistoryPage;
use pages::layout::Layout;
use pages::library::{LocalPlaylistPage, Playlists};
use pages::playlist::YtPlaylist;
use pages::search::Search;
use pages::settings::SettingsPage;
use pages::subscriptions::Subscriptions;
use pages::trending::Trending;
use pages::party::{PartyPage, PartyStart};
use pages::twitch::{TwitchChannelPage, TwitchHome};
use pages::watch::Watch;

static CSS: Asset = asset!("/assets/tailwind.css");
static HLS: Asset = asset!("/assets/vendor/hls.min.js");
static PLAYER_JS: Asset = asset!("/assets/vendor/player.js");
static FAVICON: Asset = asset!("/assets/favicon-32.png");

#[derive(Routable, Clone, PartialEq, Debug)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Layout)]
        #[route("/")]
        Home {},
        #[route("/feed?:profile")]
        Feed { profile: String },
        #[route("/trending?:cat")]
        Trending { cat: String },
        #[route("/stream")]
        StreamPage {},
        #[route("/search?:q&:sort&:date&:kind&:duration")]
        Search { q: String, sort: String, date: String, kind: String, duration: String },
        #[route("/watch?:v&:list&:t")]
        Watch { v: String, list: String, t: String },
        #[route("/channel/:id?:tab")]
        ChannelPage { id: String, tab: String },
        #[route("/twitch?:game")]
        TwitchHome { game: String },
        #[route("/twitch/:login?:tab")]
        TwitchChannelPage { login: String, tab: String },
        #[route("/party")]
        PartyStart {},
        #[route("/playlist?:list")]
        YtPlaylist { list: String },
        #[route("/playlists")]
        Playlists {},
        #[route("/playlists/:id")]
        LocalPlaylistPage { id: i64 },
        #[route("/subscriptions")]
        Subscriptions {},
        #[route("/history")]
        HistoryPage {},
        #[route("/settings?:tab")]
        SettingsPage { tab: String },
    #[end_layout]
    // Watch-Party: eigener Rahmen (auch für Gäste ohne Konto)
    #[route("/party/:id?:k")]
    PartyPage { id: String, k: String },
    #[layout(Layout)]
        #[route("/:..rest")]
        NotFound { rest: Vec<String> },
}

fn main() {
    setup_document();
    dioxus::launch(App);
}

/// Seitenrahmen (früher `index.html`): Sprache, dunkles Theme und Meta-Angaben, bevor die
/// Oberfläche das erste Mal zeichnet – `no-referrer` muss vor dem ersten Bild gesetzt sein.
fn setup_document() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    if let Some(html) = doc.document_element() {
        let _ = html.set_attribute("lang", "de");
        html.set_class_name("dark");
    }
    let Some(head) = doc.head() else { return };
    let meta = [
        ("viewport", "width=device-width, initial-scale=1, viewport-fit=cover"),
        ("color-scheme", "dark"),
        ("theme-color", "#121212"),
        ("referrer", "no-referrer"),
        ("robots", "noindex, nofollow"),
        ("apple-mobile-web-app-capable", "yes"),
        ("apple-mobile-web-app-title", "nrzTube"),
    ];
    for (name, content) in meta {
        if let Ok(Some(old)) = doc.query_selector(&format!("meta[name=\"{name}\"]")) {
            old.remove();
        }
        if let Ok(el) = doc.create_element("meta") {
            let _ = el.set_attribute("name", name);
            let _ = el.set_attribute("content", content);
            let _ = head.append_child(&el);
        }
    }
    if let Ok(style) = doc.create_element("style") {
        style.set_text_content(Some("html,body{background:#121212;color:#fafafa;margin:0}"));
        let _ = head.append_child(&style);
    }
}

#[component]
fn App() -> Element {
    use_hook(|| {
        spawn(async {
            state::load_auth().await;
        });
    });
    rsx! {
        document::Stylesheet { href: CSS }
        document::Link { rel: "icon", href: FAVICON }
        document::Script { src: HLS }
        document::Script { src: PLAYER_JS }
        AccountGate {
            Router::<Route> {}
        }
        components::Toaster {}
    }
}
