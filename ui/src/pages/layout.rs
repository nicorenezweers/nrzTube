//! Gerüst: Kopf mit Suche, Seitenleiste (Profile, Abos), Player (gross oder Mini) und Inhalt.

use crate::Route;
use crate::api;
use crate::components::{AddToPlaylistDialog, ShortcutsDialog, TwitchGlyph, VideoMenu, watch_route};
use crate::player::PlayerHost;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use dioxus::prelude::*;

#[component]
pub fn Layout() -> Element {
    let route = use_route::<Route>();
    let s = SETTINGS();
    let on_watch = matches!(&route, Route::Watch { v, .. } if NOW().map(|n| &n.id == v).unwrap_or(false));
    // Ohne Mini-Player stoppt das Video beim Verlassen der Seite
    let mini_allowed = s.mini_player;
    use_effect(use_reactive!(|on_watch, mini_allowed| {
        if !on_watch && !mini_allowed && NOW.peek().is_some() {
            crate::util::eval("window.nrzDetach('nrz-video')");
            *NOW.write() = None;
        }
    }));
    let route_key = format!("{route:?}");
    use_effect(use_reactive!(|route_key| { let _ = route_key; if CTX.peek().is_some() { *CTX.write() = None; } if SIDE_PREVIEW.peek().is_some() { *SIDE_PREVIEW.write() = None; } }));
    let blur = if s.blur_thumbnails { "blur-thumbs" } else { "" };
    // Twitch-Livestatus alle zwei Minuten auffrischen
    use_future(|| async {
        loop {
            gloo_timers::future::TimeoutFuture::new(120_000).await;
            reload_tw_live().await;
        }
    });
    rsx! {
        div { class: "flex h-dvh flex-col overflow-hidden {blur}",
            Header {}
            div { class: "flex min-h-0 flex-1",
                Sidebar {}
                main { id: "main-scroll", class: if CHAT_DOCK() && !CHAT_HIDDEN() { "min-w-0 flex-1 overflow-y-auto pb-20 md:pb-6 xl:mr-[var(--chat-w,360px)]" } else { "min-w-0 flex-1 overflow-y-auto pb-20 md:pb-6" },
                    PlayerHost { big: on_watch }
                    Outlet::<Route> {}
                }
            }
            BottomNav {}
        }
        AddToPlaylistDialog {}
        ShortcutsDialog {}
        VideoMenu {}
        if let Some((v, y)) = SIDE_PREVIEW() {
            div { class: "pointer-events-none fixed left-[244px] z-[80] hidden w-72 overflow-hidden rounded-xl border bg-popover shadow-2xl md:block",
                style: "top: clamp(60px, {y - 70.0}px, calc(100vh - 230px))",
                div { class: "relative aspect-video bg-muted",
                    if !v.thumb.is_empty() { img { class: "absolute inset-0 size-full object-cover", src: "{v.thumb}", alt: "" } }
                    span { class: "absolute left-2 top-2 flex items-center gap-1 rounded bg-red-600 px-1.5 py-0.5 text-[10px] font-bold text-white", span { class: "size-1.5 rounded-full bg-white" } "LIVE" }
                    if let Some(t) = &v.views_text { span { class: "absolute bottom-2 left-2 rounded bg-black/70 px-1.5 py-0.5 text-[11px] text-white tabular-nums", "{t}" } }
                }
                div { class: "p-2.5",
                    div { class: "line-clamp-2 text-sm font-medium leading-snug", "{v.title}" }
                    div { class: "mt-0.5 truncate text-xs text-muted-foreground", "{v.author}" if let Some(g) = &v.description { " · {g}" } }
                }
            }
        }
    }
}

