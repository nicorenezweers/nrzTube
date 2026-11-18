//! Abos verwalten, in Profile ordnen, exportieren und importieren.

use crate::Route;
use crate::api;
use crate::components::*;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use dioxus::prelude::*;
use nrztube_types::*;

const COLORS: &[&str] = &["#a3a3a3", "#ef4444", "#f97316", "#eab308", "#22c55e", "#14b8a6", "#3b82f6", "#8b5cf6", "#ec4899"];

#[component]
pub fn Subscriptions() -> Element {
    let mut filter = use_signal(String::new);
    let mut editing = use_signal(|| None::<Profile>);
    use_hook(|| { spawn(async { reload_subs().await; reload_profiles().await; }); });
    let f = filter().to_lowercase();
    let subs: Vec<Subscription> = SUBS().into_iter().filter(|s| f.is_empty() || s.name.to_lowercase().contains(&f)).collect();
    let missing_thumbs = SUBS().iter().any(|s| s.thumb.is_empty());
    rsx! {
        div { class: "mx-auto max-w-6xl p-4 md:p-6",
            PageTitle { title: "Abos & Profile" }

            // Profile
            section { class: "mb-8",
                div { class: "mb-3 flex items-center justify-between",
                    div {
                        h2 { class: "text-base font-semibold", "Profile" }
                        p { class: "text-xs text-muted-foreground", "Ordne Abos in Profile, um einen fokussierten Feed zu bekommen (z. B. „Tech“, „Musik“, „Kochen“)." }
                    }
                    Button { size: ButtonSize::Sm, onclick: move |_| editing.set(Some(Profile { id: 0, name: String::new(), color: COLORS[6].into(), channels: vec![] })), icons::Plus {} "Neues Profil" }
                }
                div { class: "grid gap-3 sm:grid-cols-2 lg:grid-cols-3",
                    for p in PROFILES() {
                        div { key: "{p.id}", class: "flex items-center gap-3 rounded-xl border bg-card p-3",
                            span { class: "size-3 shrink-0 rounded-full", style: "background: {p.color}" }
                            Link { to: Route::Feed { profile: p.id.to_string() }, class: "min-w-0 flex-1",
                                div { class: "truncate font-medium", "{p.name}" }
                                div { class: "text-xs text-muted-foreground", "{p.channels.len()} Kanäle" }
                            }
                            Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Bearbeiten", onclick: { let p = p.clone(); move |_| editing.set(Some(p.clone())) }, icons::Pencil {} }
                        }
                    }
                    if PROFILES().is_empty() {
                        p { class: "text-sm text-muted-foreground", "Noch keine Profile." }
                    }
                }
            }
            if let Some(p) = editing() {
                ProfileEditor { key: "{p.id}", profile: p, onclose: move |_| editing.set(None) }
            }

            // Abos
            section { class: "mb-8",
                div { class: "mb-3 flex flex-wrap items-center justify-between gap-2",
                    h2 { class: "text-base font-semibold", "Abos ({SUBS().len()})" }
                    div { class: "flex gap-2",
                        if missing_thumbs {
                            Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, title: "Fehlende Kanalbilder und Namen laden",
                                onclick: move |_| { spawn(async move { let _ = api::post::<_, serde_json::Value>("/api/subscriptions/refresh", &()).await; toast("Kanalbilder werden im Hintergrund geladen"); }); },
                                icons::RefreshCw {} "Bilder laden"
                            }
                        }
                        input { class: "h-8 w-48 rounded-md border border-input bg-input/30 px-3 text-sm outline-none", placeholder: "Abos durchsuchen", value: "{filter}", oninput: move |e| filter.set(e.value()) }
                    }
                }
                if SUBS().is_empty() {
                    p { class: "text-sm text-muted-foreground", "Noch keine Abos. Suche einen Kanal und tippe auf „Abonnieren“ – oder importiere deine Abos unten." }
                }
                div { class: "grid gap-2 sm:grid-cols-2 lg:grid-cols-3",
                    for s in subs {
                        SubRow { key: "{s.channel_id}", s: s.clone() }
                    }
                }
            }

            DataSection {}
        }
    }
}

