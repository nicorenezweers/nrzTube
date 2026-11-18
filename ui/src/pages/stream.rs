//! Stream-Liste: Videos im privaten Modus per Rechtsklick sammeln, im Stream-Modus der Reihe
//! nach abspielen. Fertig Geschautes fällt (einstellbar) automatisch heraus.

use crate::Route;
use crate::api;
use crate::components::*;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::util;
use dioxus::prelude::*;
use nrztube_types::*;

#[component]
pub fn StreamPage() -> Element {
    let pid = stream_list_id();
    let mut data = use_resource(move || async move {
        // Nach jeder Änderung der Playlists neu laden (Zähler in PLAYLISTS)
        let _ = stream_count();
        let Some(id) = stream_list_id() else { return None };
        api::get::<LocalPlaylist>(&format!("/api/playlists/{id}")).await.ok()
    });
    let s = SETTINGS();
    let nav = navigator();
    let p = data.read().clone().flatten();
    let videos: Vec<VideoItem> = p.as_ref().map(|p| p.videos.clone()).unwrap_or_default();
    let total: u32 = videos.iter().filter_map(|v| v.duration).sum();
    let first = videos.first().map(|v| v.id.clone());
    use_effect(use_reactive!(|first| {
        if let Some(f) = first {
            prefetch(vec![f]);
        }
    }));

    rsx! {
        div { class: "mx-auto max-w-5xl px-3 py-4 md:p-6",
            div { class: "mb-5 rounded-2xl bg-card p-5",
                div { class: "flex items-center gap-3",
                    span { class: "flex size-11 items-center justify-center rounded-xl bg-secondary",
                        icons::Radio { class: "size-5" }
                    }
                    div { class: "min-w-0",
                        h1 { class: "text-xl font-bold md:text-2xl", "Stream-Liste" }
                        div { class: "text-xs text-muted-foreground",
                            "{videos.len()} Videos"
                            if total > 0 { " • {util::duration(total as f64)}" }
                        }
                    }
                }
                p { class: "mt-3 text-sm text-muted-foreground",
                    if s.stream_mode {
                        "Stream-Modus ist an – Verlauf, Abos und Vorschläge sind ausgeblendet."
                    } else {
                        "Sammle Videos mit Rechtsklick (Handy: lange drücken) → „Zur Stream-Liste“. Im Stream-Modus spielst du sie der Reihe nach ab – privat, ohne deinen Verlauf zu zeigen."
                    }
                }
                div { class: "mt-4 flex flex-wrap gap-2",
                    if let (Some(f), Some(id)) = (first.clone(), pid) {
                        Button { size: ButtonSize::Sm, onclick: move |_| { nav.push(Route::Watch { v: f.clone(), list: format!("local:{id}"), t: String::new() }); },
                            icons::Play { class: "fill-current" } "Stream starten"
                        }
                    }
                    Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm,
                        onclick: move |_| { let on = !SETTINGS.peek().stream_mode; update_settings(|x| x.stream_mode = on); toast(if on { "Stream-Modus an" } else { "Stream-Modus aus" }); },
                        if s.stream_mode { icons::RadioTower {} "Stream-Modus beenden" } else { icons::RadioTower {} "Stream-Modus starten" }
                    }
                    if !videos.is_empty() {
                        if let Some(id) = pid {
                            Button { variant: ButtonVariant::Ghost, size: ButtonSize::Sm, onclick: move |_| {
                                    let ids: Vec<String> = data.peek().clone().flatten().map(|p| p.videos.iter().map(|v| v.id.clone()).collect()).unwrap_or_default();
                                    spawn(async move {
                                        for v in ids { let _ = api::delete::<serde_json::Value>(&format!("/api/playlists/{id}/videos/{v}")).await; }
                                        reload_playlists().await;
                                        data.restart();
                                        toast("Stream-Liste geleert");
                                    });
                                },
                                icons::Trash2 {} "Leeren"
                            }
                        }
                    }
                }
            }
            match (pid, p) {
                (None, _) => rsx! { Loading {} },
                (_, None) => rsx! { Loading {} },
                (Some(id), Some(_)) if videos.is_empty() => rsx! {
                    EmptyState { title: "Die Stream-Liste ist leer", text: "Rechtsklick auf ein Video (Handy: lange drücken) und „Zur Stream-Liste“ wählen – oder das Funk-Symbol auf einer Karte.".to_string(),
                        { let _ = id; rsx! {} }
                    }
                },
                (Some(id), Some(_)) => rsx! {
                    div { class: "flex flex-col gap-2",
                        for (i, v) in videos.into_iter().enumerate() {
                            {
                                let (a, b, c, d) = (v.id.clone(), v.id.clone(), v.id.clone(), v.id.clone());
                                rsx! {
                                    div { key: "{v.id}", class: if i == 0 { "flex flex-wrap items-center gap-x-2 gap-y-1 rounded-xl border bg-card/60 p-2" } else { "flex flex-wrap items-center gap-x-2 gap-y-1 rounded-xl p-2 hover:bg-card/40" },
                                        span { class: "w-6 shrink-0 text-center text-xs font-medium text-muted-foreground", if i == 0 { icons::Play { class: "mx-auto size-3.5 fill-current" } } else { "{i + 1}" } }
                                        div { class: "min-w-0 flex-1 basis-[calc(100%-2.5rem)] md:basis-0",
                                            VideoCard { v: v.clone(), list: Some(format!("local:{id}")), compact: true }
                                        }
                                        div { class: "ml-8 flex shrink-0 flex-row md:ml-0",
                                            Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Als Nächstes",
                                                onclick: move |_| { let v = a.clone(); spawn(async move { let _ = api::post::<_, serde_json::Value>(&format!("/api/playlists/{id}/move"), &serde_json::json!({ "video": v, "delta": -100000 })).await; data.restart(); }); },
                                                icons::ArrowUpToLine { class: "size-4" }
                                            }
                                            Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Nach oben",
                                                onclick: move |_| { let v = b.clone(); spawn(async move { let _ = api::post::<_, serde_json::Value>(&format!("/api/playlists/{id}/move"), &serde_json::json!({ "video": v, "delta": -1 })).await; data.restart(); }); },
                                                icons::ArrowUp { class: "size-4" }
                                            }
                                            Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Nach unten",
                                                onclick: move |_| { let v = c.clone(); spawn(async move { let _ = api::post::<_, serde_json::Value>(&format!("/api/playlists/{id}/move"), &serde_json::json!({ "video": v, "delta": 1 })).await; data.restart(); }); },
                                                icons::ArrowDown { class: "size-4" }
                                            }
                                            Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Entfernen",
                                                onclick: move |_| { let v = d.clone(); spawn(async move { let _ = api::delete::<serde_json::Value>(&format!("/api/playlists/{id}/videos/{v}")).await; reload_playlists().await; data.restart(); }); },
                                                icons::X { class: "size-4" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}