#[component]
fn Header() -> Element {
    let nav = navigator();
    let route = use_route::<Route>();
    let initial = match &route {
        Route::Search { q, .. } => q.clone(),
        _ => String::new(),
    };
    let mut q = use_signal(|| initial.clone());
    use_effect(use_reactive!(|initial| q.set(initial)));
    let mut suggestions = use_signal(Vec::<String>::new);
    let mut focused = use_signal(|| false);
    let mut sel = use_signal(|| None::<usize>);
    let hide_sugg = SETTINGS().hide_search_suggestions || SETTINGS().stream_mode;
    let stream_on = SETTINGS().stream_mode;
    // Handy: Suche klappt über den ganzen Kopf auf
    let mut msearch = use_signal(|| false);

    let mut go = move |text: String| {
        let text = text.trim().to_string();
        suggestions.set(vec![]);
        msearch.set(false);
        if text.is_empty() {
            return;
        }
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("search")) {
            use wasm_bindgen::JsCast;
            if let Ok(i) = el.dyn_into::<web_sys::HtmlElement>() {
                let _ = i.blur();
            }
        }
        // Eingefügte YouTube-Adressen direkt öffnen
        let lower = text.to_lowercase();
        if let Some(r) = twitch_route(&text) {
            nav.push(r);
            return;
        }
        if lower.contains("youtube.com/") || lower.contains("youtu.be/") || text.starts_with('@') {
            spawn(async move {
                if let Ok(r) = api::get::<serde_json::Value>(&format!("/api/resolve?q={}", api::enc(&text))).await {
                    let id = r.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    match r.get("kind").and_then(|x| x.as_str()) {
                        Some("video") => {
                            let t = r.get("t").and_then(|x| x.as_str()).and_then(|x| x.trim_end_matches('s').parse().ok());
                            nav.push(watch_route(&id, r.get("list").and_then(|x| x.as_str()), t));
                        }
                        Some("channel") => { nav.push(Route::ChannelPage { id, tab: String::new() }); }
                        Some("playlist") => { nav.push(Route::YtPlaylist { list: id }); }
                        _ => { nav.push(Route::Search { q: text, sort: String::new(), date: String::new(), kind: String::new(), duration: String::new() }); }
                    }
                }
            });
            return;
        }
        nav.push(Route::Search { q: text, sort: String::new(), date: String::new(), kind: String::new(), duration: String::new() });
    };

    rsx! {
        header { class: "z-40 flex h-14 shrink-0 items-center gap-1.5 bg-background px-2 sm:gap-3 md:h-[52px] md:px-3 pt-safe",
            if msearch() {
                Button { variant: ButtonVariant::Ghost, size: ButtonSize::Icon, class: "sm:hidden", aria_label: "Zurück",
                    onclick: move |_| { msearch.set(false); suggestions.set(vec![]); },
                    icons::ArrowLeft {}
                }
            } else {
                Button { variant: ButtonVariant::Ghost, size: ButtonSize::Icon, class: "hidden text-muted-foreground hover:text-foreground md:inline-flex", aria_label: "Menü",
                    onclick: move |_| { let v = SIDEBAR(); *SIDEBAR.write() = !v; },
                    icons::Menu {}
                }
                Link { to: Route::Home {}, class: "flex shrink-0 items-center gap-2 px-1 font-semibold tracking-tight",
                    span { class: "flex size-6 items-center justify-center rounded-md bg-primary text-primary-foreground",
                        icons::Play { class: "size-3.5 fill-current" }
                    }
                    span { class: "text-[16px] sm:text-[15px]", "nrzTube" }
                }
                if stream_on {
                    // dezenter Hinweis; Klick führt zu den Stream-Einstellungen
                    Link { to: Route::SettingsPage { tab: "stream".into() }, title: "Stream-Modus ist an",
                        class: "flex items-center gap-1.5 rounded-full border border-border px-2 py-0.5 text-[11px] text-muted-foreground hover:text-foreground",
                        span { class: "size-1.5 rounded-full bg-muted-foreground/70" }
                        "Stream"
                    }
                }
            }
            div { class: if msearch() { "relative mx-auto w-full max-w-xl" } else { "relative mx-auto hidden w-full max-w-xl sm:block" },
                form { class: "relative",
                    onsubmit: move |e| { e.prevent_default(); let t = match sel() { Some(i) => suggestions().get(i).cloned().unwrap_or(q()), None => q() }; q.set(t.clone()); go(t); },
                    icons::Search { class: "pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground/70" }
                    if !focused() && q().is_empty() {
                        kbd { class: "pointer-events-none absolute right-2.5 top-1/2 hidden -translate-y-1/2 rounded border border-border/60 px-1.5 text-[10px] text-muted-foreground/70 md:block", "/" }
                    }
                    input {
                        id: "search",
                        r#type: "search",
                        autocomplete: "off",
                        enterkeyhint: "search",
                        class: "h-9 w-full rounded-lg bg-secondary/60 pl-9 pr-8 text-[16px] outline-none transition-colors placeholder:text-muted-foreground/70 hover:bg-secondary focus:bg-secondary focus-visible:ring-1 focus-visible:ring-ring/40 sm:text-sm",
                        placeholder: "Suchen oder Link einfügen",
                        value: "{q}",
                        onfocus: move |_| focused.set(true),
                        onblur: move |_| {
                            spawn(async move { gloo_timers::future::TimeoutFuture::new(200).await; focused.set(false); });
                        },
                        onkeydown: move |e| {
                            let n = suggestions().len();
                            match e.key() {
                                Key::ArrowDown if n > 0 => { e.prevent_default(); sel.set(Some(sel().map(|i| (i + 1) % n).unwrap_or(0))); }
                                Key::ArrowUp if n > 0 => { e.prevent_default(); sel.set(Some(sel().map(|i| (i + n - 1) % n).unwrap_or(n - 1))); }
                                Key::Escape => { suggestions.set(vec![]); msearch.set(false); }
                                _ => {}
                            }
                        },
                        oninput: move |e| {
                            let v = e.value();
                            q.set(v.clone());
                            sel.set(None);
                            if hide_sugg || v.trim().is_empty() { suggestions.set(vec![]); return; }
                            spawn(async move {
                                gloo_timers::future::TimeoutFuture::new(150).await;
                                if q.peek().as_str() != v { return; }
                                if let Ok(list) = api::get::<Vec<String>>(&format!("/api/suggest?q={}", api::enc(&v))).await {
                                    if q.peek().as_str() == v { suggestions.set(list); }
                                }
                            });
                        },
                    }
                }
                if focused() && !suggestions().is_empty() {
                    div { class: "fixed inset-x-0 top-14 z-50 overflow-hidden border-b bg-popover py-1 shadow-2xl sm:absolute sm:top-11 sm:rounded-xl sm:border",
                        for (i, s) in suggestions().into_iter().enumerate() {
                            button { key: "{s}",
                                class: if sel() == Some(i) { "flex w-full items-center gap-3 bg-accent px-4 py-3 text-left text-[15px] sm:py-2 sm:text-sm" } else { "flex w-full items-center gap-3 px-4 py-3 text-left text-[15px] hover:bg-accent sm:py-2 sm:text-sm" },
                                onmousedown: move |e| e.prevent_default(),
                                onclick: { let s = s.clone(); move |_| { q.set(s.clone()); go(s.clone()); } },
                                icons::Search { class: "size-3.5 text-muted-foreground" }
                                "{s}"
                            }
                        }
                    }
                }
            }
            if !msearch() {
                div { class: "ml-auto flex items-center sm:ml-0",
                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::Icon, class: "text-muted-foreground hover:text-foreground sm:hidden", aria_label: "Suchen",
                        onclick: move |_| {
                            msearch.set(true);
                            spawn(async move {
                                gloo_timers::future::TimeoutFuture::new(30).await;
                                if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("search")) {
                                    use wasm_bindgen::JsCast;
                                    if let Ok(i) = el.dyn_into::<web_sys::HtmlElement>() { let _ = i.focus(); }
                                }
                            });
                        },
                        icons::Search {}
                    }
                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::Icon, class: "text-muted-foreground hover:text-foreground", title: "Einstellungen", href: "/settings",
                        icons::Settings {}
                    }
                }
            }
        }
    }
}