#[component]
fn SubRow(s: Subscription) -> Element {
    let (id, name, thumb) = (s.channel_id.clone(), s.name.clone(), s.thumb.clone());
    let in_profiles: Vec<Profile> = PROFILES().into_iter().filter(|p| p.channels.contains(&s.channel_id)).collect();
    rsx! {
        div { class: "flex items-center gap-3 rounded-xl border bg-card p-2.5",
            Link { to: Route::ChannelPage { id: s.channel_id.clone(), tab: String::new() }, class: "flex min-w-0 flex-1 items-center gap-3",
                if s.thumb.is_empty() {
                    span { class: "flex size-9 shrink-0 items-center justify-center rounded-full bg-muted text-sm font-semibold", {s.name.chars().next().map(|c| c.to_string()).unwrap_or_default()} }
                } else {
                    img { class: "size-9 shrink-0 rounded-full object-cover", src: "{s.thumb}", alt: "", loading: "lazy" }
                }
                div { class: "min-w-0",
                    div { class: "truncate text-sm font-medium", if s.name.is_empty() { "{s.channel_id}" } else { "{s.name}" } }
                    div { class: "flex gap-1",
                        for p in in_profiles { span { key: "{p.id}", class: "size-2 rounded-full", style: "background: {p.color}", title: "{p.name}" } }
                    }
                }
            }
            Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Abo beenden",
                onclick: move |_| { let (a, b, c) = (id.clone(), name.clone(), thumb.clone()); spawn(async move { toggle_subscription(a, b, c).await; }); },
                icons::X {}
            }
        }
    }
}

#[component]
fn ProfileEditor(profile: Profile, onclose: EventHandler<()>) -> Element {
    let mut p = use_signal(|| profile.clone());
    let mut filter = use_signal(String::new);
    let save = move |_| {
        let pr = p();
        if pr.name.trim().is_empty() {
            toast_err("Bitte einen Namen eingeben");
            return;
        }
        spawn(async move {
            match api::post::<_, serde_json::Value>("/api/profiles", &pr).await {
                Ok(_) => { reload_profiles().await; toast("Profil gespeichert"); onclose.call(()); }
                Err(e) => toast_err(e),
            }
        });
    };
    let pid = profile.id;
    let f = filter().to_lowercase();
    rsx! {
        Modal { title: if pid == 0 { "Neues Profil" } else { "Profil bearbeiten" }, onclose: move |_| onclose.call(()), wide: true,
            input { class: "mb-3 h-10 w-full rounded-md border border-input bg-input/30 px-3 text-sm outline-none", placeholder: "Name des Profils", value: "{p().name}",
                oninput: move |e| p.write().name = e.value() }
            div { class: "mb-4 flex gap-2",
                for c in COLORS.iter() {
                    button { key: "{c}", class: if p().color == *c { "size-7 rounded-full ring-2 ring-foreground ring-offset-2 ring-offset-popover" } else { "size-7 rounded-full" },
                        style: "background: {c}", onclick: move |_| p.write().color = c.to_string() }
                }
            }
            div { class: "mb-2 flex items-center justify-between gap-2",
                span { class: "text-sm font-medium", "Kanäle ({p().channels.len()})" }
                div { class: "flex gap-2",
                    button { class: "text-xs text-muted-foreground hover:text-foreground", onclick: move |_| p.write().channels = SUBS().iter().map(|s| s.channel_id.clone()).collect(), "Alle" }
                    button { class: "text-xs text-muted-foreground hover:text-foreground", onclick: move |_| p.write().channels.clear(), "Keine" }
                    input { class: "h-8 w-40 rounded-md border border-input bg-input/30 px-2 text-xs outline-none", placeholder: "Filtern", value: "{filter}", oninput: move |e| filter.set(e.value()) }
                }
            }
            div { class: "grid max-h-80 gap-1 overflow-y-auto sm:grid-cols-2",
                for s in SUBS().into_iter().filter(|s| f.is_empty() || s.name.to_lowercase().contains(&f)) {
                    {
                        let on = p().channels.contains(&s.channel_id);
                        let cid = s.channel_id.clone();
                        rsx! {
                            button { key: "{s.channel_id}", class: "flex items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm hover:bg-accent",
                                onclick: move |_| { let mut w = p.write(); if on { w.channels.retain(|c| c != &cid); } else { w.channels.push(cid.clone()); } },
                                span { class: if on { "flex size-5 items-center justify-center rounded border bg-primary text-primary-foreground" } else { "flex size-5 items-center justify-center rounded border" },
                                    if on { icons::Check { class: "size-3.5" } }
                                }
                                span { class: "truncate", "{s.name}" }
                            }
                        }
                    }
                }
            }
            div { class: "mt-5 flex justify-between gap-2",
                if pid > 0 {
                    Button { variant: ButtonVariant::Ghost, onclick: move |_| {
                        spawn(async move { let _ = api::delete::<serde_json::Value>(&format!("/api/profiles/{pid}")).await; reload_profiles().await; toast("Profil gelöscht"); onclose.call(()); });
                    }, icons::Trash2 {} "Löschen" }
                } else { span {} }
                Button { onclick: save, icons::Check {} "Speichern" }
            }
        }
    }
}

