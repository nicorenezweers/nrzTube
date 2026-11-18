//! Wiederverwendbare Bausteine: Video-, Kanal- und Playlist-Karten, Raster, Dialoge, Meldungen.

use crate::Route;
use crate::api;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::util;
use dioxus::prelude::*;
use nrztube_types::*;

pub fn watch_route(id: &str, list: Option<&str>, t: Option<f64>) -> Route {
    Route::Watch {
        v: id.to_string(),
        list: list.unwrap_or("").to_string(),
        t: t.filter(|x| *x > 0.0).map(|x| (x as u64).to_string()).unwrap_or_default(),
    }
}

/// Kanal-Seite zu einer Autor-ID: YouTube (UC…) oder Twitch (tw:<login>)
pub fn channel_route(author_id: &str) -> Route {
    match author_id.strip_prefix("tw:") {
        Some(login) => Route::TwitchChannelPage { login: login.to_string(), tab: String::new() },
        None => Route::ChannelPage { id: author_id.to_string(), tab: String::new() },
    }
}

pub fn watch_href(id: &str, list: Option<&str>) -> String {
    let mut s = format!("/watch?v={id}");
    if let Some(l) = list.filter(|l| !l.is_empty()) {
        s += &format!("&list={l}");
    }
    s
}

// ------------------------------------------------------------------ Kleinkram

#[component]
pub fn Loading(#[props(default)] label: Option<String>) -> Element {
    rsx! {
        div { class: "flex items-center justify-center gap-2 py-16 text-sm text-muted-foreground",
            icons::LoaderCircle { class: "size-5 animate-spin" }
            {label.unwrap_or_else(|| "Lädt …".into())}
        }
    }
}

#[component]
pub fn ErrorBox(message: String, #[props(default)] retry: Option<EventHandler<()>>) -> Element {
    rsx! {
        div { class: "mx-auto my-10 flex max-w-lg flex-col items-center gap-3 rounded-xl border bg-card p-6 text-center",
            icons::CircleAlert { class: "size-8 text-muted-foreground" }
            p { class: "text-sm text-muted-foreground whitespace-pre-line", "{message}" }
            if let Some(r) = retry {
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, onclick: move |_| r.call(()),
                    icons::RefreshCw {} "Nochmal versuchen"
                }
            }
        }
    }
}

#[component]
pub fn EmptyState(title: String, #[props(default)] text: Option<String>, #[props(default)] children: Element) -> Element {
    rsx! {
        div { class: "mx-auto my-16 flex max-w-md flex-col items-center gap-2 text-center",
            icons::SquarePlay { class: "size-10 text-muted-foreground/60" }
            h3 { class: "text-base font-semibold", "{title}" }
            if let Some(t) = text { p { class: "text-sm text-muted-foreground", "{t}" } }
            {children}
        }
    }
}

#[component]
pub fn PageTitle(title: String, #[props(default)] children: Element) -> Element {
    rsx! {
        div { class: "mb-4 flex flex-wrap items-center justify-between gap-3",
            h1 { class: "text-xl font-semibold tracking-tight", "{title}" }
            div { class: "flex flex-wrap items-center gap-2", {children} }
        }
    }
}

/// Umschalter im nrzUI-Stil (gesteuert)
#[component]
pub fn Toggle(checked: bool, onchange: EventHandler<bool>, #[props(default)] label: Option<String>) -> Element {
    let state = if checked { "checked" } else { "unchecked" };
    rsx! {
        button {
            r#type: "button",
            role: "switch",
            aria_checked: "{checked}",
            aria_label: label.unwrap_or_default(),
            "data-state": "{state}",
            class: "inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=checked]:bg-primary data-[state=unchecked]:bg-input",
            onclick: move |_| onchange.call(!checked),
            span {
                "data-state": "{state}",
                class: "block size-5 rounded-full bg-background shadow-lg ring-0 transition-transform pointer-events-none data-[state=checked]:translate-x-5 data-[state=unchecked]:translate-x-0",
            }
        }
    }
}

#[component]
pub fn SettingRow(title: String, #[props(default)] text: Option<String>, children: Element) -> Element {
    rsx! {
        div { class: "flex items-center justify-between gap-4 py-3",
            div { class: "min-w-0",
                div { class: "text-sm font-medium", "{title}" }
                if let Some(t) = text { div { class: "text-xs text-muted-foreground mt-0.5", "{t}" } }
            }
            div { class: "shrink-0", {children} }
        }
    }
}

#[component]
pub fn Select(value: String, options: Vec<(String, String)>, onchange: EventHandler<String>) -> Element {
    rsx! {
        select {
            class: "h-9 rounded-md border border-input bg-input/30 px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
            value: "{value}",
            onchange: move |e| onchange.call(e.value()),
            for (v, l) in options {
                option { value: "{v}", selected: v == value, "{l}" }
            }
        }
    }
}

#[component]
pub fn Modal(title: String, onclose: EventHandler<()>, children: Element, #[props(default)] wide: bool) -> Element {
    let w = if wide { "max-w-2xl" } else { "max-w-md" };
    rsx! {
        div { class: "fixed inset-0 z-[90] flex items-end justify-center bg-black/60 p-0 sm:items-center sm:p-4 animate-in",
            onclick: move |_| onclose.call(()),
            div { class: "w-full {w} max-h-[90vh] overflow-y-auto rounded-t-2xl border bg-popover p-5 shadow-2xl sm:rounded-2xl",
                onclick: move |e| e.stop_propagation(),
                div { class: "mb-4 flex items-center justify-between gap-3",
                    h2 { class: "text-base font-semibold", "{title}" }
                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, aria_label: "Schliessen", onclick: move |_| onclose.call(()),
                        icons::X {}
                    }
                }
                {children}
            }
        }
    }
}