#[component]
fn NavItem(to: Route, label: String, active: bool, children: Element) -> Element {
    let collapsed = SIDEBAR();
    rsx! {
        Link { to,
            class: if active { "flex items-center gap-3 rounded-lg bg-sidenav-accent px-3 py-2 text-sm font-medium" } else { "flex items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-sidenav-accent/60 hover:text-foreground" },
            title: "{label}",
            {children}
            if !collapsed { span { class: "truncate", "{label}" } }
        }
    }
}

#[component]
fn Sidebar() -> Element {
    let route = use_route::<Route>();
    let s = SETTINGS();
    let collapsed = SIDEBAR();
    let width = if collapsed { "w-16" } else { "w-60" };
    let profile_active = |id: i64| matches!(&route, Route::Feed { profile } if profile == &id.to_string());
    rsx! {
        aside { class: "hidden {width} shrink-0 flex-col gap-1 overflow-y-auto border-r bg-sidenav p-2 md:flex",
            NavItem { to: Route::Home {}, label: if s.stream_mode { "Stream" } else { "Startseite" }, active: matches!(route, Route::Home {}),
                icons::House { class: "size-4 shrink-0" }
            }
            if !s.stream_mode {
                NavItem { to: Route::Feed { profile: String::new() }, label: "Abos-Feed", active: matches!(&route, Route::Feed { profile } if profile.is_empty()),
                    icons::SquarePlay { class: "size-4 shrink-0" }
                }
            }
            Link { to: match crate::pages::party::PARTY() { Some(id) => Route::PartyPage { id, k: String::new() }, None => Route::PartyStart {} },
                class: if matches!(route, Route::PartyStart {} | Route::PartyPage { .. }) { "flex items-center gap-3 rounded-lg bg-sidenav-accent px-3 py-2 text-sm font-medium" } else { "flex items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-sidenav-accent/60 hover:text-foreground" },
                title: "Watch-Partys",
                icons::Users { class: "size-4 shrink-0" }
                if !collapsed {
                    span { class: "truncate", "Watch-Partys" }
                    if crate::pages::party::PARTY().is_some() {
                        span { class: "ml-auto flex items-center gap-1 rounded-full bg-green-500/15 px-1.5 text-[11px] font-medium text-green-400",
                            span { class: "size-1.5 rounded-full bg-green-500" } "Party"
                        }
                    }
                }
            }
            if s.twitch_enabled {
                Link { to: Route::TwitchHome { game: String::new() },
                    class: if matches!(route, Route::TwitchHome { .. } | Route::TwitchChannelPage { .. }) { "flex items-center gap-3 rounded-lg bg-sidenav-accent px-3 py-2 text-sm font-medium" } else { "flex items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-sidenav-accent/60 hover:text-foreground" },
                    title: "Twitch",
                    TwitchGlyph { class: "size-4 shrink-0" }
                    if !collapsed {
                        span { class: "truncate", "Twitch" }
                        if !TW_LIVE().is_empty() && !s.stream_mode {
                            span { class: "ml-auto flex items-center gap-1 rounded-full bg-red-500/15 px-1.5 text-[11px] font-medium text-red-400 tabular-nums",
                                span { class: "size-1.5 rounded-full bg-red-500" }
                                "{TW_LIVE().len()}"
                            }
                        }
                    }
                }
            }
            div { class: "mx-3 my-1.5 border-t" }
            if s.stream_enabled {
            Link { to: Route::StreamPage {},
                class: if matches!(route, Route::StreamPage {}) { "flex items-center gap-3 rounded-lg bg-sidenav-accent px-3 py-2 text-sm font-medium" } else { "flex items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-sidenav-accent/60 hover:text-foreground" },
                title: "Stream-Liste",
                icons::Radio { class: "size-4 shrink-0" }
                if !collapsed {
                    span { class: "truncate", "Stream-Liste" }
                    if stream_count() > 0 { span { class: "ml-auto rounded-full bg-secondary px-1.5 text-[11px] tabular-nums", "{stream_count()}" } }
                }
            }
            }
            if !s.stream_mode {
                NavItem { to: Route::Subscriptions {}, label: "Abos & Profile", active: matches!(route, Route::Subscriptions {}),
                    icons::Users { class: "size-4 shrink-0" }
                }
            }
            if !s.hide_playlists {
                NavItem { to: Route::Playlists {}, label: "Playlists", active: matches!(route, Route::Playlists {} | Route::LocalPlaylistPage { .. }),
                    icons::ListVideo { class: "size-4 shrink-0" }
                }
            }
            if !s.stream_mode {
                NavItem { to: Route::HistoryPage {}, label: "Verlauf", active: matches!(route, Route::HistoryPage {}),
                    icons::History { class: "size-4 shrink-0" }
                }
            }
            div { class: "mx-3 my-1.5 border-t" }
            NavItem { to: Route::SettingsPage { tab: String::new() }, label: "Einstellungen", active: matches!(route, Route::SettingsPage { .. }),
                icons::Settings { class: "size-4 shrink-0" }
            }
            if s.twitch_enabled && !collapsed && !s.stream_mode && (!TW_LIVE().is_empty() || !TW_FOLLOWS().is_empty()) {
                div { class: "mt-4 flex items-center gap-1.5 px-3 pb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground", "Twitch" }
                for v in TW_LIVE() {
                    div { key: "{v.id}", onmouseenter: { let v = v.clone(); move |e: MouseEvent| *SIDE_PREVIEW.write() = Some((v.clone(), e.client_coordinates().y)) }, onmouseleave: move |_| *SIDE_PREVIEW.write() = None,
                        Link { to: Route::Watch { v: v.id.clone(), list: String::new(), t: String::new() },
                            class: "flex items-center gap-3 rounded-lg px-3 py-1.5 text-sm text-muted-foreground hover:bg-sidenav-accent/60 hover:text-foreground",
                            span { class: "relative shrink-0",
                                img { class: "size-6 rounded-full object-cover", src: v.author_thumb.clone().unwrap_or_default(), alt: "", loading: "lazy" }
                                span { class: "absolute -bottom-0.5 -right-0.5 size-2.5 rounded-full border-2 border-sidenav bg-red-500" }
                            }
                            span { class: "min-w-0 flex-1",
                                span { class: "block truncate text-foreground", "{v.author}" }
                                span { class: "block truncate text-[11px]", {v.description.clone().unwrap_or_default()} }
                            }
                            span { class: "text-[11px] tabular-nums", {crate::util::compact(v.views.unwrap_or(0))} }
                        }
                    }
                }
                for f in TW_FOLLOWS().into_iter().filter(|f| !is_tw_live(&f.login)) {
                    Link { key: "{f.login}", to: Route::TwitchChannelPage { login: f.login.clone(), tab: String::new() },
                        class: "flex items-center gap-3 rounded-lg px-3 py-1.5 text-sm text-muted-foreground hover:bg-sidenav-accent/60 hover:text-foreground",
                        img { class: "size-6 shrink-0 rounded-full object-cover opacity-70 grayscale-[40%]", src: "{f.thumb}", alt: "", loading: "lazy" }
                        span { class: "truncate", "{f.name}" }
                    }
                }
            }
            if !PROFILES().is_empty() && !collapsed && !s.stream_mode {
                div { class: "mt-4 px-3 pb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground", "Profile" }
                for p in PROFILES() {
                    Link { key: "{p.id}", to: Route::Feed { profile: p.id.to_string() },
                        class: if profile_active(p.id) { "flex items-center gap-3 rounded-lg bg-sidenav-accent px-3 py-1.5 text-sm" } else { "flex items-center gap-3 rounded-lg px-3 py-1.5 text-sm text-muted-foreground hover:bg-sidenav-accent/60 hover:text-foreground" },
                        span { class: "size-2.5 shrink-0 rounded-full", style: format!("background: {}", if p.color.is_empty() { "#888".to_string() } else { p.color.clone() }) }
                        span { class: "truncate", "{p.name}" }
                        span { class: "ml-auto text-xs text-muted-foreground", "{p.channels.len()}" }
                    }
                }
            }
            if !s.hide_sidebar_subscriptions && !SUBS().is_empty() && !collapsed && !s.stream_mode {
                div { class: "mt-4 px-3 pb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground", "Abos" }
                for sub in SUBS() {
                    {
                        // Gerade live: roter Punkt, Klick öffnet direkt den Livestream
                        let live = YT_LIVE().into_iter().find(|v| v.author_id == sub.channel_id);
                        let to = match &live { Some(v) => watch_route(&v.id, None, None), None => Route::ChannelPage { id: sub.channel_id.clone(), tab: String::new() } };
                        rsx! {
                            div { key: "{sub.channel_id}", onmouseenter: { let lv = live.clone(); move |e: MouseEvent| { if let Some(v) = lv.clone() { *SIDE_PREVIEW.write() = Some((v, e.client_coordinates().y)); } } }, onmouseleave: move |_| *SIDE_PREVIEW.write() = None,
                                Link { to,
                                    class: "flex items-center gap-3 rounded-lg px-3 py-1.5 text-sm text-muted-foreground hover:bg-sidenav-accent/60 hover:text-foreground",
                                    span { class: "relative shrink-0",
                                        if sub.thumb.is_empty() || s.hide_channel_avatars {
                                            span { class: "flex size-6 items-center justify-center rounded-full bg-muted text-[10px] font-semibold", {sub.name.chars().next().map(|c| c.to_string()).unwrap_or_default()} }
                                        } else {
                                            img { class: "size-6 rounded-full object-cover", src: "{sub.thumb}", alt: "", loading: "lazy" }
                                        }
                                        if live.is_some() { span { class: "absolute -bottom-0.5 -right-0.5 size-2.5 rounded-full border-2 border-sidenav bg-red-500" } }
                                    }
                                    span { class: if live.is_some() { "truncate text-foreground" } else { "truncate" }, "{sub.name}" }
                                    if live.is_some() { span { class: "ml-auto rounded bg-red-600 px-1 text-[9px] font-bold text-white", "LIVE" } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn BottomNav() -> Element {
    let route = use_route::<Route>();
    let s = SETTINGS();
    let item = |active: bool| if active { "relative flex flex-1 flex-col items-center gap-0.5 pb-1.5 pt-2 text-[10.5px] font-medium text-foreground" } else { "relative flex flex-1 flex-col items-center gap-0.5 pb-1.5 pt-2 text-[10.5px] text-muted-foreground" };
    let n = stream_count();
    rsx! {
        nav { class: "glass fixed inset-x-0 bottom-0 z-50 flex border-t pb-safe md:hidden",
            Link { to: Route::Home {}, class: item(matches!(route, Route::Home {})),
                span { class: if matches!(route, Route::Home {}) { "flex [&_svg]:fill-current" } else { "flex" }, icons::House { class: "size-[22px]" } }
                if s.stream_mode { "Stream" } else { "Start" }
            }
            if !s.stream_mode {
                Link { to: Route::Feed { profile: String::new() }, class: item(matches!(route, Route::Feed { .. })), icons::SquarePlay { class: "size-[22px]" } "Abos" }
            }
            if s.twitch_enabled && !s.stream_mode {
                Link { to: Route::TwitchHome { game: String::new() }, class: item(matches!(route, Route::TwitchHome { .. } | Route::TwitchChannelPage { .. })),
                    span { class: "relative",
                        TwitchGlyph { class: "size-[22px]" }
                        if !TW_LIVE().is_empty() { span { class: "absolute -right-1 -top-0.5 size-2 rounded-full bg-red-500" } }
                    }
                    "Twitch"
                }
            }
            if s.stream_enabled {
            Link { to: Route::StreamPage {}, class: item(matches!(route, Route::StreamPage {})),
                span { class: "relative",
                    icons::Radio { class: "size-[22px]" }
                    if n > 0 { span { class: "absolute -right-2.5 -top-1.5 min-w-4 rounded-full bg-secondary px-1 text-center text-[10px] font-semibold leading-4 text-foreground", "{n}" } }
                }
                "Liste"
            }
            }
            Link { to: Route::Playlists {}, class: item(matches!(route, Route::Playlists {} | Route::LocalPlaylistPage { .. })), icons::ListVideo { class: "size-[22px]" } "Playlists" }
            if s.stream_mode {
                Link { to: Route::SettingsPage { tab: String::new() }, class: item(matches!(route, Route::SettingsPage { .. })), icons::Settings { class: "size-[22px]" } "Mehr" }
            } else {
                Link { to: Route::HistoryPage {}, class: item(matches!(route, Route::HistoryPage {} | Route::Subscriptions {})), icons::History { class: "size-[22px]" } "Verlauf" }
            }
        }
    }
}

/// Twitch-Adressen: Kanal, VOD (twitch.tv/videos/…) oder Clip (…/clip/…, clips.twitch.tv/…)
fn twitch_route(text: &str) -> Option<Route> {
    let word = |s: &str| -> String { s.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect() };
    let lower_s = text.to_lowercase();
    let lower = lower_s.as_str();
    // Clip-Kennungen unterscheiden Gross/Klein: aus dem Original schneiden (gleiche Länge vorausgesetzt)
    let orig = if text.len() == lower.len() { text } else { lower };
    if let Some(i) = lower.find("clips.twitch.tv/") {
        let slug = word(&orig[i + 16..]);
        return (!slug.is_empty()).then(|| Route::Watch { v: format!("tw:clip:{slug}"), list: String::new(), t: String::new() });
    }
    let i = lower.find("twitch.tv/")?;
    let rest = &lower[i + 10..];
    if let Some(v) = rest.strip_prefix("videos/") {
        let id: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
        return (!id.is_empty()).then(|| Route::Watch { v: format!("tw:vod:{id}"), list: String::new(), t: String::new() });
    }
    let login = word(rest);
    if login.is_empty() {
        return None;
    }
    if let Some(c) = rest.find("/clip/") {
        let slug = word(&orig[i + 10 + c + 6..]);
        if !slug.is_empty() {
            return Some(Route::Watch { v: format!("tw:clip:{slug}"), list: String::new(), t: String::new() });
        }
    }
    Some(Route::TwitchChannelPage { login, tab: String::new() })
}