/// Import-Knopf: Datei wählen, Inhalt an den Server schicken
#[component]
fn ImportButton(label: String, endpoint: String, accept: String) -> Element {
    let mut busy = use_signal(|| false);
    rsx! {
        label { class: "inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md border border-input bg-input/30 px-3 text-sm hover:bg-accent",
            if busy() { icons::LoaderCircle { class: "size-4 animate-spin" } } else { icons::Upload { class: "size-4" } }
            "{label}"
            input { r#type: "file", class: "hidden", accept: "{accept}",
                onchange: move |e| {
                    let files = e.files();
                    let endpoint = endpoint.clone();
                    let Some(file) = files.into_iter().next() else { return };
                    busy.set(true);
                    spawn(async move {
                        match file.read_string().await {
                            Ok(text) => match api::post_text::<ImportResult>(&endpoint, text).await {
                                Ok(r) => toast(format!("{} importiert, {} übersprungen{}", r.added, r.skipped, if r.format.is_empty() { String::new() } else { format!(" ({})", r.format) })),
                                Err(e) => toast_err(e),
                            },
                            Err(e) => toast_err(e),
                        }
                        reload_subs().await;
                        reload_profiles().await;
                        reload_playlists().await;
                        busy.set(false);
                    });
                },
            }
        }
    }
}

#[component]
pub fn DataSection() -> Element {
    let link = "inline-flex h-8 items-center gap-1.5 rounded-md border border-input bg-input/30 px-3 text-sm hover:bg-accent";
    rsx! {
        section { class: "rounded-2xl border bg-card p-5",
            h2 { class: "text-base font-semibold", "Exportieren & Importieren" }
            p { class: "mb-4 text-xs text-muted-foreground", "Kompatibel mit FreeTube, NewPipe, Invidious, YouTube (Google Takeout CSV) und RSS-Readern (OPML). Beim Import werden FreeTube-Profile mit übernommen." }
            div { class: "flex flex-col gap-4",
                div {
                    div { class: "mb-2 text-sm font-medium", "Abos" }
                    div { class: "flex flex-wrap gap-2",
                        ImportButton { label: "Importieren", endpoint: "/api/import/subscriptions", accept: ".db,.json,.csv,.opml,.xml,.txt" }
                        a { class: link, href: "/api/export/subscriptions?format=freetube", download: "", icons::Download { class: "size-4" } "FreeTube (.db)" }
                        a { class: link, href: "/api/export/subscriptions?format=newpipe", download: "", icons::Download { class: "size-4" } "NewPipe (.json)" }
                        a { class: link, href: "/api/export/subscriptions?format=csv", download: "", icons::Download { class: "size-4" } "YouTube (.csv)" }
                        a { class: link, href: "/api/export/subscriptions?format=opml", download: "", icons::Download { class: "size-4" } "OPML" }
                    }
                }
                div {
                    div { class: "mb-2 text-sm font-medium", "Playlists" }
                    div { class: "flex flex-wrap gap-2",
                        ImportButton { label: "Importieren", endpoint: "/api/import/playlists", accept: ".db,.json,.txt" }
                        a { class: link, href: "/api/export/playlists", download: "", icons::Download { class: "size-4" } "FreeTube (.db)" }
                    }
                }
                div {
                    div { class: "mb-2 text-sm font-medium", "Verlauf" }
                    div { class: "flex flex-wrap gap-2",
                        ImportButton { label: "Importieren", endpoint: "/api/import/history", accept: ".db,.json,.txt" }
                        a { class: link, href: "/api/export/history", download: "", icons::Download { class: "size-4" } "FreeTube (.db)" }
                    }
                }
            }
        }
    }
}