#[component]
pub fn Toaster() -> Element {
    rsx! {
        div { class: "pointer-events-none fixed bottom-4 left-1/2 z-[100] flex w-[min(92vw,420px)] -translate-x-1/2 flex-col gap-2",
            for t in TOASTS() {
                div { key: "{t.id}",
                    class: if t.error { "pointer-events-auto animate-toast-in rounded-xl border border-destructive/40 bg-popover px-4 py-3 text-sm shadow-xl" } else { "pointer-events-auto animate-toast-in rounded-xl border bg-popover px-4 py-3 text-sm shadow-xl" },
                    "{t.text}"
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Karten

#[component]
pub fn Thumb(v: VideoItem, #[props(default)] progress: Option<f64>, #[props(default)] class: Option<String>) -> Element {
    let s = SETTINGS.read();
    let da = DEARROW.read().get(&v.id).cloned().unwrap_or_default();
    let mut failed = use_signal(|| false);
    // Hover-Vorschau: die drei automatischen Standbilder von YouTube im Wechsel
    let mut frame = use_signal(|| 0u8);
    let mut hover_n = use_signal(|| 0u32);
    let animate = !is_external_id(&v.id) && !v.live && !v.upcoming && v.id.len() == 11;
    let src = match (&da.thumb, failed()) {
        _ if frame() > 0 => format!("/vi/{}/hq{}.jpg", v.id, frame()),
        (Some(t), false) if s.dearrow_thumbs => t.clone(),
        _ => v.thumb.clone(),
    };
    let blur = if s.blur_thumbnails { "blur-lg" } else { "" };
    let aspect = if v.short && class.is_none() { "aspect-[9/16]" } else { "aspect-video" };
    let pct = match (progress, v.duration) {
        (Some(p), Some(d)) if d > 0 && p > 0.0 => Some((p / d as f64 * 100.0).min(100.0)),
        _ => None,
    };
    rsx! {
        div { class: "thumb relative overflow-hidden rounded-xl bg-muted {aspect} {class.clone().unwrap_or_default()}",
            onmouseenter: move |_| {
                if !animate { return; }
                let n = *hover_n.peek() + 1;
                hover_n.set(n);
                spawn(async move {
                    // kurz warten, damit schnelles Drüberfahren nichts lädt
                    gloo_timers::future::TimeoutFuture::new(450).await;
                    let mut f = 1u8;
                    while *hover_n.peek() == n {
                        frame.set(f);
                        f = if f >= 3 { 1 } else { f + 1 };
                        gloo_timers::future::TimeoutFuture::new(750).await;
                    }
                });
            },
            onmouseleave: move |_| { let n = *hover_n.peek() + 1; hover_n.set(n); frame.set(0); },
            img {
                class: "absolute inset-0 size-full object-cover transition-transform duration-300 group-hover:scale-[1.03] {blur}",
                src: "{src}",
                loading: "lazy",
                alt: "",
                onerror: move |_| {
                    // Standbild fehlt: Vorschau beenden statt kaputtes Bild
                    if *frame.peek() > 0 { let n = *hover_n.peek() + 1; hover_n.set(n); frame.set(0); } else { failed.set(true); }
                },
            }
            div { class: "absolute bottom-1.5 right-1.5 flex gap-1",
                if v.live {
                    span { class: "flex items-center gap-1 rounded bg-red-600 px-1.5 py-0.5 text-[11px] font-semibold text-white",
                        span { class: "size-1.5 rounded-full bg-white" }
                        "LIVE"
                    }
                } else if v.upcoming {
                    span { class: "rounded bg-black/80 px-1.5 py-0.5 text-[11px] font-medium text-white", "Geplant" }
                } else if let Some(d) = v.duration {
                    span { class: "rounded bg-black/80 px-1.5 py-0.5 text-[11px] font-medium text-white tabular-nums", {util::duration(d as f64)} }
                }
                if v.short {
                    span { class: "rounded bg-black/80 px-1.5 py-0.5 text-[11px] font-medium text-white", "Short" }
                }
            }
            if is_twitch_id(&v.id) {
                div { class: "absolute left-1.5 top-1.5 flex items-center gap-1 rounded bg-black/70 px-1.5 py-0.5 text-[10px] font-medium text-white backdrop-blur-sm",
                    TwitchGlyph { class: "size-3 text-[#a970ff]" }
                    if v.id.starts_with("tw:clip:") { "Clip" } else if v.id.starts_with("tw:vod:") { "VOD" } else { "Twitch" }
                }
                if v.live {
                    if let Some(t) = &v.views_text {
                        span { class: "absolute bottom-1.5 left-1.5 rounded bg-black/70 px-1.5 py-0.5 text-[11px] font-medium text-white tabular-nums backdrop-blur-sm", "{t}" }
                    }
                }
            }
            if let Some(p) = pct {
                div { class: "absolute inset-x-0 bottom-0 h-1 bg-white/25",
                    div { class: "h-full bg-red-600", style: "width: {p}%" }
                }
            }
        }
    }
}

pub fn display_title(v: &VideoItem) -> (String, Option<String>) {
    let s = SETTINGS.read();
    if s.dearrow_titles {
        if let Some(t) = DEARROW.read().get(&v.id).and_then(|d| d.title.clone()) {
            if t != v.title {
                return (t, Some(v.title.clone()));
            }
        }
    }
    (v.title.clone(), None)
}

#[component]
pub fn VideoCard(
    v: VideoItem,
    #[props(default)] list: Option<String>,
    #[props(default)] progress: Option<f64>,
    #[props(default)] actions: Option<Element>,
    #[props(default)] compact: bool,
    /// Warum vorgeschlagen (Startseite)
    #[props(default)] reason: Option<String>,
) -> Element {
    let s = SETTINGS.read().clone();
    // Im Stream-Modus keine Spuren aus dem Verlauf zeigen
    let progress = if s.stream_mode { None } else { progress };
    let (title, original) = display_title(&v);
    let href = watch_href(&v.id, list.as_deref());
    let new_window = s.open_in_new_window;
    let route = watch_route(&v.id, list.as_deref(), None);
    let nav = navigator();
    let mut meta = Vec::new();
    if !s.hide_views {
        if let Some(x) = util::views_label(&v) {
            meta.push(x);
        }
    }
    if !s.hide_upload_date {
        if let Some(x) = util::published_label(&v) {
            meta.push(x);
        }
    }
    let meta = meta.join(" • ");
    let horizontal = compact || s.list_view;
    let v2 = v.clone();
    // Langes Drücken (Handy): Menü statt Öffnen
    let mut press = use_signal(|| 0u32);
    let mut long_pressed = use_signal(|| false);
    let open = move |e: MouseEvent| {
        let m = e.modifiers();
        if m.ctrl() || m.meta() || m.shift() {
            return;
        }
        e.prevent_default();
        if *long_pressed.peek() {
            long_pressed.set(false);
            return;
        }
        if new_window {
            util::open_window(&watch_href(&v2.id, None), true);
        } else {
            *PENDING.write() = Some(v2.clone());
            nav.push(route.clone());
        }
    };
    let open2 = open.clone();
    let v_ctx = v.clone();
    let v_more = v.clone();
    let v_stream = v.clone();
    let pre_id = v.id.clone();
    let mut hover_timer = use_signal(|| 0u32);
    let title_attr = original.clone().map(|o| format!("Original: {o}")).unwrap_or_else(|| title.clone());
    let wrap = if horizontal { "group flex gap-3" } else { "group flex flex-col gap-2" };
    let thumb_class = if horizontal {
        Some(if compact { "w-40 shrink-0 sm:w-44".to_string() } else { "w-48 shrink-0 sm:w-64".to_string() })
    } else {
        None
    };
    rsx! {
        div { class: "{wrap} relative min-w-0",
            style: "-webkit-touch-callout: none",
            oncontextmenu: move |e: MouseEvent| {
                e.prevent_default();
                // Schon über langes Drücken geöffnet
                if CTX.peek().is_some() { return; }
                let c = e.client_coordinates();
                *CTX.write() = Some((v_ctx.clone(), c.x, c.y));
            },
            // Kurz mit der Maus darauf: Streams schon holen, dann startet das Video beim Klick sofort
            onmouseenter: move |_| {
                let n = *hover_timer.peek() + 1;
                hover_timer.set(n);
                let id = pre_id.clone();
                spawn(async move {
                    gloo_timers::future::TimeoutFuture::new(180).await;
                    if *hover_timer.peek() == n { prefetch(vec![id]); }
                });
            },
            onmouseleave: move |_| { let n = *hover_timer.peek() + 1; hover_timer.set(n); },
            ontouchstart: {
                let id = v.id.clone();
                let v_press = v.clone();
                move |e: TouchEvent| {
                    prefetch(vec![id.clone()]);
                    let (x, y) = e.touches().first().map(|t| (t.client_coordinates().x, t.client_coordinates().y)).unwrap_or((0.0, 0.0));
                    let n = *press.peek() + 1;
                    press.set(n);
                    long_pressed.set(false);
                    let vp = v_press.clone();
                    spawn(async move {
                        gloo_timers::future::TimeoutFuture::new(480).await;
                        if *press.peek() == n {
                            long_pressed.set(true);
                            util::eval("navigator.vibrate && navigator.vibrate(12)");
                            *CTX.write() = Some((vp, x, y));
                        }
                    });
                }
            },
            ontouchmove: move |_| { let n = *press.peek() + 1; press.set(n); },
            ontouchend: move |_| { let n = *press.peek() + 1; press.set(n); },
            a { href: "{href}", onclick: open, class: "block min-w-0 {thumb_class.clone().unwrap_or_default()}",
                Thumb { v: v.clone(), progress, class: thumb_class.clone().map(|c| format!("{c} w-full")) }
            }
            div { class: "flex min-w-0 flex-1 gap-2.5",
                if !horizontal && !s.hide_channel_avatars && v.author_thumb.is_some() {
                    Link { to: channel_route(&v.author_id), class: "mt-0.5 shrink-0",
                        img { class: "size-8 rounded-full bg-muted object-cover", src: v.author_thumb.clone().unwrap_or_default(), loading: "lazy", alt: "" }
                    }
                }
                div { class: "min-w-0 flex-1",
                    a { href: "{href}", onclick: open2, title: "{title_attr}",
                        class: if compact { "line-clamp-2 text-sm font-medium leading-snug" } else { "line-clamp-2 text-[15px] font-medium leading-snug" },
                        "{title}"
                    }
                    if !v.author.is_empty() {
                        if v.author_id.is_empty() {
                            div { class: "mt-1 truncate text-xs text-muted-foreground", "{v.author}" }
                        } else {
                            Link { to: channel_route(&v.author_id),
                                class: "mt-1 flex items-center gap-1 truncate text-xs text-muted-foreground hover:text-foreground",
                                if is_twitch_id(&v.author_id) { TwitchGlyph { class: "size-3 shrink-0 text-[#a970ff]" } }
                                "{v.author}"
                                if v.verified { icons::Check { class: "size-3" } }
                            }
                        }
                    }
                    if !meta.is_empty() {
                        div { class: "truncate text-xs text-muted-foreground", "{meta}" }
                    }
                    if let Some(r) = reason.filter(|r| !r.is_empty()) {
                        div { class: "mt-1 flex items-center gap-1 truncate text-[11px] text-muted-foreground/80",
                            icons::Sparkles { class: "size-3 shrink-0" }
                            span { class: "truncate", "{r}" }
                        }
                    }
                    if horizontal && !compact {
                        if let Some(d) = &v.description {
                            p { class: "mt-1 line-clamp-2 text-xs text-muted-foreground", "{d}" }
                        }
                    }
                }
                div { class: "flex shrink-0 flex-col items-center gap-1 opacity-100 md:opacity-0 md:group-hover:opacity-100 transition-opacity",
                    if let Some(a) = actions { {a} }
                    if s.stream_enabled {
                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconXs, class: "hidden md:inline-flex", title: "Zur Stream-Liste",
                        onclick: move |_| add_to_stream(v_stream.clone()),
                        icons::Radio { class: "size-3.5" }
                    }
                    }
                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconXs, title: "Mehr",
                        onclick: move |e: MouseEvent| {
                            let c = e.client_coordinates();
                            *CTX.write() = Some((v_more.clone(), c.x, c.y));
                        },
                        icons::EllipsisVertical { class: "size-4" }
                    }
                }
            }
        }
    }
}

#[component]
pub fn VideoGrid(
    videos: Vec<VideoItem>,
    #[props(default)] list: Option<String>,
) -> Element {
    let list_view = SETTINGS.read().list_view;
    let grid = if list_view { "flex flex-col gap-4" } else { "edge-grid grid grid-cols-1 gap-x-4 gap-y-6 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4 md:gap-y-7" };
    let ids: Vec<String> = videos.iter().map(|v| v.id.clone()).collect();
    use_effect(use_reactive!(|ids| load_dearrow(ids)));
    let (shorts, normal): (Vec<VideoItem>, Vec<VideoItem>) = if list_view { (vec![], videos) } else { videos.into_iter().partition(|v| v.short) };
    rsx! {
        div { class: "{grid}",
            for v in normal {
                VideoCard { key: "{v.id}", v: v.clone(), list: list.clone() }
            }
        }
        if !shorts.is_empty() {
            h2 { class: "mb-3 mt-8 flex items-center gap-2 text-base font-semibold", icons::Flame { class: "size-4" } "Shorts" }
            div { class: "grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6",
                for v in shorts {
                    VideoCard { key: "{v.id}", v: v.clone() }
                }
            }
        }
    }
}

#[component]
pub fn ChannelCard(c: ChannelItem) -> Element {
    let s = SETTINGS.read().clone();
    let sub = is_subscribed(&c.id);
    let (id, name, thumb) = (c.id.clone(), c.name.clone(), c.thumb.clone());
    rsx! {
        div { class: "flex items-center gap-4 rounded-xl border bg-card p-4",
            Link { to: Route::ChannelPage { id: c.id.clone(), tab: String::new() }, class: "shrink-0",
                img { class: "size-16 rounded-full bg-muted object-cover sm:size-20", src: "{c.thumb}", alt: "", loading: "lazy" }
            }
            div { class: "min-w-0 flex-1",
                Link { to: Route::ChannelPage { id: c.id.clone(), tab: String::new() }, class: "flex items-center gap-1 font-semibold hover:underline",
                    "{c.name}"
                    if c.verified { icons::Check { class: "size-3.5" } }
                }
                div { class: "text-xs text-muted-foreground",
                    {[c.handle.clone(), if s.hide_subscriber_counts { None } else { c.subscribers.clone() }, c.video_count.clone()].into_iter().flatten().collect::<Vec<_>>().join(" • ")}
                }
                if let Some(d) = &c.description { p { class: "mt-1 line-clamp-2 text-xs text-muted-foreground", "{d}" } }
            }
            Button { variant: if sub { ButtonVariant::Secondary } else { ButtonVariant::Default }, size: ButtonSize::Sm,
                onclick: move |_| { let (a, b, c) = (id.clone(), name.clone(), thumb.clone()); spawn(async move { toggle_subscription(a, b, c).await; }); },
                if sub { "Abonniert" } else { "Abonnieren" }
            }
        }
    }
}

#[component]
pub fn PlaylistCard(p: PlaylistItem) -> Element {
    rsx! {
        Link { to: Route::YtPlaylist { list: p.id.clone() }, class: "group flex flex-col gap-2",
            div { class: "relative aspect-video overflow-hidden rounded-xl bg-muted",
                img { class: "absolute inset-0 size-full object-cover", src: "{p.thumb}", alt: "", loading: "lazy" }
                div { class: "absolute inset-y-0 right-0 flex w-2/5 flex-col items-center justify-center gap-1 bg-black/70 text-white",
                    icons::ListVideo { class: "size-5" }
                    span { class: "text-xs font-medium", {p.video_count.clone().unwrap_or_default()} }
                }
            }
            div {
                div { class: "line-clamp-2 text-[15px] font-medium leading-snug", "{p.title}" }
                if !p.author.is_empty() { div { class: "text-xs text-muted-foreground", "{p.author}" } }
            }
        }
    }
}

#[component]
pub fn ItemGrid(items: Vec<Item>) -> Element {
    let ids: Vec<String> = items.iter().filter_map(|i| if let Item::Video(v) = i { Some(v.id.clone()) } else { None }).collect();
    use_effect(use_reactive!(|ids| load_dearrow(ids)));
    let channels: Vec<ChannelItem> = items.iter().filter_map(|i| if let Item::Channel(c) = i { Some(c.clone()) } else { None }).collect();
    let playlists: Vec<PlaylistItem> = items.iter().filter_map(|i| if let Item::Playlist(p) = i { Some(p.clone()) } else { None }).collect();
    let videos: Vec<VideoItem> = items.into_iter().filter_map(|i| if let Item::Video(v) = i { Some(v) } else { None }).collect();
    rsx! {
        if !channels.is_empty() {
            div { class: "mb-6 grid gap-3 lg:grid-cols-2",
                for c in channels { ChannelCard { key: "{c.id}", c: c.clone() } }
            }
        }
        VideoGrid { videos }
        if !playlists.is_empty() {
            h2 { class: "mb-3 mt-8 text-base font-semibold", "Playlists" }
            div { class: "grid grid-cols-1 gap-x-4 gap-y-6 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4",
                for p in playlists { PlaylistCard { key: "{p.id}", p: p.clone() } }
            }
        }
    }
}

#[component]
pub fn LoadMore(loading: bool, onclick: EventHandler<()>) -> Element {
    rsx! {
        div { class: "flex justify-center py-8",
            Button { variant: ButtonVariant::Outline, disabled: loading, onclick: move |_| onclick.call(()),
                if loading { icons::LoaderCircle { class: "animate-spin" } } else { icons::ChevronDown {} }
                "Mehr laden"
            }
        }
    }
}

/// Text mit anklickbaren Links und Zeitmarken (Beschreibung, Kommentare, Beiträge)
#[component]
pub fn RichText(text: String, #[props(default)] class: Option<String>) -> Element {
    let mut parts: Vec<(String, Option<String>, Option<f64>)> = Vec::new();
    for (li, line) in text.split('\n').enumerate() {
        if li > 0 {
            parts.push(("\n".into(), None, None));
        }
        for (wi, word) in line.split(' ').enumerate() {
            if wi > 0 {
                parts.push((" ".into(), None, None));
            }
            if word.starts_with("http://") || word.starts_with("https://") {
                parts.push((word.to_string(), Some(word.to_string()), None));
                continue;
            }
            let ts = util::timestamps(word);
            if let Some((a, b, secs)) = ts.first().copied() {
                if a > 0 {
                    parts.push((word[..a].to_string(), None, None));
                }
                parts.push((word[a..b].to_string(), None, Some(secs)));
                if b < word.len() {
                    parts.push((word[b..].to_string(), None, None));
                }
                continue;
            }
            parts.push((word.to_string(), None, None));
        }
    }
    let class = class.unwrap_or_default();
    rsx! {
        div { class: "whitespace-pre-wrap break-words {class}",
            for (txt, link, ts) in parts {
                if let Some(l) = link {
                    { link_el(txt, l) }
                } else if let Some(t) = ts {
                    button { class: "text-sky-300 hover:underline", onclick: move |_| *SEEK.write() = Some(t), "{txt}" }
                } else {
                    "{txt}"
                }
            }
        }
    }
}

/// YouTube-Links bleiben in nrzTube, alles andere öffnet extern
fn link_el(txt: String, url: String) -> Element {
    let internal = url.contains("youtube.com/") || url.contains("youtu.be/");
    if internal {
        let u = url.clone();
        let nav = navigator();
        rsx! {
            a { href: "{url}", class: "text-sky-300 hover:underline",
                onclick: move |e| {
                    e.prevent_default();
                    let u = u.clone();
                    spawn(async move {
                        if let Ok(r) = api::get::<serde_json::Value>(&format!("/api/resolve?q={}", api::enc(&u))).await {
                            let id = r.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                            match r.get("kind").and_then(|x| x.as_str()) {
                                Some("video") => { let t = r.get("t").and_then(|x| x.as_str()).and_then(|x| x.trim_end_matches('s').parse().ok()); nav.push(watch_route(&id, r.get("list").and_then(|x| x.as_str()), t)); }
                                Some("channel") => { nav.push(Route::ChannelPage { id, tab: String::new() }); }
                                Some("playlist") => { nav.push(Route::YtPlaylist { list: id }); }
                                _ => util::open_window(&u, false),
                            }
                        }
                    });
                },
                "{txt}"
            }
        }
    } else {
        rsx! { a { href: "{url}", target: "_blank", rel: "noopener noreferrer", class: "text-sky-300 hover:underline", "{txt}" } }
    }
}

// ------------------------------------------------------------------ Dialoge

#[component]
pub fn AddToPlaylistDialog() -> Element {
    match ADD_TO() {
        Some(video) => rsx! { AddToPlaylistInner { key: "{video.id}", video } },
        None => rsx! {},
    }
}

#[component]
fn AddToPlaylistInner(video: VideoItem) -> Element {
    let mut containing = use_signal(Vec::<i64>::new);
    let mut new_title = use_signal(String::new);
    let vid = video.id.clone();
    use_effect(use_reactive!(|vid| {
        spawn(async move {
            if let Ok(ids) = api::get::<Vec<i64>>(&format!("/api/playlists/containing/{vid}")).await {
                containing.set(ids);
            }
        });
    }));
    let video2 = video.clone();
    let create = move |_| {
        let title = new_title().trim().to_string();
        if title.is_empty() {
            return;
        }
        let v = video2.clone();
        spawn(async move {
            match api::post::<_, serde_json::Value>("/api/playlists", &serde_json::json!({ "title": title })).await {
                Ok(r) => {
                    if let Some(id) = r.get("id").and_then(|x| x.as_i64()) {
                        let _ = api::post::<_, serde_json::Value>(&format!("/api/playlists/{id}/videos"), &v).await;
                        containing.write().push(id);
                        toast(format!("Zu „{title}“ hinzugefügt"));
                    }
                    new_title.set(String::new());
                    reload_playlists().await;
                }
                Err(e) => toast_err(e),
            }
        });
    };
    rsx! {
        Modal { title: "Zu Playlist hinzufügen", onclose: move |_| *ADD_TO.write() = None,
            p { class: "mb-3 line-clamp-2 text-sm text-muted-foreground", "{video.title}" }
            div { class: "flex flex-col gap-1",
                for p in PLAYLISTS() {
                    {
                        let inside = containing().contains(&p.id);
                        let v = video.clone();
                        let pid = p.id;
                        let title = p.title.clone();
                        rsx! {
                            button { key: "{p.id}", class: "flex items-center gap-3 rounded-lg px-3 py-2 text-left text-sm hover:bg-accent",
                                onclick: move |_| {
                                    let v = v.clone();
                                    let title = title.clone();
                                    spawn(async move {
                                        let res = if inside {
                                            api::delete::<serde_json::Value>(&format!("/api/playlists/{pid}/videos/{}", v.id)).await
                                        } else {
                                            api::post::<_, serde_json::Value>(&format!("/api/playlists/{pid}/videos"), &v).await
                                        };
                                        match res {
                                            Ok(_) => {
                                                if inside { containing.write().retain(|x| *x != pid); toast(format!("Aus „{title}“ entfernt")); }
                                                else { containing.write().push(pid); toast(format!("Zu „{title}“ hinzugefügt")); }
                                                reload_playlists().await;
                                            }
                                            Err(e) => toast_err(e),
                                        }
                                    });
                                },
                                span { class: if inside { "flex size-5 items-center justify-center rounded border bg-primary text-primary-foreground" } else { "flex size-5 items-center justify-center rounded border" },
                                    if inside { icons::Check { class: "size-3.5" } }
                                }
                                span { class: "flex-1 truncate", "{p.title}" }
                                span { class: "text-xs text-muted-foreground", "{p.count}" }
                            }
                        }
                    }
                }
            }
            div { class: "mt-4 flex gap-2",
                input { class: "h-9 flex-1 rounded-md border border-input bg-input/30 px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
                    placeholder: "Neue Playlist …", value: "{new_title}",
                    oninput: move |e| new_title.set(e.value()),
                }
                Button { size: ButtonSize::Sm, onclick: create, icons::Plus {} "Anlegen" }
            }
        }
    }
}

pub const SHORTCUT_LIST: &[(&str, &str)] = &[
    ("Leertaste / K", "Abspielen / Pause"),
    ("J / L", "10 Sekunden zurück / vor"),
    ("← / →", "5 Sekunden zurück / vor"),
    ("↑ / ↓", "Lauter / leiser"),
    ("M", "Ton aus / an"),
    ("F", "Vollbild"),
    ("T", "Kinomodus"),
    ("I", "Bild-im-Bild (Mini-Player)"),
    ("C", "Untertitel an / aus"),
    ("S", "Bildschirmfoto"),
    ("< / >", "Langsamer / schneller"),
    (", / .", "Ein Bild zurück / vor (pausiert)"),
    ("0 – 9", "Zu 0 % – 90 % springen"),
    ("Pos1 / Ende", "Anfang / Ende"),
    ("N / P", "Nächstes / vorheriges Video"),
    ("Enter", "SponsorBlock-Segment überspringen"),
    ("/", "Suche"),
    ("?", "Diese Übersicht"),
];

#[component]
pub fn ShortcutsDialog() -> Element {
    if !SHORTCUTS() {
        return rsx! {};
    }
    rsx! {
        Modal { title: "Tastenkürzel", onclose: move |_| *SHORTCUTS.write() = false, wide: true,
            div { class: "grid gap-x-8 gap-y-2 sm:grid-cols-2",
                for (k, d) in SHORTCUT_LIST.iter() {
                    div { class: "flex items-center justify-between gap-4 text-sm",
                        span { class: "text-muted-foreground", "{d}" }
                        kbd { class: "rounded border bg-muted px-1.5 py-0.5 font-mono text-xs", "{k}" }
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Kontextmenü

/// Menü eines Videos: am Mauszeiger (Computer) oder als Blatt von unten (Handy)
#[component]
pub fn VideoMenu() -> Element {
    // Klick direkt nach langem Drücken (Finger loslassen) schliesst das Menü nicht gleich wieder
    let mut opened_at = use_signal(|| 0.0f64);
    let is_open = CTX().is_some();
    use_effect(use_reactive!(|is_open| { if is_open { opened_at.set(js_sys::Date::now()); } }));
    let Some((v, x, y)) = CTX() else { return rsx! {} };
    let win = web_sys::window();
    let (w, h) = win
        .as_ref()
        .map(|w| (w.inner_width().ok().and_then(|x| x.as_f64()).unwrap_or(1200.0), w.inner_height().ok().and_then(|x| x.as_f64()).unwrap_or(800.0)))
        .unwrap_or((1200.0, 800.0));
    let mobile = w < 768.0;
    let (mw, mh) = (264.0, 420.0);
    let left = x.min(w - mw - 8.0).max(8.0);
    let top = if y + mh > h { (y - mh).max(8.0) } else { y };
    let rating = RATINGS().get(&v.id).copied().unwrap_or(0);
    let close = move || *CTX.write() = None;
    let (title, _) = display_title(&v);
    let item = "flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left text-sm hover:bg-accent md:py-2";
    let (v1, v2, v3, v4, v5) = (v.clone(), v.clone(), v.clone(), v.clone(), v.clone());
    let id_win = v.id.clone();
    let id_copy = v.id.clone();
    let channel = (v.author_id.clone(), v.author.clone());
    let channel_login = v.author_id.trim_start_matches("tw:").to_string();
    let twitch_game = if is_twitch_id(&v.id) { v.description.clone().filter(|g| !g.is_empty()) } else { None };
    let body = rsx! {
        if SETTINGS().stream_enabled {
        button { class: item, onclick: move |_| { add_to_stream(v1.clone()); close(); },
            icons::Radio { class: "size-4" } "Zur Stream-Liste"
        }
        }
        button { class: item, onclick: move |_| { add_to_watch_later(v2.clone()); close(); },
            icons::Clock { class: "size-4" } "Später ansehen"
        }
        button { class: item, onclick: move |_| { *ADD_TO.write() = Some(v3.clone()); close(); },
            icons::ListPlus { class: "size-4" } "Zu Playlist hinzufügen …"
        }
        {
            let vp = v4.clone();
            let in_party = crate::pages::party::PARTY().is_some();
            rsx! {
                button { class: item, onclick: move |_| {
                        if in_party { crate::pages::party::enqueue(vp.clone()); } else { crate::pages::party::start_party(Some(vp.clone())); }
                        close();
                    },
                    icons::Users { class: "size-4" } if in_party { "In Watch-Party einreihen" } else { "Zusammen schauen …" }
                }
            }
        }
        div { class: "my-1 h-px bg-border" }
        button { class: item, onclick: move |_| { rate(v4.clone(), if rating == 1 { 0 } else { 1 }); close(); },
            span { class: if rating == 1 { "flex [&_svg]:fill-current" } else { "flex" }, icons::ThumbsUp { class: "size-4" } }
            if rating == 1 { "Gefällt mir nicht mehr" } else { "Gefällt mir (Favorit)" }
        }
        button { class: item, onclick: move |_| { rate(v5.clone(), -1); close(); },
            icons::Ban { class: "size-4" } "Nicht interessiert"
        }
        if !channel.0.is_empty() {
            button { class: item, onclick: move |_| {
                    let (cid, name) = channel.clone();
                    update_settings(|s| { if !s.hidden_channels.contains(&cid) { s.hidden_channels.push(cid.clone()); } });
                    toast(format!("{name} wird nicht mehr vorgeschlagen"));
                    close();
                },
                icons::CircleMinus { class: "size-4" } if is_twitch_id(&channel.0) { "Kanal ausblenden" } else { "Kanal nicht empfehlen" }
            }
        }
        if let Some(game) = twitch_game.clone() {
            button { class: item, onclick: move |_| {
                    let g = game.clone();
                    update_settings(|s| { if !s.twitch_hidden_games.iter().any(|x| x.eq_ignore_ascii_case(&g)) { s.twitch_hidden_games.push(g.clone()); } });
                    toast(format!("Kategorie „{game}“ ausgeblendet"));
                    spawn(async { reload_tw_live().await; });
                    close();
                },
                icons::EyeOff { class: "size-4" } "Kategorie „{game}“ ausblenden"
            }
        }
        div { class: "my-1 h-px bg-border" }
        button { class: item, onclick: move |_| { util::open_window(&watch_href(&id_win, None), true); close(); },
            icons::AppWindow { class: "size-4" } "In neuem Fenster öffnen"
        }
        button { class: item, onclick: move |_| { util::copy(&share_link(&id_copy, &channel_login)); toast("Link kopiert"); close(); },
            icons::Link { class: "size-4" } "Link kopieren"
        }
    };
    rsx! {
        div { class: "fixed inset-0 z-[70]", onclick: move |_| { if js_sys::Date::now() - *opened_at.peek() > 400.0 { close() } }, oncontextmenu: move |e| { e.prevent_default(); close(); },
            if mobile { div { class: "absolute inset-0 bg-black/50 animate-[enter_150ms_ease-out]" } }
        }
        if mobile {
            div { class: "fixed inset-x-0 bottom-0 z-[71] max-h-[85dvh] overflow-y-auto rounded-t-2xl border-t bg-popover p-2 pb-safe shadow-2xl animate-[up_200ms_ease-out]",
                div { class: "mx-auto mb-2 mt-1 h-1 w-10 rounded-full bg-muted-foreground/40" }
                div { class: "flex items-center gap-3 px-3 pb-3",
                    img { class: "aspect-video w-24 shrink-0 rounded-md bg-muted object-cover", src: "{v.thumb}", alt: "" }
                    div { class: "min-w-0",
                        div { class: "line-clamp-2 text-sm font-medium", "{title}" }
                        div { class: "truncate text-xs text-muted-foreground", "{v.author}" }
                    }
                }
                {body}
            }
        } else {
            div { class: "fixed z-[71] w-[264px] rounded-xl border bg-popover p-1.5 shadow-2xl animate-[enter_120ms_ease-out]",
                style: "left: {left}px; top: {top}px",
                {body}
            }
        }
    }
}


// ------------------------------------------------------------------ Twitch

/// Twitch-Logo (vereinfachte Sprechblase)
#[component]
pub fn TwitchGlyph(#[props(default)] class: Option<String>) -> Element {
    rsx! {
        svg { class: class.unwrap_or_else(|| "size-4".into()), view_box: "0 0 24 24", fill: "currentColor", "aria-hidden": "true",
            path { d: "M4.3 3 3 6.4v13.1h4.5V22h2.6l2.4-2.5h3.6l4.9-4.9V3H4.3Zm1.6 1.6h13.4v9.2l-2.9 2.9h-4.6L9.4 19.1v-2.4H5.9V4.6Zm4.5 2.9v5.1H12V7.5h-1.6Zm4.4 0v5.1h1.6V7.5h-1.6Z" }
        }
    }
}

/// "Jetzt live auf Twitch": waagrecht scrollbare Leiste
#[component]
pub fn LiveRow() -> Element {
    let s = SETTINGS();
    // YouTube-Abos zuerst, dann Twitch
    let mut live = YT_LIVE();
    if s.twitch_enabled {
        live.extend(TW_LIVE());
    }
    if !s.twitch_live_row || live.is_empty() || s.stream_mode {
        return rsx! {};
    }
    rsx! {
        section { class: "mb-6",
            div { class: "mb-3 flex items-center gap-2",
                span { class: "relative flex size-2",
                    span { class: "absolute inline-flex size-full animate-ping rounded-full bg-red-500 opacity-60" }
                    span { class: "relative inline-flex size-2 rounded-full bg-red-500" }
                }
                h2 { class: "text-base font-semibold", "Jetzt live" }
                span { class: "text-sm text-muted-foreground", "{live.len()}" }
                if s.twitch_enabled {
                    Link { to: Route::TwitchHome { game: String::new() }, class: "ml-auto text-sm text-muted-foreground hover:text-foreground", "Alle Twitch-Streams" }
                }
            }
            div { class: "shelf -mx-3 flex snap-x gap-4 overflow-x-auto px-3 pb-1 md:-mx-0 md:px-0",
                for v in live {
                    div { key: "{v.id}", class: "w-[78vw] shrink-0 snap-start sm:w-72 xl:w-80",
                        VideoCard { v: v.clone() }
                    }
                }
            }
        }
    }
}

/// Verknüpfte Kanäle der anderen Plattform als kleine Karten
#[component]
pub fn LinkedChannels(links: Vec<ChannelLink>, to_twitch: bool, #[props(default)] on_unlink: Option<EventHandler<ChannelLink>>) -> Element {
    if links.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "flex flex-wrap gap-2",
            for l in links {
                {
                    let (route, name, thumb) = if to_twitch {
                        (Route::TwitchChannelPage { login: l.tw_login.clone(), tab: String::new() }, if l.tw_name.is_empty() { l.tw_login.clone() } else { l.tw_name.clone() }, l.tw_thumb.clone())
                    } else {
                        (Route::ChannelPage { id: l.yt_id.clone(), tab: String::new() }, if l.yt_name.is_empty() { l.yt_id.clone() } else { l.yt_name.clone() }, l.yt_thumb.clone())
                    };
                    let live = to_twitch && l.live.is_some();
                    let l2 = l.clone();
                    rsx! {
                        div { key: "{l.yt_id}{l.tw_login}", class: "group/link flex items-center gap-2 rounded-full border bg-card py-1 pl-1 pr-3 text-sm transition-colors hover:bg-accent",
                            title: "{l.source_label()}",
                            Link { to: route, class: "flex items-center gap-2",
                                span { class: "relative",
                                    if thumb.is_empty() {
                                        span { class: "flex size-7 items-center justify-center rounded-full bg-muted",
                                            if to_twitch { TwitchGlyph { class: "size-3.5 text-[#a970ff]" } } else { icons::Youtube { class: "size-3.5" } }
                                        }
                                    } else {
                                        img { class: if live { "size-7 rounded-full object-cover ring-2 ring-red-500" } else { "size-7 rounded-full object-cover" }, src: "{thumb}", alt: "" }
                                    }
                                }
                                if to_twitch { TwitchGlyph { class: "size-3.5 text-[#a970ff]" } } else { icons::Youtube { class: "size-3.5 text-red-500" } }
                                span { class: "font-medium", "{name}" }
                                if live { span { class: "rounded bg-red-600 px-1 text-[10px] font-bold text-white", "LIVE" } }
                            }
                            if let Some(h) = on_unlink {
                                button { class: "ml-1 hidden rounded-full p-0.5 text-muted-foreground hover:text-foreground group-hover/link:block", title: "Verknüpfung lösen",
                                    onclick: move |_| h.call(l2.clone()),
                                    icons::X { class: "size-3" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Teilen-Adresse: YouTube oder Twitch (Live, VOD, Clip)
pub fn share_link(id: &str, login: &str) -> String {
    if let Some(l) = id.strip_prefix("tw:live:") {
        return format!("https://www.twitch.tv/{l}");
    }
    if let Some(v) = id.strip_prefix("tw:vod:") {
        return format!("https://www.twitch.tv/videos/{v}");
    }
    if let Some(c) = id.strip_prefix("tw:clip:") {
        return format!("https://www.twitch.tv/{login}/clip/{c}");
    }
    format!("https://youtu.be/{id}")
}
