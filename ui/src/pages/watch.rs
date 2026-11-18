//! Video-Seite: Player (im Layout), Titel, Kanal, Beschreibung mit Kapiteln, Kommentare,
//! ähnliche Videos oder die laufende Playlist.

use crate::Route;
use crate::api;
use crate::components::*;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::util;
use crate::pages::twitch::{LiveChat, TwitchChip, TwitchWatch};
use dioxus::prelude::*;
use nrztube_types::*;

#[component]
pub fn Watch(v: String, list: String, t: String) -> Element {
    if is_twitch_id(&v) {
        return rsx! { TwitchWatchRoute { v, t } };
    }
    rsx! { YtWatch { v, list, t } }
}

/// Twitch: Player setzen, dann die Twitch-Seite
#[component]
fn TwitchWatchRoute(v: String, t: String) -> Element {
    let key = (v.clone(), t.clone());
    use_effect(use_reactive!(|key| {
        let (v, t) = key.clone();
        let start: f64 = t.trim_end_matches('s').parse().unwrap_or(0.0);
        let same = NOW.peek().as_ref().map(|n| n.id == v).unwrap_or(false);
        if !same {
            *NOW.write() = Some(NowPlaying { id: v.clone(), list: None, local_list: None, start });
        } else if start > 0.0 {
            *SEEK.write() = Some(start);
        }
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("main-scroll")) {
            el.set_scroll_top(0);
        }
    }));
    rsx! { TwitchWatch { key: "{v}", v: v.clone() } }
}

