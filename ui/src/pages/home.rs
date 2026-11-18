//! Startseite wie bei YouTube: Vorschläge aus dem eigenen Verlauf (Sehdauer, Kanäle, Likes,
//! Suchen) – berechnet auf dem eigenen Server, gespeichert nur in der lokalen Datenbank.
//! Keine Shorts. Im Stream-Modus erscheint stattdessen die Stream-Liste.

use crate::Route;
use crate::api;
use crate::components::*;
use crate::pages::feed::Feed;
use crate::pages::stream::StreamPage;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use dioxus::prelude::*;
use nrztube_types::*;

const CHIPS: &[(&str, &str)] = &[("all", "Alle"), ("subs", "Von deinen Abos"), ("known", "Kanäle, die du magst"), ("new", "Neu entdecken")];

#[component]
pub fn Home() -> Element {
    let s = SETTINGS();
    let nav = navigator();
    let start = s.start_page.clone();
    use_effect(use_reactive!(|start| {
        match start.as_str() {
            "feed" => { nav.replace(Route::Feed { profile: String::new() }); }
            "trending" => { nav.replace(Route::Trending { cat: String::new() }); }
            "subscriptions" => { nav.replace(Route::Subscriptions {}); }
            "history" => { nav.replace(Route::HistoryPage {}); }
            "playlists" => { nav.replace(Route::Playlists {}); }
            _ => {}
        }
    }));
    if s.stream_mode {
        return rsx! { StreamPage {} };
    }
    if !s.reco_enabled {
        return rsx! { Feed { profile: String::new() } };
    }
    rsx! { Recommendations {} }
}

#[component]
fn Recommendations() -> Element {
    let mut refresh = use_signal(|| false);
    let mut chip = use_signal(|| "all".to_string());
    let mut shown = use_signal(|| 36usize);
    let mut data = use_resource(move || async move {
        let r = refresh();
        api::get::<HomeFeed>(&format!("/api/home?refresh={r}")).await
    });
    let feed = data.read().clone();
    if let Some(Ok(f)) = &feed {
        let ids: Vec<String> = f.items.iter().map(|i| i.video.id.clone()).chain(f.resume.iter().map(|h| h.video.id.clone())).collect();
        load_dearrow(ids);
    }
    let disliked: Vec<String> = RATINGS().iter().filter(|(_, r)| **r < 0).map(|(k, _)| k.clone()).collect();
    let hide_q = hide_queued();
    let queued = STREAM_IDS();
    let hidden = SETTINGS().hidden_channels;

    rsx! {
        div { class: "mx-auto max-w-[1800px] px-3 pb-6 pt-3 md:px-6 md:pt-4",
            // Chips wie bei YouTube, auf dem Handy waagrecht scrollbar
            div { class: "shelf sticky top-0 z-20 -mx-3 mb-4 flex items-center gap-2 overflow-x-auto bg-background/90 px-3 py-2 backdrop-blur md:-mx-6 md:px-6",
                for (k, l) in CHIPS.iter() {
                    button { key: "{k}",
                        class: if chip() == *k { "shrink-0 rounded-lg bg-foreground px-3 py-1.5 text-sm font-medium text-background" } else { "shrink-0 rounded-lg bg-secondary px-3 py-1.5 text-sm hover:bg-accent" },
                        onclick: move |_| { chip.set(k.to_string()); shown.set(36); },
                        "{l}"
                    }
                }
                div { class: "ml-auto shrink-0",
                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::Sm, title: "Neue Vorschläge",
                        onclick: move |_| { refresh.set(true); shown.set(36); data.restart(); },
                        icons::RefreshCw { class: if data.read().is_none() { "animate-spin" } else { "" } }
                        span { class: "hidden sm:inline", "Neu mischen" }
                    }
                }
            }
            match feed {
                None => rsx! { SkeletonGrid {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message, retry: move |_| data.restart() } },
                Some(Ok(f)) => {
                    let items: Vec<RecoItem> = f.items.iter()
                        .filter(|i| chip() == "all" || i.kind == chip())
                        .filter(|i| !disliked.contains(&i.video.id) && !hidden.contains(&i.video.author_id))
                        // Schon in der Stream-Liste: nicht mehr vorschlagen
                        .filter(|i| !(hide_q && queued.contains(&i.video.id)))
                        .cloned()
                        .collect();
                    let total = items.len();
                    rsx! {
                        if f.cold && chip() == "all" {
                            div { class: "mb-5 flex items-start gap-3 rounded-xl border bg-card p-4 text-sm",
                                icons::Sparkles { class: "mt-0.5 size-5 shrink-0" }
                                div {
                                    div { class: "font-medium", "Deine Startseite lernt mit" }
                                    div { class: "mt-0.5 text-muted-foreground", "Schau ein paar Videos, gib Daumen hoch oder runter – nrzTube merkt sich, welche Kanäle und Themen du magst und wie lange du schaust. Alles bleibt in deiner eigenen Datenbank." }
                                }
                            }
                        }
                        if !f.resume.is_empty() && chip() == "all" {
                            section { class: "mb-7",
                                h2 { class: "mb-3 flex items-center gap-2 text-base font-semibold", icons::History { class: "size-4" } "Weiterschauen" }
                                div { class: "shelf -mx-3 flex snap-x gap-3 overflow-x-auto px-3 md:-mx-0 md:px-0",
                                    for h in f.resume.iter().cloned() {
                                        div { key: "{h.video.id}", class: "w-64 shrink-0 snap-start sm:w-72",
                                            VideoCard { v: h.video.clone(), progress: Some(h.progress) }
                                        }
                                    }
                                }
                            }
                        }
                        if items.is_empty() {
                            EmptyState { title: "Hier ist noch nichts", text: "In dieser Auswahl gibt es gerade keine Vorschläge.".to_string() }
                        } else {
                            div { class: "edge-grid grid grid-cols-1 gap-x-4 gap-y-6 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4 md:gap-y-8",
                                for i in items.into_iter().take(shown()) {
                                    VideoCard { key: "{i.video.id}", v: i.video.clone(), reason: Some(i.reason.clone()) }
                                }
                            }
                            if total > shown() {
                                div { class: "mt-8 flex justify-center",
                                    Button { variant: ButtonVariant::Secondary, onclick: move |_| shown.set(shown() + 36), "Mehr anzeigen" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Platzhalter beim Laden – fühlt sich schneller an als ein Drehkreis
#[component]
pub fn SkeletonGrid() -> Element {
    rsx! {
        div { class: "grid grid-cols-1 gap-x-4 gap-y-6 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4 md:gap-y-8",
            for i in 0..12 {
                div { key: "{i}", class: "flex flex-col gap-2",
                    div { class: "aspect-video w-full animate-pulse rounded-xl bg-muted" }
                    div { class: "flex gap-2.5",
                        div { class: "size-8 shrink-0 animate-pulse rounded-full bg-muted" }
                        div { class: "flex-1 space-y-2",
                            div { class: "h-3.5 w-11/12 animate-pulse rounded bg-muted" }
                            div { class: "h-3 w-2/3 animate-pulse rounded bg-muted" }
                        }
                    }
                }
            }
        }
    }
}
