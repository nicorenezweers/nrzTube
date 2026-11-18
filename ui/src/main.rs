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
static HLS: Asset = asset!("/assets/hls.min.js");
static PLAYER_JS: Asset = asset!("/assets/player.js");
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
    dioxus::launch(App);
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
