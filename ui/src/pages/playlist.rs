//! YouTube-Playlist: Kopf, alle Videos, Nachladen, als lokale Playlist speichern.

use crate::Route;
use crate::api;
use crate::components::*;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use dioxus::prelude::*;
use nrztube_types::*;

#[component]
pub fn YtPlaylist(list: String) -> Element {
    let mut videos = use_signal(Vec::<VideoItem>::new);
    let mut cont = use_signal(|| None::<String>);
    let mut loading = use_signal(|| false);
    let l = list.clone();
    let data = use_resource(use_reactive!(|l| async move {
        let r = api::get::<PlaylistDetails>(&format!("/api/playlist/{}", api::enc(&l))).await;
        if let Ok(p) = &r {
            videos.set(p.videos.clone());
            cont.set(p.continuation.clone());
        }
        r
    }));
    let l2 = list.clone();
    let more = move |_| {
        let Some(c) = cont() else { return };
        let l = l2.clone();
        loading.set(true);
        spawn(async move {
            if let Ok(p) = api::get::<PlaylistDetails>(&format!("/api/playlist/{}?cont={}", api::enc(&l), api::enc(&c))).await {
                videos.write().extend(p.videos);
                cont.set(p.continuation);
            }
            loading.set(false);
        });
    };
    let p = match data() {
        None => return rsx! { Loading {} },
        Some(Err(e)) => return rsx! { ErrorBox { message: e.message } },
        Some(Ok(p)) => p,
    };
    let first = videos().first().map(|v| v.id.clone());
    let title = p.title.clone();
    let desc = p.description.clone();
    let save = move |_| {
        let (title, desc) = (title.clone(), desc.clone());
        let vids = videos();
        spawn(async move {
            match api::post::<_, serde_json::Value>("/api/playlists", &serde_json::json!({ "title": title, "description": desc })).await {
                Ok(r) => {
                    if let Some(id) = r.get("id").and_then(|x| x.as_i64()) {
                        for v in vids {
                            let _ = api::post::<_, serde_json::Value>(&format!("/api/playlists/{id}/videos"), &v).await;
                        }
                        toast(format!("„{title}“ lokal gespeichert"));
                        reload_playlists().await;
                    }
                }
                Err(e) => toast_err(e),
            }
        });
    };
    rsx! {
        div { class: "mx-auto max-w-[1800px] px-3 py-4 md:p-6",
            div { class: "mb-6 flex flex-col gap-4 rounded-2xl bg-card p-5 md:flex-row md:items-end",
                if !p.thumb.is_empty() { img { class: "aspect-video w-full rounded-xl object-cover md:w-72", src: "{p.thumb}", alt: "" } }
                div { class: "min-w-0 flex-1",
                    h1 { class: "text-2xl font-bold", "{p.title}" }
                    if !p.author.is_empty() {
                        Link { to: Route::ChannelPage { id: p.author_id.clone(), tab: String::new() }, class: "mt-1 block text-sm hover:underline", "{p.author}" }
                    }
                    div { class: "mt-1 text-xs text-muted-foreground", {[p.video_count.clone(), p.views.clone()].into_iter().flatten().collect::<Vec<_>>().join(" • ")} }
                    if !p.description.is_empty() { RichText { text: p.description.clone(), class: "mt-2 line-clamp-3 text-sm text-muted-foreground" } }
                    div { class: "mt-4 flex flex-wrap gap-2",
                        if let Some(f) = first {
                            Button { size: ButtonSize::Sm, onclick: { let l = list.clone(); move |_| { navigator().push(watch_route(&f, Some(&l), None)); } }, icons::Play {} "Alle abspielen" }
                        }
                        Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, onclick: save, icons::Download {} "Lokal speichern" }
                    }
                }
            }
            VideoGrid { videos: videos(), list: Some(list.clone()) }
            if cont().is_some() { LoadMore { loading: loading(), onclick: more } }
        }
    }
}
