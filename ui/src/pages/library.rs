//! Lokale Playlists (Favoriten, Später ansehen, eigene) – liegen auf dem nrzTube-Server.

use crate::Route;
use crate::api;
use crate::components::*;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use dioxus::prelude::*;
use nrztube_types::*;

#[component]
pub fn Playlists() -> Element {
    let mut creating = use_signal(|| false);
    let mut title = use_signal(String::new);
    let mut filter = use_signal(String::new);
    use_hook(|| { spawn(async { reload_playlists().await; }); });
    let create = move |_| {
        let t = title().trim().to_string();
        if t.is_empty() { return; }
        spawn(async move {
            match api::post::<_, serde_json::Value>("/api/playlists", &serde_json::json!({ "title": t })).await {
                Ok(_) => { title.set(String::new()); creating.set(false); reload_playlists().await; }
                Err(e) => toast_err(e),
            }
        });
    };
    let f = filter().to_lowercase();
    let lists: Vec<LocalPlaylist> = PLAYLISTS().into_iter().filter(|p| f.is_empty() || p.title.to_lowercase().contains(&f)).collect();
    rsx! {
        div { class: "mx-auto max-w-[1800px] px-3 py-4 md:p-6",
            PageTitle { title: "Playlists",
                input { class: "h-8 w-44 rounded-md border border-input bg-input/30 px-3 text-sm outline-none", placeholder: "Suchen …", value: "{filter}", oninput: move |e| filter.set(e.value()) }
                Button { size: ButtonSize::Sm, onclick: move |_| creating.set(true), icons::Plus {} "Neue Playlist" }
            }
            if creating() {
                Modal { title: "Neue Playlist", onclose: move |_| creating.set(false),
                    input { class: "h-10 w-full rounded-md border border-input bg-input/30 px-3 text-sm outline-none", placeholder: "Titel", value: "{title}", autofocus: true,
                        oninput: move |e| title.set(e.value()) }
                    div { class: "mt-4 flex justify-end", Button { onclick: create, "Anlegen" } }
                }
            }
            div { class: "grid grid-cols-1 gap-x-4 gap-y-6 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4",
                for p in lists {
                    Link { key: "{p.id}", to: Route::LocalPlaylistPage { id: p.id }, class: "group flex flex-col gap-2",
                        div { class: "relative aspect-video overflow-hidden rounded-xl bg-muted",
                            if !p.thumb.is_empty() { img { class: "absolute inset-0 size-full object-cover", src: "{p.thumb}", alt: "" } }
                            div { class: "absolute inset-y-0 right-0 flex w-2/5 flex-col items-center justify-center gap-1 bg-black/70 text-white",
                                if p.title == "Favoriten" && p.protected { icons::Heart { class: "size-5" } }
                                else if p.protected { icons::Clock { class: "size-5" } }
                                else { icons::ListVideo { class: "size-5" } }
                                span { class: "text-xs font-medium", "{p.count} Videos" }
                            }
                        }
                        div { class: "text-[15px] font-medium", "{p.title}" }
                        if !p.description.is_empty() { div { class: "line-clamp-1 text-xs text-muted-foreground", "{p.description}" } }
                    }
                }
            }
        }
    }
}