#[component]
fn YtWatch(v: String, list: String, t: String) -> Element {
    // Player auf dieses Video setzen (ausser es läuft schon – z. B. aus dem Mini-Player)
    let key = (v.clone(), list.clone(), t.clone());
    use_effect(use_reactive!(|key| {
        let (v, list, t) = key.clone();
        let local_list = list.strip_prefix("local:").and_then(|x| x.parse::<i64>().ok());
        let yt_list = if list.is_empty() || local_list.is_some() { None } else { Some(list.clone()) };
        let start: f64 = t.trim_end_matches('s').parse().unwrap_or(0.0);
        let same = NOW.peek().as_ref().map(|n| n.id == v).unwrap_or(false);
        if !same {
            *NOW.write() = Some(NowPlaying { id: v.clone(), list: yt_list, local_list, start });
        } else if start > 0.0 {
            *SEEK.write() = Some(start);
        }
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("main-scroll")) {
            el.set_scroll_top(0);
        }
    }));

    let vid = v.clone();
    let lst = list.clone();
    let details = use_resource(use_reactive!(|vid, lst| async move {
        let yt_list = if lst.is_empty() || lst.starts_with("local:") { String::new() } else { format!("?list={}", api::enc(&lst)) };
        let r = api::get::<VideoDetails>(&format!("/api/video/{vid}{yt_list}")).await;
        if let Ok(d) = &r {
            *DETAILS.write() = Some(d.clone());
            load_dearrow(d.related.iter().map(|x| x.id.clone()).chain(std::iter::once(d.id.clone())).collect());
        }
        r
    }));

    let s = SETTINGS();
    // Auf dem Handy grosse Karten wie im Feed, auf breiten Bildschirmen die kompakte Liste daneben
    let wide = web_sys::window().and_then(|w| w.inner_width().ok()).and_then(|x| x.as_f64()).unwrap_or(1400.0) >= 1280.0;
    let local_list = list.strip_prefix("local:").and_then(|x| x.parse::<i64>().ok());
    // Livestream: Livechat (breit rechts angedockt, sonst über den ähnlichen Videos)
    let live_id = match details() { Some(Ok(d)) if d.live => Some(d.id.clone()), _ => None };
    let dock = live_id.is_some() && s.twitch_chat && wide;
    use_effect(use_reactive!(|dock| { *CHAT_DOCK.write() = dock; }));
    use_drop(|| { *CHAT_DOCK.write() = false; });
    let wrap = if s.theater { "mx-auto max-w-[1800px] px-4 py-4 md:px-6" } else { "mx-auto max-w-6xl px-4 py-4 md:px-6" };
    rsx! {
        div { class: "{wrap}",
            match details() {
                // Noch nicht da (oder noch das vorige Video): Titel sofort, Rest als Platzhalter
                None => rsx! { PendingInfo { v: v.clone() } },
                Some(Ok(d)) if d.id != v => rsx! { PendingInfo { v: v.clone() } },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message, retry: move |_| { let mut d = details; d.restart(); } } },
                Some(Ok(d)) => rsx! {
                    // Livestream: eine Spalte (Chat ist angedockt oder steht unter dem Video)
                    div { class: if d.live { "grid gap-6" } else { "grid gap-6 xl:grid-cols-[minmax(0,1fr)_400px]" },
                        div { class: "min-w-0",
                            VideoInfo { d: d.clone() }
                            if !s.hide_comments {
                                if let Some(tok) = d.comments_token.clone() {
                                    Comments { key: "{d.id}", token: tok, count: d.comment_count.clone() }
                                }
                            }
                        }
                        aside { class: "min-w-0",
                            if let Some(lid) = live_id.clone().filter(|_| s.twitch_chat && !dock && !CHAT_HIDDEN()) {
                                div { class: "mb-6 h-[60vh] overflow-hidden rounded-xl border bg-card",
                                    LiveChat { key: "{lid}", kind: "youtube".to_string(), id: lid.clone() }
                                }
                            }
                            if let Some(pid) = local_list {
                                LocalQueue { id: pid, current: d.id.clone() }
                            }
                            if let Some(pl) = d.playlist.clone() {
                                PlaylistPanel { pl, current: d.id.clone() }
                            }
                            // Livestreams: keine ähnlichen Videos (aufgeräumter)
                            if !s.hide_related && !d.related.is_empty() && !d.live {
                                div { class: "flex flex-col gap-3",
                                    h2 { class: "text-sm font-semibold text-muted-foreground", "Ähnliche Videos" }
                                    for r in d.related.iter() {
                                        VideoCard { key: "{r.id}", v: r.clone(), compact: wide }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
        if let Some(lid) = live_id.clone().filter(|_| dock) {
            aside { class: if CHAT_HIDDEN() { "hidden" } else { "fixed bottom-0 right-0 top-14 md:top-[52px] z-20 hidden w-[var(--chat-w,360px)] border-l bg-card xl:block" },
                // Trennlinie: ziehen ändert die Breite zwischen Player und Chat
                div { class: "chat-resize absolute inset-y-0 -left-1 z-10 w-2 cursor-col-resize hover:bg-foreground/10", title: "Ziehen, um die Breite zu ändern",
                    onmousedown: move |_| crate::util::eval("window.nrzChatResize && window.nrzChatResize()"),
                }
                LiveChat { key: "{lid}", kind: "youtube".to_string(), id: lid.clone() }
            }
        }
    }
}

#[component]
fn VideoInfo(d: VideoDetails) -> Element {
    let s = SETTINGS();
    let mut expanded = use_signal(|| false);
    let subscribed = is_subscribed(&d.author_id);
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
        published: d.relative_date.clone(),
        live: d.live,
        ..Default::default()
    };
    let (title, original) = display_title(&item);
    let watch_later = PLAYLISTS().into_iter().find(|p| p.protected && p.title == "Später ansehen").map(|p| p.id);
    let (cid, cname, cthumb) = (d.author_id.clone(), d.author.clone(), d.author_thumb.clone().unwrap_or_default());
    let item_wl = item.clone();
    let item_fav = item.clone();
    let item_add = item.clone();
    let item_stream = item.clone();
    let item_party = item_stream.clone();
    let id_link = d.id.clone();
    let mut more = use_signal(|| false);
    let mut meta = Vec::new();
    if !s.hide_views {
        if let Some(v) = &d.views_text {
            meta.push(v.clone());
        }
    }
    if !s.hide_upload_date {
        if let Some(x) = d.date_text.clone().or(d.relative_date.clone()) {
            meta.push(x);
        }
    }
    rsx! {
        h1 { class: "text-lg font-semibold leading-snug md:text-xl", title: original.clone().map(|o| format!("Original: {o}")).unwrap_or_default(), "{title}" }
        if let Some(o) = original.clone().filter(|_| s.dearrow_show_original) {
            p { class: "mt-0.5 text-xs text-muted-foreground", "Original: {o}" }
        }
        div { class: "mt-3 flex flex-wrap items-center gap-3",
            Link { to: Route::ChannelPage { id: d.author_id.clone(), tab: String::new() }, class: "flex min-w-0 items-center gap-3",
                if !s.hide_channel_avatars {
                    if let Some(a) = &d.author_thumb { img { class: "size-10 rounded-full bg-muted object-cover", src: "{a}", alt: "" } }
                }
                div { class: "min-w-0",
                    div { class: "flex items-center gap-1 truncate font-medium", "{d.author}" if d.verified { icons::Check { class: "size-3.5" } } }
                    if !s.hide_subscriber_counts { if let Some(sc) = &d.subscribers { div { class: "text-xs text-muted-foreground", "{sc}" } } }
                }
            }
            Button { variant: if subscribed { ButtonVariant::Secondary } else { ButtonVariant::Default }, size: ButtonSize::Sm,
                onclick: move |_| { let (a, b, c) = (cid.clone(), cname.clone(), cthumb.clone()); spawn(async move { toggle_subscription(a, b, c).await; }); },
                if subscribed { icons::BellOff {} "Abonniert" } else { icons::Bell {} "Abonnieren" }
            }
            TwitchChip { yt_id: d.author_id.clone(), description: d.description.clone() }
            // Handy: Aktionen als wischbare Leiste wie bei YouTube
            div { class: "ml-auto flex items-center gap-2",
                LikeBar { item: item_fav.clone(), likes_text: if s.hide_likes { None } else { d.likes_text.clone() } }
                // Weitere Aktionen im ⋮-Menü – unter dem Video bleibt es aufgeräumt
                div { class: "relative",
                    Button { variant: ButtonVariant::Secondary, size: ButtonSize::IconSm, title: "Weitere Aktionen",
                        onclick: move |_| more.set(!more()),
                        icons::EllipsisVertical {}
                    }
                    if more() {
                        div { class: "fixed inset-0 z-40", onclick: move |_| more.set(false) }
                        div { class: "absolute right-0 top-full z-50 mt-1 w-56 overflow-hidden rounded-xl border bg-popover p-1 text-sm shadow-2xl",
                            if SETTINGS().stream_enabled {
                                button { class: "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-accent",
                                    onclick: move |_| { add_to_stream(item_stream.clone()); more.set(false); },
                                    icons::Radio { class: "size-4" } "Zur Stream-Liste"
                                }
                            }
                            button { class: "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-accent",
                                onclick: move |_| {
                                    let v = item_party.clone();
                                    if crate::pages::party::PARTY.peek().is_some() { crate::pages::party::enqueue(v); } else { crate::pages::party::start_party(Some(v)); }
                                    more.set(false);
                                },
                                icons::Users { class: "size-4" } if crate::pages::party::PARTY().is_some() { "In Watch-Party einreihen" } else { "Zusammen schauen" }
                            }
                            if let Some(pid) = watch_later {
                                button { class: "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-accent",
                                    onclick: move |_| {
                                        more.set(false);
                                        let v = item_wl.clone();
                                        spawn(async move {
                                            match api::post::<_, serde_json::Value>(&format!("/api/playlists/{pid}/videos"), &v).await {
                                                Ok(_) => { toast("Zu „Später ansehen“ hinzugefügt"); reload_playlists().await; }
                                                Err(e) => toast_err(e),
                                            }
                                        });
                                    },
                                    icons::Clock { class: "size-4" } "Später ansehen"
                                }
                            }
                            button { class: "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-accent",
                                onclick: move |_| { *ADD_TO.write() = Some(item_add.clone()); more.set(false); },
                                icons::ListPlus { class: "size-4" } "In Playlist speichern"
                            }
                            button { class: "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-accent",
                                onclick: move |_| { util::copy(&format!("https://youtu.be/{id_link}")); toast("YouTube-Link kopiert"); more.set(false); },
                                icons::Share2 { class: "size-4" } "Teilen (Link kopieren)"
                            }
                        }
                    }
                }
            }
        }
        crate::pages::react_info::ReactPanel { video: d.id.clone(), channel: d.author_id.clone() }
        if !s.hide_description || !s.hide_chapters {
            div { class: "mt-4 rounded-xl bg-card p-4 text-sm",
                if !meta.is_empty() { div { class: "mb-2 font-medium", {meta.join(" • ")} } }
                if !s.hide_description {
                    {
                        let long = d.description.lines().count() > 3 || d.description.len() > 300;
                        rsx! {
                            div { class: if expanded() || !long { "" } else { "line-clamp-3 cursor-pointer" },
                                // Eingeklappt: Klick klappt auf (zu nur über den Knopf)
                                onclick: move |_| { if long && !expanded() { expanded.set(true); } },
                                RichText { text: d.description.clone() }
                            }
                            if long {
                                button { class: "mt-2 text-xs font-medium text-muted-foreground hover:text-foreground", onclick: move |_| expanded.set(!expanded()),
                                    if expanded() { "Weniger anzeigen" } else { "Mehr anzeigen" }
                                }
                            }
                        }
                    }
                }
                if !s.hide_chapters && !d.chapters.is_empty() {
                    div { class: "mt-4",
                        h3 { class: "mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground", "Kapitel" }
                        div { class: "shelf flex gap-2 overflow-x-auto pb-1",
                            for c in d.chapters.iter().cloned() {
                                button { key: "{c.start}", class: "flex w-44 shrink-0 flex-col gap-1 rounded-lg p-1.5 text-left hover:bg-accent",
                                    onclick: move |_| *SEEK.write() = Some(c.start),
                                    if let Some(th) = &c.thumb { img { class: "aspect-video w-full rounded-md object-cover", src: "{th}", alt: "", loading: "lazy" } }
                                    span { class: "line-clamp-2 text-xs font-medium", "{c.title}" }
                                    span { class: "text-[11px] text-sky-300 tabular-nums", {util::duration(c.start)} }
                                }
                            }
                        }
                    }
                }
                if !d.family_safe {
                    div { class: "mt-3 inline-flex items-center gap-1.5 rounded-md bg-muted px-2 py-1 text-xs text-muted-foreground", icons::Shield { class: "size-3.5" } "Nicht als familienfreundlich markiert" }
                }
            }
        }
    }
}

#[component]
fn PlaylistPanel(pl: WatchPlaylist, current: String) -> Element {
    rsx! {
        div { class: "mb-5 overflow-hidden rounded-xl border bg-card",
            div { class: "border-b p-3",
                Link { to: Route::YtPlaylist { list: pl.id.clone() }, class: "font-semibold hover:underline", "{pl.title}" }
                div { class: "text-xs text-muted-foreground", "{pl.author} • {pl.index + 1} / {pl.videos.len()}" }
            }
            div { class: "max-h-[420px] overflow-y-auto p-2",
                for (i, v) in pl.videos.iter().enumerate() {
                    Link { key: "{v.id}", to: watch_route(&v.id, Some(&pl.id), None),
                        class: if v.id == current { "flex gap-2 rounded-lg bg-accent p-1.5" } else { "flex gap-2 rounded-lg p-1.5 hover:bg-accent/60" },
                        span { class: "w-5 shrink-0 pt-3 text-center text-xs text-muted-foreground", if v.id == current { "▶" } else { "{i + 1}" } }
                        img { class: "aspect-video w-24 shrink-0 rounded-md object-cover", src: "{v.thumb}", alt: "", loading: "lazy" }
                        div { class: "min-w-0",
                            div { class: "line-clamp-2 text-xs font-medium", "{v.title}" }
                            div { class: "truncate text-[11px] text-muted-foreground", "{v.author}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn LocalQueue(id: i64, current: String) -> Element {
    let pl = use_resource(use_reactive!(|id| async move { api::get::<LocalPlaylist>(&format!("/api/playlists/{id}")).await }));
    let Some(Ok(p)) = pl() else { return rsx! {} };
    let idx = p.videos.iter().position(|v| v.id == current).map(|i| i + 1).unwrap_or(0);
    rsx! {
        div { class: "mb-5 overflow-hidden rounded-xl border bg-card",
            div { class: "border-b p-3",
                Link { to: Route::LocalPlaylistPage { id }, class: "font-semibold hover:underline", "{p.title}" }
                div { class: "text-xs text-muted-foreground", "Lokale Playlist • {idx} / {p.videos.len()}" }
            }
            div { class: "max-h-[420px] overflow-y-auto p-2",
                for v in p.videos.iter() {
                    Link { key: "{v.id}", to: Route::Watch { v: v.id.clone(), list: format!("local:{id}"), t: String::new() },
                        class: if v.id == current { "flex gap-2 rounded-lg bg-accent p-1.5" } else { "flex gap-2 rounded-lg p-1.5 hover:bg-accent/60" },
                        img { class: "aspect-video w-24 shrink-0 rounded-md object-cover", src: "{v.thumb}", alt: "", loading: "lazy" }
                        div { class: "min-w-0",
                            div { class: "line-clamp-2 text-xs font-medium", "{v.title}" }
                            div { class: "truncate text-[11px] text-muted-foreground", "{v.author}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Comments(token: String, count: Option<String>) -> Element {
    let mut comments = use_signal(Vec::<Comment>::new);
    let mut cont = use_signal(|| None::<String>);
    let mut newest = use_signal(|| None::<String>);
    let mut top = use_signal(|| None::<String>);
    let mut sort_newest = use_signal(|| false);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);
    let mut total = use_signal(|| count.clone());
    let mut load = move |tok: String, replace: bool| {
        loading.set(true);
        spawn(async move {
            match api::get::<CommentPage>(&format!("/api/comments?token={}", api::enc(&tok))).await {
                Ok(p) => {
                    if replace {
                        comments.set(p.comments);
                    } else {
                        comments.write().extend(p.comments);
                    }
                    cont.set(p.continuation);
                    if p.newest_token.is_some() {
                        newest.set(p.newest_token);
                        top.set(p.top_token);
                    }
                    if let Some(c) = p.count {
                        total.set(Some(c));
                    }
                }
                Err(e) => error.set(Some(e.message)),
            }
            loading.set(false);
        });
    };
    let t0 = token.clone();
    use_hook(move || load(t0, true));
    rsx! {
        section { class: "mt-6",
            div { class: "mb-4 flex items-center gap-3",
                h2 { class: "text-base font-semibold", {total().map(|c| if c.contains("Komm") { c } else { format!("{c} Kommentare") }).unwrap_or_else(|| "Kommentare".into())} }
                if newest().is_some() {
                    div { class: "ml-auto flex gap-1 text-xs",
                        button { class: if !sort_newest() { "rounded-full bg-primary px-3 py-1 text-primary-foreground" } else { "rounded-full bg-secondary px-3 py-1" },
                            onclick: move |_| { if let Some(t) = top() { sort_newest.set(false); load(t, true); } }, "Top" }
                        button { class: if sort_newest() { "rounded-full bg-primary px-3 py-1 text-primary-foreground" } else { "rounded-full bg-secondary px-3 py-1" },
                            onclick: move |_| { if let Some(t) = newest() { sort_newest.set(true); load(t, true); } }, "Neueste" }
                    }
                }
            }
            if let Some(e) = error() { p { class: "text-sm text-muted-foreground", "{e}" } }
            div { class: "flex flex-col gap-5",
                for c in comments() {
                    CommentView { key: "{c.id}", c: c.clone() }
                }
            }
            if loading() {
                Loading {}
            } else if let Some(c) = cont() {
                LoadMore { loading: false, onclick: move |_| load(c.clone(), false) }
            }
        }
    }
}

#[component]
fn CommentView(c: Comment) -> Element {
    let s = SETTINGS();
    let mut replies = use_signal(Vec::<Comment>::new);
    let mut replies_cont = use_signal(|| None::<String>);
    let mut open = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut fetch = move |tok: String| {
        busy.set(true);
        spawn(async move {
            if let Ok(p) = api::get::<CommentPage>(&format!("/api/comments?token={}", api::enc(&tok))).await {
                replies.write().extend(p.comments);
                replies_cont.set(p.continuation);
            }
            busy.set(false);
        });
    };
    let tok = c.replies_token.clone();
    rsx! {
        div { class: "flex gap-3",
            if !s.hide_comment_avatars {
                Link { to: Route::ChannelPage { id: c.author_id.clone(), tab: String::new() }, class: "shrink-0",
                    img { class: "size-9 rounded-full bg-muted object-cover", src: "{c.author_thumb}", alt: "", loading: "lazy" }
                }
            }
            div { class: "min-w-0 flex-1",
                div { class: "flex flex-wrap items-center gap-2 text-xs",
                    if c.pinned { span { class: "flex items-center gap-1 text-muted-foreground", icons::Pin { class: "size-3" } "Angepinnt" } }
                    span { class: if c.by_owner { "rounded-full bg-secondary px-2 py-0.5 font-medium" } else { "font-medium" }, "{c.author}" }
                    if c.verified { icons::Check { class: "size-3" } }
                    span { class: "text-muted-foreground", "{c.published}" }
                }
                RichText { text: c.text.clone(), class: "mt-1 text-sm" }
                div { class: "mt-1.5 flex items-center gap-4 text-xs text-muted-foreground",
                    if !s.hide_likes && !c.likes.is_empty() { span { class: "flex items-center gap-1", icons::ThumbsUp { class: "size-3.5" } "{c.likes}" } }
                    if c.hearted { span { class: "flex items-center gap-1 text-red-400", icons::Heart { class: "size-3.5 fill-current" } "vom Kanal" } }
                    if let Some(t) = tok.clone() {
                        button { class: "flex items-center gap-1 font-medium text-sky-300 hover:underline",
                            onclick: move |_| { let o = !open(); open.set(o); if o && replies().is_empty() { fetch(t.clone()); } },
                            if open() { icons::ChevronUp { class: "size-3.5" } } else { icons::ChevronDown { class: "size-3.5" } }
                            {if c.reply_count > 0 { format!("{} Antworten", c.reply_count) } else { "Antworten".into() }}
                        }
                    }
                }
                if open() {
                    div { class: "mt-3 flex flex-col gap-4",
                        for r in replies() { CommentView { key: "{r.id}", c: r.clone() } }
                        if busy() { div { class: "text-xs text-muted-foreground", "Lädt …" } }
                        else if let Some(t) = replies_cont() {
                            button { class: "self-start text-xs font-medium text-sky-300 hover:underline", onclick: move |_| fetch(t.clone()), "Weitere Antworten" }
                        }
                    }
                }
            }
        }
    }
}

/// Daumen hoch (= Favorit) und Daumen runter, mit Zahlen von YouTube bzw. Return YouTube Dislike
#[component]
fn LikeBar(item: VideoItem, likes_text: Option<String>) -> Element {
    let id = item.id.clone();
    let rating = use_resource(use_reactive!(|id| async move { api::get::<Rating>(&format!("/api/rating/{id}")).await.ok() }));
    let server = rating.read().clone().flatten().unwrap_or_default();
    let mine = RATINGS().get(&item.id).copied().unwrap_or(server.rating);
    let hide = SETTINGS().hide_likes;
    // Eigene Stimme in die Zahl einrechnen
    let likes_label = likes_text.clone().or_else(|| server.likes.map(|l| util::compact(l)));
    let dislikes = server.dislikes.map(|d| d + if mine == -1 && server.rating != -1 { 1 } else { 0 });
    let (a, b) = (item.clone(), item.clone());
    rsx! {
        div { class: "flex h-8 items-center overflow-hidden rounded-full bg-secondary text-sm",
            button { class: "flex h-full items-center gap-1.5 pl-3 pr-3 hover:bg-accent", title: if mine == 1 { "Gefällt mir nicht mehr" } else { "Gefällt mir – kommt in die Favoriten" },
                onclick: move |_| rate(a.clone(), if mine == 1 { 0 } else { 1 }),
                span { class: if mine == 1 { "flex [&_svg]:fill-current" } else { "flex" }, icons::ThumbsUp { class: "size-4" } }
                if !hide { if let Some(l) = likes_label { span { class: "tabular-nums", "{l}" } } }
            }
            span { class: "h-5 w-px bg-border" }
            button { class: "flex h-full items-center gap-1.5 pl-3 pr-3 hover:bg-accent", title: "Gefällt mir nicht – wird seltener vorgeschlagen",
                onclick: move |_| rate(b.clone(), if mine == -1 { 0 } else { -1 }),
                span { class: if mine == -1 { "flex [&_svg]:fill-current" } else { "flex" }, icons::ThumbsDown { class: "size-4" } }
                if !hide { if let Some(d) = dislikes { span { class: "tabular-nums", {util::compact(d)} } } }
            }
        }
    }
}

/// Während die Details laden: Titel und Kanal des angeklickten Videos, Platzhalter daneben
#[component]
fn PendingInfo(v: String) -> Element {
    let p = PENDING().filter(|p| p.id == v);
    rsx! {
        div { class: "grid gap-6 xl:grid-cols-[minmax(0,1fr)_400px]",
            div { class: "min-w-0",
                if let Some(p) = &p {
                    h1 { class: "text-lg font-semibold leading-snug md:text-xl", "{p.title}" }
                    div { class: "mt-3 flex items-center gap-3",
                        if let Some(a) = &p.author_thumb { img { class: "size-10 rounded-full bg-muted object-cover", src: "{a}", alt: "" } } else { div { class: "size-10 animate-pulse rounded-full bg-muted" } }
                        div { class: "font-medium", "{p.author}" }
                    }
                } else {
                    div { class: "h-6 w-3/4 animate-pulse rounded bg-muted" }
                    div { class: "mt-4 flex items-center gap-3",
                        div { class: "size-10 animate-pulse rounded-full bg-muted" }
                        div { class: "h-4 w-40 animate-pulse rounded bg-muted" }
                    }
                }
                div { class: "mt-4 h-24 animate-pulse rounded-xl bg-card" }
            }
            aside { class: "flex min-w-0 flex-col gap-3",
                for i in 0..6 {
                    div { key: "{i}", class: "flex gap-2",
                        div { class: "aspect-video w-40 shrink-0 animate-pulse rounded-lg bg-muted" }
                        div { class: "flex-1 space-y-2 pt-1",
                            div { class: "h-3.5 w-11/12 animate-pulse rounded bg-muted" }
                            div { class: "h-3 w-2/3 animate-pulse rounded bg-muted" }
                        }
                    }
                }
            }
        }
    }
}
