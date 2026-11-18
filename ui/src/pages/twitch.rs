//! Twitch: Übersicht (Live von gefolgten Kanälen, Kategorien, Top-Streams), Kanal-Seite
//! (Live, VODs, Highlights, Clips, verknüpfte YouTube-Kanäle), Wiedergabe mit Chat und die
//! Twitch-Bausteine für YouTube-Kanal- und Video-Seiten.

use crate::Route;
use crate::api;
use crate::components::*;
use crate::pages::home::SkeletonGrid;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::util;
use dioxus::prelude::*;
use nrztube_types::*;
use serde::Deserialize;

const PURPLE: &str = "text-[#a970ff]";

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Game {
    name: String,
    art: String,
    viewers: u64,
}

// ------------------------------------------------------------------ Übersicht

#[component]
pub fn TwitchHome(game: String) -> Element {
    let mut q = use_signal(String::new);
    let mut submitted = use_signal(String::new);
    let results = use_resource(move || async move {
        let q = submitted();
        if q.trim().is_empty() {
            return None;
        }
        Some(api::get::<Vec<TwitchChannel>>(&format!("/api/twitch/search?q={}", api::enc(&q))).await)
    });
    let g = game.clone();
    let top = use_resource(use_reactive!(|g| async move { api::get::<Vec<VideoItem>>(&format!("/api/twitch/top?game={}", api::enc(&g))).await }));
    let games = use_resource(|| async { api::get::<Vec<Game>>("/api/twitch/games").await });
    use_hook(|| { spawn(async { reload_tw_follows().await; reload_tw_live().await; }); });
    let live = TW_LIVE();
    let follows = TW_FOLLOWS();
    let nav = navigator();

    rsx! {
        div { class: "mx-auto max-w-[1800px] px-3 pb-8 pt-4 md:px-6",
            // Kopf mit Suche
            div { class: "mb-6 flex flex-wrap items-center gap-3",
                div { class: "flex items-center gap-2.5",
                    span { class: "flex size-9 items-center justify-center rounded-xl bg-[#a970ff]/15", TwitchGlyph { class: "size-5 {PURPLE}" } }
                    h1 { class: "text-xl font-semibold tracking-tight", "Twitch" }
                }
                form { class: "relative ml-auto w-full sm:w-80",
                    onsubmit: move |e| { e.prevent_default(); submitted.set(q()); },
                    icons::Search { class: "pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" }
                    input { class: "h-9 w-full rounded-full border border-input bg-input/30 pl-9 pr-3 text-sm outline-none focus:border-ring",
                        r#type: "search", placeholder: "Twitch-Kanal suchen", value: "{q}",
                        oninput: move |e| { q.set(e.value()); if e.value().is_empty() { submitted.set(String::new()); } } }
                }
            }

            if let Some(Some(r)) = results() {
                section { class: "mb-8",
                    h2 { class: "mb-3 text-base font-semibold", "Kanäle" }
                    match r {
                        Ok(list) if list.is_empty() => rsx! { p { class: "text-sm text-muted-foreground", "Nichts gefunden." } },
                        Ok(list) => rsx! {
                            div { class: "grid gap-3 sm:grid-cols-2 xl:grid-cols-3",
                                for c in list { TwitchChannelCard { key: "{c.login}", c: c.clone() } }
                            }
                        },
                        Err(e) => rsx! { ErrorBox { message: e.message } },
                    }
                }
            }

            // Live von gefolgten (und verknüpften) Kanälen
            if !live.is_empty() {
                section { class: "mb-8",
                    div { class: "mb-3 flex items-center gap-2",
                        span { class: "size-2 rounded-full bg-red-500" }
                        h2 { class: "text-base font-semibold", "Live von deinen Kanälen" }
                    }
                    VideoGrid { videos: live.clone() }
                }
            }

            // Gefolgte Kanäle als Leiste
            if !follows.is_empty() {
                section { class: "mb-8",
                    h2 { class: "mb-3 text-base font-semibold", "Gefolgt" }
                    div { class: "shelf -mx-3 flex gap-4 overflow-x-auto px-3 md:mx-0 md:px-0",
                        for f in follows {
                            {
                                let on = is_tw_live(&f.login);
                                rsx! {
                                    Link { key: "{f.login}", to: Route::TwitchChannelPage { login: f.login.clone(), tab: String::new() },
                                        class: "flex w-20 shrink-0 flex-col items-center gap-1.5 text-center",
                                        span { class: "relative",
                                            img { class: if on { "size-16 rounded-full object-cover ring-2 ring-red-500 ring-offset-2 ring-offset-background" } else { "size-16 rounded-full object-cover opacity-80" }, src: "{f.thumb}", alt: "" }
                                            if on { span { class: "absolute -bottom-1 left-1/2 -translate-x-1/2 rounded bg-red-600 px-1 text-[9px] font-bold text-white", "LIVE" } }
                                        }
                                        span { class: "w-full truncate text-xs", "{f.name}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Kategorien
            if let Some(Ok(gs)) = games() {
                section { class: "mb-6",
                    div { class: "shelf -mx-3 flex gap-2 overflow-x-auto px-3 pb-1 md:mx-0 md:px-0",
                        button { class: if game.is_empty() { "shrink-0 rounded-lg bg-foreground px-3 py-1.5 text-sm font-medium text-background" } else { "shrink-0 rounded-lg bg-secondary px-3 py-1.5 text-sm hover:bg-accent" },
                            onclick: move |_| { nav.replace(Route::TwitchHome { game: String::new() }); },
                            "Alle"
                        }
                        for gm in gs {
                            {
                                let active = gm.name == game;
                                let name = gm.name.clone();
                                rsx! {
                                    button { key: "{gm.name}",
                                        class: if active { "flex shrink-0 items-center gap-2 rounded-lg bg-foreground py-1 pl-1 pr-3 text-sm font-medium text-background" } else { "flex shrink-0 items-center gap-2 rounded-lg bg-secondary py-1 pl-1 pr-3 text-sm hover:bg-accent" },
                                        title: "{util::number(gm.viewers)} Zuschauer",
                                        onclick: move |_| { nav.replace(Route::TwitchHome { game: name.clone() }); },
                                        img { class: "h-7 w-5 rounded object-cover", src: "{gm.art}", alt: "" }
                                        "{gm.name}"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            section {
                h2 { class: "mb-3 text-base font-semibold",
                    if game.is_empty() { "Meistgesehen auf Twitch" } else { "Live in {game}" }
                }
                match top() {
                    None => rsx! { SkeletonGrid {} },
                    Some(Err(e)) => rsx! { ErrorBox { message: e.message } },
                    Some(Ok(v)) if v.is_empty() => rsx! { EmptyState { title: "Gerade niemand live".to_string() } },
                    Some(Ok(v)) => rsx! { VideoGrid { videos: v } },
                }
            }
        }
    }
}

#[component]
fn TwitchChannelCard(c: TwitchChannel) -> Element {
    let following = is_tw_following(&c.login);
    let (l, n, t) = (c.login.clone(), c.name.clone(), c.avatar.clone());
    rsx! {
        div { class: "flex items-center gap-3 rounded-xl border bg-card p-3",
            Link { to: Route::TwitchChannelPage { login: c.login.clone(), tab: String::new() }, class: "relative shrink-0",
                img { class: if c.live.is_some() { "size-14 rounded-full object-cover ring-2 ring-red-500" } else { "size-14 rounded-full object-cover" }, src: "{c.avatar}", alt: "" }
                if c.live.is_some() { span { class: "absolute -bottom-1 left-1/2 -translate-x-1/2 rounded bg-red-600 px-1 text-[9px] font-bold text-white", "LIVE" } }
            }
            Link { to: Route::TwitchChannelPage { login: c.login.clone(), tab: String::new() }, class: "min-w-0 flex-1",
                div { class: "truncate font-medium", "{c.name}" }
                div { class: "truncate text-xs text-muted-foreground",
                    if let Some(st) = &c.live { "{st.game} • {util::compact(st.viewers)} Zuschauer" }
                    else if let Some(f) = c.followers { "{util::compact(f)} Follower" }
                }
            }
            Button { variant: if following { ButtonVariant::Secondary } else { ButtonVariant::Outline }, size: ButtonSize::Sm,
                onclick: move |_| toggle_tw_follow(l.clone(), n.clone(), t.clone()),
                if following { "Gefolgt" } else { "Folgen" }
            }
        }
    }
}

// ------------------------------------------------------------------ Kanal

fn tab_label(t: &str) -> &'static str {
    match t {
        "highlight" => "Highlights",
        "clips" => "Clips",
        "upload" => "Uploads",
        "about" => "Info",
        _ => "Videos",
    }
}

#[component]
pub fn TwitchChannelPage(login: String, tab: String) -> Element {
    let l = login.clone();
    let mut channel = use_resource(use_reactive!(|l| async move { api::get::<TwitchChannel>(&format!("/api/twitch/channel/{}", api::enc(&l))).await }));
    let current = if tab.is_empty() { "archive".to_string() } else { tab.clone() };
    let (l2, t2) = (login.clone(), current.clone());
    let videos = use_resource(use_reactive!(|l2, t2| async move {
        if t2 == "about" {
            return Ok(vec![]);
        }
        api::get::<Vec<VideoItem>>(&format!("/api/twitch/channel/{}/videos?kind={}", api::enc(&l2), t2)).await
    }));
    let ch = match channel() {
        None => return rsx! { Loading {} },
        Some(Err(e)) => return rsx! { ErrorBox { message: e.message, retry: move |_| channel.restart() } },
        Some(Ok(c)) => c,
    };
    let following = is_tw_following(&ch.login);
    let (fl, fn_, ft) = (ch.login.clone(), ch.name.clone(), ch.avatar.clone());
    let tabs = ["archive", "highlight", "clips", "upload", "about"];
    let live_id = format!("tw:live:{}", ch.login);
    rsx! {
        div { class: "mx-auto max-w-[1800px]",
            if let Some(b) = &ch.banner {
                div { class: "aspect-[6/1] w-full overflow-hidden bg-muted md:mx-6 md:mt-4 md:w-auto md:rounded-2xl",
                    img { class: "size-full object-cover", src: "{b}", alt: "" }
                }
            }
            div { class: "flex flex-wrap items-center gap-4 px-4 py-5 md:px-6",
                span { class: "relative shrink-0",
                    img { class: if ch.live.is_some() { "size-20 rounded-full bg-muted object-cover ring-[3px] ring-red-500 ring-offset-2 ring-offset-background md:size-28" } else { "size-20 rounded-full bg-muted object-cover md:size-28" }, src: "{ch.avatar}", alt: "" }
                    if ch.live.is_some() { span { class: "absolute -bottom-1 left-1/2 -translate-x-1/2 rounded bg-red-600 px-1.5 text-[11px] font-bold text-white", "LIVE" } }
                }
                div { class: "min-w-0 flex-1",
                    h1 { class: "flex items-center gap-2 text-2xl font-bold", "{ch.name}" TwitchGlyph { class: "size-5 {PURPLE}" } }
                    div { class: "mt-1 text-sm text-muted-foreground",
                        "twitch.tv/{ch.login}"
                        if let Some(f) = ch.followers { " • {util::compact(f)} Follower" }
                    }
                    if !ch.description.is_empty() {
                        p { class: "mt-1 line-clamp-2 max-w-2xl text-sm text-muted-foreground", "{ch.description}" }
                    }
                    if !ch.links.is_empty() {
                        div { class: "mt-3 flex flex-wrap items-center gap-2",
                            span { class: "text-xs text-muted-foreground", "Auch auf YouTube:" }
                            LinkedChannels { links: ch.links.clone(), to_twitch: false,
                                on_unlink: move |lk: ChannelLink| {
                                    spawn(async move {
                                        let _ = api::delete::<serde_json::Value>(&format!("/api/links/{}/{}", lk.yt_id, lk.tw_login)).await;
                                        toast("Verknüpfung gelöst");
                                        channel.restart();
                                    });
                                }
                            }
                        }
                    }
                }
                Button { variant: if following { ButtonVariant::Secondary } else { ButtonVariant::Default },
                    onclick: move |_| toggle_tw_follow(fl.clone(), fn_.clone(), ft.clone()),
                    if following { icons::HeartOff {} "Gefolgt" } else { icons::Heart {} "Folgen" }
                }
            }

            // Gerade live: grosse Karte
            if let Some(st) = &ch.live {
                div { class: "px-4 md:px-6",
                    Link { to: Route::Watch { v: live_id.clone(), list: String::new(), t: String::new() },
                        class: "group mb-6 grid overflow-hidden rounded-2xl border bg-card transition-colors hover:bg-accent/40 md:grid-cols-[minmax(0,480px)_1fr]",
                        div { class: "relative aspect-video bg-muted",
                            img { class: "absolute inset-0 size-full object-cover", src: "{st.thumb}?t={util::now() / 120}", alt: "" }
                            span { class: "absolute left-2 top-2 flex items-center gap-1 rounded bg-red-600 px-1.5 py-0.5 text-xs font-bold text-white",
                                span { class: "size-1.5 animate-pulse rounded-full bg-white" } "LIVE"
                            }
                            span { class: "absolute bottom-2 left-2 rounded bg-black/70 px-1.5 py-0.5 text-xs text-white tabular-nums", "{util::number(st.viewers)} Zuschauer" }
                            span { class: "absolute inset-0 flex items-center justify-center bg-black/0 transition-colors group-hover:bg-black/30",
                                icons::Play { class: "size-12 fill-white text-white opacity-0 transition-opacity group-hover:opacity-100" }
                            }
                        }
                        div { class: "flex flex-col justify-center gap-2 p-4 md:p-6",
                            div { class: "text-xs font-medium uppercase tracking-wider text-red-500", "Jetzt live" }
                            h2 { class: "line-clamp-2 text-lg font-semibold", "{st.title}" }
                            div { class: "text-sm text-muted-foreground",
                                "{st.game}"
                                if let Some(t) = st.started { " • live seit {util::ago(t).trim_start_matches(\"vor \")}" }
                            }
                            div { class: "mt-1",
                                span { class: "inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground",
                                    icons::Play { class: "size-4 fill-current" } "Ansehen"
                                }
                            }
                        }
                    }
                }
            }

            div { class: "sticky top-0 z-10 flex items-center gap-1 overflow-x-auto border-b bg-background/95 px-4 md:px-6 shelf",
                for t in tabs {
                    Link { key: "{t}", to: Route::TwitchChannelPage { login: login.clone(), tab: t.to_string() },
                        class: if t == current { "shrink-0 border-b-2 border-foreground px-3 py-3 text-sm font-medium" } else { "shrink-0 border-b-2 border-transparent px-3 py-3 text-sm text-muted-foreground hover:text-foreground" },
                        {tab_label(t)}
                    }
                }
            }
            div { class: "p-4 md:p-6",
                if current == "about" {
                    div { class: "max-w-2xl space-y-4",
                        if !ch.description.is_empty() { p { class: "whitespace-pre-line text-sm", "{ch.description}" } }
                        if !ch.socials.is_empty() {
                            div { class: "flex flex-col gap-1.5",
                                h3 { class: "text-sm font-semibold", "Links" }
                                for (title, url) in ch.socials.iter() {
                                    a { key: "{url}", href: "{url}", target: "_blank", rel: "noreferrer noopener", class: "flex items-center gap-2 text-sm text-muted-foreground hover:text-foreground",
                                        icons::ExternalLink { class: "size-3.5" }
                                        if title.is_empty() { "{url}" } else { "{title}" }
                                        span { class: "truncate text-xs opacity-60", "{url}" }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    match videos() {
                        None => rsx! { SkeletonGrid {} },
                        Some(Err(e)) => rsx! { ErrorBox { message: e.message } },
                        Some(Ok(v)) if v.is_empty() => rsx! { EmptyState { title: "Hier ist nichts".to_string(), text: "Twitch löscht vergangene Streams je nach Kanal nach 7 bis 60 Tagen.".to_string() } },
                        Some(Ok(v)) => rsx! { VideoGrid { videos: v } },
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Wiedergabe

/// Video-Seite für Twitch (Live, VOD, Clip). Der Player läuft wie immer im Layout.
#[component]
pub fn TwitchWatch(v: String) -> Element {
    let vid = v.clone();
    let mut details = use_resource(use_reactive!(|vid| async move {
        let r = api::get::<VideoDetails>(&format!("/api/twitch/details/{vid}")).await;
        if let Ok(d) = &r {
            *DETAILS.write() = Some(d.clone());
        }
        r
    }));
    let s = SETTINGS();
    let live = v.starts_with("tw:live:");
    let login = match details() {
        Some(Ok(d)) => d.author_id.trim_start_matches("tw:").to_string(),
        _ => v.strip_prefix("tw:live:").unwrap_or("").to_string(),
    };
    let wide = util::window().inner_width().ok().and_then(|x| x.as_f64()).unwrap_or(1400.0) >= 1280.0;
    let dock = live && s.twitch_chat && wide && !login.is_empty();
    use_effect(use_reactive!(|dock| { *CHAT_DOCK.write() = dock; }));
    use_drop(|| { *CHAT_DOCK.write() = false; });
    // Livestatus regelmässig auffrischen (Titel, Zuschauer)
    use_future(move || async move {
        if !live { return; }
        loop {
            gloo_timers::future::TimeoutFuture::new(60_000).await;
            details.restart();
        }
    });
    let wrap = if s.theater { "mx-auto max-w-[1800px] px-4 py-4 md:px-6" } else { "mx-auto max-w-6xl px-4 py-4 md:px-6" };
    rsx! {
        div { class: "{wrap}",
            match details() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message, retry: move |_| details.restart() } },
                Some(Ok(d)) => rsx! {
                    TwitchInfo { d: d.clone() }
                    if live && s.twitch_chat && !dock && !CHAT_HIDDEN() {
                        div { class: "mt-4 h-[60vh] overflow-hidden rounded-xl border bg-card",
                            LiveChat { key: "{login}", kind: "twitch".to_string(), id: login.clone() }
                        }
                    }
                    if !live {
                        MoreFromChannel { login: login.clone(), current: v.clone() }
                    }
                },
            }
        }
        if dock {
            aside { class: if CHAT_HIDDEN() { "hidden" } else { "fixed bottom-0 right-0 top-14 md:top-[52px] z-20 hidden w-[var(--chat-w,360px)] border-l bg-card xl:block" },
                // Trennlinie: ziehen ändert die Breite zwischen Player und Chat
                div { class: "chat-resize absolute inset-y-0 -left-1 z-10 w-2 cursor-col-resize hover:bg-foreground/10", title: "Ziehen, um die Breite zu ändern",
                    onmousedown: move |_| crate::util::eval("window.nrzChatResize && window.nrzChatResize()"),
                }
                LiveChat { key: "{login}", kind: "twitch".to_string(), id: login.clone() }
            }
        }
    }
}

#[component]
fn TwitchInfo(d: VideoDetails) -> Element {
    let login = d.author_id.trim_start_matches("tw:").to_string();
    let following = is_tw_following(&login);
    let (fl, fname, ft) = (login.clone(), d.author.clone(), d.author_thumb.clone().unwrap_or_default());
    let l = login.clone();
    let links = use_resource(use_reactive!(|l| async move { api::get::<TwitchChannel>(&format!("/api/twitch/channel/{l}")).await.map(|c| c.links).unwrap_or_default() }));
    let mut expanded = use_signal(|| false);
    let item = VideoItem {
        id: d.id.clone(),
        title: d.title.clone(),
        author: d.author.clone(),
        author_id: d.author_id.clone(),
        author_thumb: d.author_thumb.clone(),
        thumb: d.thumb.clone(),
        duration: d.duration,
        views: d.views,
        views_text: d.views_text.clone(),
        live: d.live,
        ..Default::default()
    };
    let item_stream = item.clone();
    let item_pl = item.clone();
    let share = format!("https://www.twitch.tv/{}", match d.id.split(':').nth(1) {
        Some("vod") => format!("videos/{}", d.id.rsplit(':').next().unwrap_or("")),
        Some("clip") => format!("{login}/clip/{}", d.id.rsplit(':').next().unwrap_or("")),
        _ => login.clone(),
    });
    rsx! {
        h1 { class: "text-lg font-semibold leading-snug md:text-xl", "{d.title}" }
        div { class: "mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-muted-foreground",
            if d.live {
                span { class: "flex items-center gap-1 rounded bg-red-600 px-1.5 py-0.5 text-[11px] font-bold text-white", span { class: "size-1.5 animate-pulse rounded-full bg-white" } "LIVE" }
            }
            if let Some(v) = &d.views_text { span { class: "tabular-nums", "{v}" } }
            if let Some(r) = &d.relative_date { span { "• {r}" } }
            if let Some(c) = &d.category {
                Link { to: Route::TwitchHome { game: c.clone() }, class: "rounded-md bg-secondary px-2 py-0.5 text-xs text-foreground hover:bg-accent", "{c}" }
            }
        }
        div { class: "mt-3 flex flex-wrap items-center gap-3",
            Link { to: Route::TwitchChannelPage { login: login.clone(), tab: String::new() }, class: "flex min-w-0 items-center gap-3",
                if let Some(a) = &d.author_thumb {
                    img { class: if d.live { "size-10 rounded-full bg-muted object-cover ring-2 ring-red-500" } else { "size-10 rounded-full bg-muted object-cover" }, src: "{a}", alt: "" }
                }
                div { class: "min-w-0",
                    div { class: "flex items-center gap-1.5 truncate font-medium", "{d.author}" TwitchGlyph { class: "size-3.5 {PURPLE}" } }
                    if let Some(sc) = &d.subscribers { div { class: "text-xs text-muted-foreground", "{sc}" } }
                }
            }
            Button { variant: if following { ButtonVariant::Secondary } else { ButtonVariant::Default }, size: ButtonSize::Sm,
                onclick: move |_| toggle_tw_follow(fl.clone(), fname.clone(), ft.clone()),
                if following { icons::HeartOff {} "Gefolgt" } else { icons::Heart {} "Folgen" }
            }
            if d.live {
                Button { variant: ButtonVariant::Secondary, size: ButtonSize::IconSm, class: "ml-auto", title: "Twitch-Adresse kopieren",
                    onclick: { let share = share.clone(); move |_| { util::copy(&share); toast("Adresse kopiert"); } },
                    icons::Share2 {}
                }
            } else {
            div { class: "shelf -mx-4 flex w-[calc(100%+2rem)] items-center gap-2 overflow-x-auto px-4 md:mx-0 md:w-auto md:flex-1 md:justify-end md:overflow-visible md:px-0",
                if SETTINGS().stream_enabled {
                Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, title: "Zur Stream-Liste",
                    onclick: move |_| add_to_stream(item_stream.clone()),
                    icons::Radio {} span { class: "hidden sm:inline", "Stream" }
                }
                }
                Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, title: "Zu Playlist",
                    onclick: move |_| *ADD_TO.write() = Some(item_pl.clone()),
                    icons::ListPlus {} span { class: "hidden sm:inline", "Speichern" }
                }
                Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm, title: "Twitch-Adresse kopieren",
                    onclick: move |_| { util::copy(&share); toast("Adresse kopiert"); },
                    icons::Share2 {} span { class: "hidden sm:inline", "Teilen" }
                }
            }
            }
        }
        if let Some(l) = links().filter(|l| !l.is_empty()) {
            div { class: "mt-4 flex flex-wrap items-center gap-2",
                span { class: "text-xs text-muted-foreground", "Auch auf YouTube:" }
                LinkedChannels { links: l, to_twitch: false }
            }
        }
        if !d.description.is_empty() {
            div { class: "mt-4 cursor-pointer rounded-xl bg-secondary/60 p-3 text-sm hover:bg-secondary",
                onclick: move |_| expanded.set(!expanded()),
                p { class: if expanded() { "whitespace-pre-line" } else { "line-clamp-2 whitespace-pre-line" }, "{d.description}" }
            }
        }
    }
}

#[component]
fn MoreFromChannel(login: String, current: String) -> Element {
    let l = login.clone();
    let vods = use_resource(use_reactive!(|l| async move { api::get::<Vec<VideoItem>>(&format!("/api/twitch/channel/{l}/videos?kind=archive")).await }));
    let Some(Ok(list)) = vods() else { return rsx! {} };
    let list: Vec<VideoItem> = list.into_iter().filter(|v| v.id != current).take(12).collect();
    if list.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { class: "mt-8",
            h2 { class: "mb-3 text-sm font-semibold text-muted-foreground", "Weitere Streams" }
            VideoGrid { videos: list }
        }
    }
}

// ------------------------------------------------------------------ Chat

/// Livechat für Twitch (`kind = "twitch"`, `id` = Login) oder YouTube (`"youtube"`, Video-ID):
/// lesen über Server-Sent Events, schreiben mit dem verbundenen Konto.
#[component]
pub fn LiveChat(kind: String, id: String) -> Element {
    let mut lines = use_signal(Vec::<ChatLine>::new);
    let mut connected = use_signal(|| false);
    let mut paused = use_signal(|| false);
    let mut draft = use_signal(String::new);
    let mut sending = use_signal(|| false);
    let twitch = kind == "twitch";
    let base = if twitch { format!("/api/twitch/chat/{id}") } else { format!("/api/ytchat/{id}") };
    let url = base.clone();
    use_future(move || {
        let url = url.clone();
        async move {
            let js = format!(
                "if (window.__nrzChat) window.__nrzChat.close();\
                 const es = new EventSource({});\
                 window.__nrzChat = es;\
                 es.onopen = () => dioxus.send('__open');\
                 es.onmessage = (e) => dioxus.send(e.data);\
                 await new Promise(() => {{}});",
                util::js_str(&url)
            );
            let mut ev = document::eval(&js);
            while let Ok(msg) = ev.recv::<String>().await {
                if msg == "__open" {
                    connected.set(true);
                    continue;
                }
                if let Ok(line) = serde_json::from_str::<ChatLine>(&msg) {
                    if *paused.peek() {
                        continue;
                    }
                    let mut w = lines.write();
                    w.push(line);
                    let n = w.len();
                    if n > 250 {
                        w.drain(0..n - 200);
                    }
                }
            }
        }
    });
    use_drop(|| util::eval("if (window.__nrzChat) { window.__nrzChat.close(); window.__nrzChat = null; }"));
    let acc = ACCOUNTS();
    let can_write = if twitch { acc.twitch.is_some() } else { acc.youtube };
    let max = if twitch { 500 } else { 200 };
    let send_url = base.clone();
    let mut send = move || {
        let text = draft().trim().to_string();
        if text.is_empty() || sending() {
            return;
        }
        sending.set(true);
        let u = send_url.clone();
        spawn(async move {
            match api::post::<_, serde_json::Value>(&u, &ChatSend { text }).await {
                Ok(_) => draft.set(String::new()),
                Err(e) => toast_err(e),
            }
            sending.set(false);
        });
    };
    let list = lines();
    rsx! {
        div { class: "flex h-full flex-col",
            div { class: "flex h-11 shrink-0 items-center gap-2 border-b px-3",
                if twitch { TwitchGlyph { class: "size-4 {PURPLE}" } } else { icons::Youtube { class: "size-4 text-red-500" } }
                span { class: "text-sm font-medium", "Livechat" }
                span { class: if connected() { "size-1.5 rounded-full bg-green-500" } else { "size-1.5 rounded-full bg-muted-foreground/50" } }
                Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, class: "ml-auto", title: if paused() { "Chat fortsetzen" } else { "Chat anhalten" },
                    onclick: move |_| paused.set(!paused()),
                    if paused() { icons::Play {} } else { icons::Pause {} }
                }
                Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Chat einklappen (im Player wieder öffnen)",
                    onclick: move |_| *CHAT_HIDDEN.write() = true,
                    icons::PanelRightClose {}
                }
            }
            // flex-col-reverse: neue Zeilen unten, Ansicht bleibt von selbst unten
            div { class: "flex min-h-0 flex-1 flex-col-reverse overflow-y-auto px-3 py-2",
                div { class: "flex flex-col gap-1",
                    if list.is_empty() {
                        p { class: "py-6 text-center text-xs text-muted-foreground", if connected() { "Warte auf Nachrichten …" } else { "Verbinde mit dem Chat …" } }
                    }
                    for m in list.iter() {
                        div { key: "{m.id}", class: "break-words text-[13px] leading-snug",
                            span { class: "font-semibold", style: "color: {chat_color(&m.color)}", "{m.user}" }
                            span { class: "text-muted-foreground", ": " }
                            for (i, p) in m.parts.iter().enumerate() {
                                match p {
                                    ChatPart::Text { t } => rsx! { span { key: "{i}", "{t}" } },
                                    ChatPart::Emote { name, url } => rsx! { img { key: "{i}", class: "inline-block h-6 align-middle", src: "{url}", alt: "{name}", title: "{name}", loading: "lazy" } },
                                }
                            }
                        }
                    }
                }
            }
            div { class: "shrink-0 border-t p-2",
                if can_write {
                    form { class: "flex items-center gap-2",
                        onsubmit: move |e| { e.prevent_default(); send(); },
                        input { class: "h-9 min-w-0 flex-1 rounded-lg border border-input bg-input/30 px-3 text-sm outline-none focus:border-ring",
                            placeholder: if twitch { format!("Als {} schreiben …", acc.twitch.clone().unwrap_or_default()) } else { "Nachricht senden …".to_string() },
                            maxlength: "{max}", value: "{draft}", disabled: sending(),
                            oninput: move |e| draft.set(e.value()) }
                        Button { size: ButtonSize::IconSm, disabled: sending() || draft().trim().is_empty(), title: "Senden",
                            if sending() { icons::LoaderCircle { class: "animate-spin" } } else { icons::Send {} }
                        }
                    }
                } else {
                    Link { to: Route::SettingsPage { tab: if twitch { "twitch".into() } else { "account".into() } },
                        class: "flex items-center justify-center gap-1.5 rounded-lg px-2 py-2 text-xs text-muted-foreground hover:bg-accent hover:text-foreground",
                        icons::LogIn { class: "size-3.5" }
                        if twitch { "Mit Twitch anmelden, um mitzuschreiben" } else { "YouTube-Anmeldung hinterlegen, um mitzuschreiben" }
                    }
                }
            }
        }
    }
}

/// Twitch-Farben, zu dunkle auf dunklem Grund aufhellen
fn chat_color(c: &str) -> String {
    let hex = c.trim_start_matches('#');
    if hex.len() != 6 {
        return "#a970ff".into();
    }
    let v = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(128) as f32;
    let (r, g, b) = (v(0), v(2), v(4));
    let lum = 0.299 * r + 0.587 * g + 0.114 * b;
    if lum < 90.0 {
        let k = 90.0 / lum.max(1.0);
        return format!("rgb({}, {}, {})", (r * k + 60.0).min(255.0) as u8, (g * k + 60.0).min(255.0) as u8, (b * k + 60.0).min(255.0) as u8);
    }
    format!("#{hex}")
}

// ------------------------------------------------------------------ Für YouTube-Seiten

/// Twitch-Logins in einem Text (twitch.tv/<login>)
pub fn twitch_logins(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out: Vec<String> = vec![];
    for (i, _) in lower.match_indices("twitch.tv/") {
        let login: String = lower[i + 10..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if login.len() >= 3 && login.len() <= 25 && !["videos", "directory", "p", "subs", "search"].contains(&login.as_str()) && !out.contains(&login) {
            out.push(login);
        }
    }
    out.truncate(4);
    out
}

/// Auf YouTube-Kanalseiten: verknüpfte Twitch-Kanäle, Live-Hinweis und von Hand verknüpfen
#[component]
pub fn YtTwitchLinks(yt_id: String, name: String) -> Element {
    let id = yt_id.clone();
    let mut links = use_resource(use_reactive!(|id| async move { api::get::<Vec<ChannelLink>>(&format!("/api/links/youtube/{id}")).await.unwrap_or_default() }));
    let mut adding = use_signal(|| false);
    let mut input = use_signal(String::new);
    if !SETTINGS().twitch_enabled {
        return rsx! {};
    }
    let list = links().unwrap_or_default();
    let live = list.iter().find(|l| l.live.is_some()).cloned();
    let yt_add = yt_id.clone();
    rsx! {
        if let Some(l) = live {
            if let Some(st) = &l.live {
                Link { to: Route::Watch { v: format!("tw:live:{}", l.tw_login), list: String::new(), t: String::new() },
                    class: "group mx-4 mb-2 flex items-center gap-3 overflow-hidden rounded-xl border border-red-500/25 bg-card p-2 pr-4 transition-colors hover:bg-accent/40 md:mx-6",
                    div { class: "relative aspect-video w-36 shrink-0 overflow-hidden rounded-lg bg-muted sm:w-44",
                        img { class: "absolute inset-0 size-full object-cover", src: "{st.thumb}?t={util::now() / 120}", alt: "" }
                        span { class: "absolute left-1 top-1 rounded bg-red-600 px-1 text-[10px] font-bold text-white", "LIVE" }
                    }
                    div { class: "min-w-0 flex-1",
                        div { class: "flex items-center gap-1.5 text-xs font-medium text-red-500", TwitchGlyph { class: "size-3.5 {PURPLE}" } "{name} ist jetzt live auf Twitch" }
                        div { class: "mt-0.5 line-clamp-1 text-sm font-medium", "{st.title}" }
                        div { class: "text-xs text-muted-foreground", "{st.game} • {util::number(st.viewers)} Zuschauer" }
                    }
                    icons::Play { class: "size-5 shrink-0 fill-current text-muted-foreground group-hover:text-foreground" }
                }
            }
        }
        div { class: "flex flex-wrap items-center gap-2 px-4 pb-3 md:px-6",
            if !list.is_empty() {
                span { class: "text-xs text-muted-foreground", "Auch auf Twitch:" }
                LinkedChannels { links: list.clone(), to_twitch: true,
                    on_unlink: move |lk: ChannelLink| {
                        spawn(async move {
                            let _ = api::delete::<serde_json::Value>(&format!("/api/links/{}/{}", lk.yt_id, lk.tw_login)).await;
                            toast("Verknüpfung gelöst");
                            links.restart();
                        });
                    }
                }
            }
            if adding() {
                form { class: "flex items-center gap-2",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let tw = input().trim().trim_start_matches("https://").trim_start_matches("www.").trim_start_matches("twitch.tv/").trim_start_matches('@').to_string();
                        let yt = yt_add.clone();
                        spawn(async move {
                            match api::post::<_, serde_json::Value>("/api/links", &LinkBody { yt_id: yt, tw_login: tw }).await {
                                Ok(_) => { toast("Mit Twitch verknüpft"); adding.set(false); input.set(String::new()); links.restart(); reload_tw_live().await; }
                                Err(e) => toast_err(e),
                            }
                        });
                    },
                    input { class: "h-7 w-44 rounded-full border border-input bg-input/30 px-3 text-xs outline-none", placeholder: "Twitch-Kanal", value: "{input}", autofocus: true, oninput: move |e| input.set(e.value()) }
                    button { r#type: "submit", class: "h-7 rounded-full bg-primary px-3 text-xs font-medium text-primary-foreground", "Verknüpfen" }
                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconXs, onclick: move |_| adding.set(false), icons::X {} }
                }
            } else {
                button { class: "flex items-center gap-1 rounded-full px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground",
                    onclick: move |_| adding.set(true),
                    TwitchGlyph { class: "size-3" }
                    if list.is_empty() { "Twitch-Kanal verknüpfen" } else { "Weiteren verknüpfen" }
                }
            }
        }
    }
}

/// Auf YouTube-Videoseiten: kleiner Hinweis, wenn der Kanal auf Twitch ist (oder gerade live)
#[component]
pub fn TwitchChip(yt_id: String, description: String) -> Element {
    let hint = twitch_logins(&description).join(",");
    let key = (yt_id.clone(), hint.clone());
    let links = use_resource(use_reactive!(|key| async move {
        let (id, hint) = key;
        if !id.starts_with("UC") {
            return vec![];
        }
        api::get::<Vec<ChannelLink>>(&format!("/api/links/youtube/{id}?hint={}", api::enc(&hint))).await.unwrap_or_default()
    }));
    if !SETTINGS().twitch_enabled {
        return rsx! {};
    }
    let list = links().unwrap_or_default();
    let Some(l) = list.iter().find(|l| l.live.is_some()).or(list.first()).cloned() else { return rsx! {} };
    let live = l.live.is_some();
    let route = if live {
        Route::Watch { v: format!("tw:live:{}", l.tw_login), list: String::new(), t: String::new() }
    } else {
        Route::TwitchChannelPage { login: l.tw_login.clone(), tab: String::new() }
    };
    let name = if l.tw_name.is_empty() { l.tw_login.clone() } else { l.tw_name.clone() };
    rsx! {
        Link { to: route, title: "{l.source_label()}",
            class: if live { "flex h-8 shrink-0 items-center gap-1.5 rounded-full bg-red-600 px-3 text-xs font-semibold text-white hover:bg-red-500" } else { "flex h-8 shrink-0 items-center gap-1.5 rounded-full bg-secondary px-3 text-xs font-medium hover:bg-accent" },
            TwitchGlyph { class: if live { "size-3.5 text-white" } else { "size-3.5 text-[#a970ff]" } }
            if live { "Jetzt live auf Twitch" } else { "{name} auf Twitch" }
        }
    }
}