#[component]
pub fn LocalPlaylistPage(id: i64) -> Element {
    let mut data = use_resource(use_reactive!(|id| async move { api::get::<LocalPlaylist>(&format!("/api/playlists/{id}")).await }));
    let mut editing = use_signal(|| false);
    let mut title = use_signal(String::new);
    let mut desc = use_signal(String::new);
    let mut filter = use_signal(String::new);
    let nav = navigator();
    let p = match data() {
        None => return rsx! { Loading {} },
        Some(Err(e)) => return rsx! { ErrorBox { message: e.message } },
        Some(Ok(p)) => p,
    };
    let first = p.videos.first().map(|v| v.id.clone());
    let protected = p.protected;
    let (pt, pd) = (p.title.clone(), p.description.clone());
    let f = filter().to_lowercase();
    let videos: Vec<VideoItem> = p.videos.iter().filter(|v| f.is_empty() || v.title.to_lowercase().contains(&f) || v.author.to_lowercase().contains(&f)).cloned().collect();
    let all: Vec<VideoItem> = p.videos.clone();
    rsx! {
        div { class: "mx-auto max-w-5xl p-4 md:p-6",
            div { class: "mb-6 rounded-2xl bg-card p-5",
                h1 { class: "text-2xl font-bold", "{p.title}" }
                if !p.description.is_empty() { p { class: "mt-1 text-sm text-muted-foreground", "{p.description}" } }
                div { class: "mt-1 text-xs text-muted-foreground", "{p.count} Videos • lokal gespeichert" }
                div { class: "mt-4 flex flex-wrap gap-2",
                    if let Some(f) = first.clone() {
                        Button { size: ButtonSize::Sm, onclick: move |_| { nav.push(Route::Watch { v: f.clone(), list: format!("local:{id}"), t: String::new() }); }, icons::Play {} "Abspielen" }
                    }
                    if !all.is_empty() {
                        Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, onclick: move |_| {
                                let i = (js_sys::Math::random() * all.len() as f64) as usize;
                                if let Some(v) = all.get(i) { nav.push(Route::Watch { v: v.id.clone(), list: format!("local:{id}"), t: String::new() }); }
                            }, icons::Sparkles {} "Zufällig" }
                    }
                    if !protected {
                        Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, onclick: move |_| { title.set(pt.clone()); desc.set(pd.clone()); editing.set(true); }, icons::Pencil {} "Bearbeiten" }
                        Button { variant: ButtonVariant::Ghost, size: ButtonSize::Sm, onclick: move |_| {
                                spawn(async move {
                                    if api::delete::<serde_json::Value>(&format!("/api/playlists/{id}")).await.is_ok() {
                                        reload_playlists().await;
                                        toast("Playlist gelöscht");
                                        navigator().push(Route::Playlists {});
                                    }
                                });
                            }, icons::Trash2 {} "Löschen" }
                    }
                    input { class: "ml-auto h-8 w-44 rounded-md border border-input bg-input/30 px-3 text-sm outline-none", placeholder: "In Playlist suchen", value: "{filter}", oninput: move |e| filter.set(e.value()) }
                }
            }
            if editing() {
                Modal { title: "Playlist bearbeiten", onclose: move |_| editing.set(false),
                    input { class: "mb-3 h-10 w-full rounded-md border border-input bg-input/30 px-3 text-sm outline-none", value: "{title}", oninput: move |e| title.set(e.value()) }
                    textarea { class: "h-24 w-full rounded-md border border-input bg-input/30 p-3 text-sm outline-none", placeholder: "Beschreibung", value: "{desc}", oninput: move |e| desc.set(e.value()) }
                    div { class: "mt-4 flex justify-end",
                        Button { onclick: move |_| {
                            spawn(async move {
                                let _ = api::put::<_, serde_json::Value>(&format!("/api/playlists/{id}"), &serde_json::json!({ "title": title(), "description": desc() })).await;
                                editing.set(false);
                                data.restart();
                                reload_playlists().await;
                            });
                        }, "Speichern" }
                    }
                }
            }
            if videos.is_empty() {
                EmptyState { title: "Leer", text: "Füge Videos über „Speichern“ oder das Listen-Symbol auf einer Karte hinzu.".to_string() }
            }
            div { class: "flex flex-col gap-3",
                for (i, v) in videos.into_iter().enumerate() {
                    {
                        let vid = v.id.clone();
                        let vid2 = v.id.clone();
                        let vid3 = v.id.clone();
                        rsx! {
                            div { key: "{v.id}", class: "flex items-start gap-2",
                                span { class: "w-6 shrink-0 pt-8 text-center text-xs text-muted-foreground", "{i + 1}" }
                                div { class: "min-w-0 flex-1",
                                    VideoCard { v: v.clone(), list: Some(format!("local:{id}")), compact: true }
                                }
                                div { class: "flex shrink-0 flex-col",
                                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconXs, title: "Nach oben", onclick: move |_| { let v = vid.clone(); spawn(async move { let _ = api::post::<_, serde_json::Value>(&format!("/api/playlists/{id}/move"), &serde_json::json!({ "video": v, "delta": -1 })).await; data.restart(); }); }, icons::ArrowUp { class: "size-3.5" } }
                                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconXs, title: "Nach unten", onclick: move |_| { let v = vid2.clone(); spawn(async move { let _ = api::post::<_, serde_json::Value>(&format!("/api/playlists/{id}/move"), &serde_json::json!({ "video": v, "delta": 1 })).await; data.restart(); }); }, icons::ArrowDown { class: "size-3.5" } }
                                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconXs, title: "Entfernen", onclick: move |_| { let v = vid3.clone(); spawn(async move { let _ = api::delete::<serde_json::Value>(&format!("/api/playlists/{id}/videos/{v}")).await; data.restart(); reload_playlists().await; }); }, icons::Trash2 { class: "size-3.5" } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
