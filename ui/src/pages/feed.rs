//! Abo-Feed (optional gefiltert nach Profil).

use crate::Route;
use crate::api;
use crate::components::*;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use dioxus::prelude::*;
use nrztube_types::*;

#[component]
pub fn Feed(profile: String) -> Element {
    let mut force = use_signal(|| false);
    let mut limit = use_signal(|| 60usize);
    let p = profile.clone();
    let feed = use_resource(use_reactive!(|p| async move {
        let f = *force.peek();
        let url = format!("/api/feed?profile={}&force={}", p.parse::<i64>().unwrap_or(0), f);
        api::get::<Vec<VideoItem>>(&url).await
    }));
    let pid: i64 = profile.parse().unwrap_or(0);
    let title = PROFILES().into_iter().find(|x| x.id == pid).map(|x| x.name).unwrap_or_else(|| "Abos-Feed".into());
    let has_subs = !SUBS().is_empty();
    rsx! {
        div { class: "mx-auto max-w-[1800px] px-3 py-4 md:p-6",
            PageTitle { title,
                div { class: "flex flex-wrap gap-1",
                    Link { to: Route::Feed { profile: String::new() },
                        class: if pid == 0 { "rounded-full bg-primary px-3 py-1 text-xs font-medium text-primary-foreground" } else { "rounded-full bg-secondary px-3 py-1 text-xs hover:bg-accent" },
                        "Alle"
                    }
                    for pr in PROFILES() {
                        Link { key: "{pr.id}", to: Route::Feed { profile: pr.id.to_string() },
                            class: if pid == pr.id { "rounded-full bg-primary px-3 py-1 text-xs font-medium text-primary-foreground" } else { "rounded-full bg-secondary px-3 py-1 text-xs hover:bg-accent" },
                            "{pr.name}"
                        }
                    }
                }
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, title: "Feeds neu laden",
                    onclick: move |_| { force.set(true); let mut f = feed; f.restart(); },
                    icons::RefreshCw {} "Aktualisieren"
                }
            }
            if !has_subs {
                EmptyState { title: "Noch keine Abos", text: "Abonniere Kanäle über die Suche oder importiere deine Abos (FreeTube, NewPipe, YouTube-Takeout, OPML).".to_string(),
                    div { class: "mt-3 flex gap-2",
                        Button { href: "/subscriptions", size: ButtonSize::Sm, icons::Upload {} "Abos importieren" }
                        Button { href: "/trending", size: ButtonSize::Sm, variant: ButtonVariant::Outline, icons::TrendingUp {} "Trends ansehen" }
                    }
                }
            } else {
                match feed() {
                    None => rsx! { Loading { label: "Feeds werden geladen …" } },
                    Some(Err(e)) => rsx! { ErrorBox { message: e.message } },
                    Some(Ok(list)) if list.is_empty() => rsx! { EmptyState { title: "Keine Videos", text: "In diesem Profil gibt es noch keine Videos.".to_string() } },
                    Some(Ok(list)) => {
                        // Schon in der Stream-Liste: nicht mehr anzeigen
                        let queued = STREAM_IDS();
                        let list: Vec<VideoItem> = if hide_queued() { list.into_iter().filter(|v| !queued.contains(&v.id)).collect() } else { list };
                        let total = list.len();
                        let shown: Vec<VideoItem> = list.into_iter().take(limit()).collect();
                        rsx! {
                            LiveRow {}
                            VideoGrid { videos: shown }
                            if total > limit() {
                                LoadMore { loading: false, onclick: move |_| limit.set(limit() + 60) }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn NotFound(rest: Vec<String>) -> Element {
    rsx! {
        EmptyState { title: "Seite nicht gefunden", text: format!("/{}", rest.join("/")),
            Button { href: "/", size: ButtonSize::Sm, "Zur Startseite" }
        }
    }
}
