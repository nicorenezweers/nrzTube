//! Social: TikTok, Instagram und X an einem Ort – gemischter Feed der gefolgten Konten mit
//! Plattform-Reitern, Profile, einzelne Beiträge und die Erkennung eingefügter Links.

use crate::Route;
use crate::api;
use crate::components::*;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::util;
use dioxus::prelude::*;
use nrztube_types::*;

pub fn platform_name(p: &str) -> &'static str {
    match p {
        "tiktok" => "TikTok",
        "instagram" => "Instagram",
        "x" => "X",
        _ => "Social",
    }
}

/// Akzentfarbe der Plattform (Text)
fn accent(p: &str) -> &'static str {
    match p {
        "tiktok" => "text-[#ff2d6f]",
        "instagram" => "text-[#e1306c]",
        _ => "text-foreground",
    }
}

fn accent_bg(p: &str) -> &'static str {
    match p {
        "tiktok" => "bg-[#ff2d6f]/15",
        "instagram" => "bg-[#e1306c]/15",
        _ => "bg-foreground/10",
    }
}

/// Plattformen, die in den Einstellungen eingeschaltet sind
pub fn enabled_platforms() -> Vec<&'static str> {
    let s = SETTINGS();
    SOCIAL_PLATFORMS.iter().copied().filter(|p| s.social_on(p)).collect()
}

/// Logo der Plattform (vereinfacht)
#[component]
pub fn PlatformGlyph(platform: String, #[props(default)] class: Option<String>) -> Element {
    let class = class.unwrap_or_else(|| "size-4".into());
    match platform.as_str() {
        "tiktok" => rsx! {
            svg { class, view_box: "0 0 24 24", fill: "currentColor", "aria-hidden": "true",
                path { d: "M16.6 5.82A4.28 4.28 0 0 1 15.54 3h-3.09v12.4a2.59 2.59 0 0 1-2.59 2.5c-1.42 0-2.6-1.16-2.6-2.6 0-1.72 1.66-3.01 3.37-2.48V9.66c-3.45-.46-6.47 2.22-6.47 5.64 0 3.33 2.76 5.7 5.69 5.7 3.14 0 5.69-2.55 5.69-5.7V9.01a7.35 7.35 0 0 0 4.3 1.38V7.3s-1.88.09-3.24-1.48Z" }
            }
        },
        "instagram" => rsx! {
            svg { class, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", "aria-hidden": "true",
                rect { x: "3", y: "3", width: "18", height: "18", rx: "5" }
                circle { cx: "12", cy: "12", r: "4" }
                circle { cx: "17.5", cy: "6.5", r: "0.6", fill: "currentColor" }
            }
        },
        "x" => rsx! {
            svg { class, view_box: "0 0 24 24", fill: "currentColor", "aria-hidden": "true",
                path { d: "M17.75 3h3.07l-6.7 7.66L22 21h-6.17l-4.83-6.32L5.47 21H2.4l7.17-8.2L2 3h6.33l4.37 5.77L17.75 3Zm-1.08 16.17h1.7L7.4 4.73H5.58l11.09 14.44Z" }
            }
        },
        _ => rsx! { icons::Sparkles { class } },
    }
}

/// Eingefügte Links und @Namen erkennen: Profil oder Beitrag auf TikTok, Instagram oder X
pub fn social_route(text: &str) -> Option<Route> {
    let t = text.trim();
    let lower = t.to_lowercase();
    let word = |s: &str| -> String { s.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.' || *c == '-').collect() };
    // Gross/Klein zählt bei Instagram-Shortcodes: aus dem Original schneiden
    let orig = if t.len() == lower.len() { t } else { lower.as_str() };
    let after = |host: &str| -> Option<(String, String)> {
        let i = lower.find(host)?;
        let start = i + host.len();
        Some((lower[start..].to_string(), orig[start..].to_string()))
    };
    if let Some((rest, _)) = after("tiktok.com/@") {
        let h = word(&rest);
        if let Some(v) = rest.find("/video/").or_else(|| rest.find("/photo/")) {
            let id: String = rest[v + 7..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if !id.is_empty() {
                return Some(Route::SocialPostPage { platform: "tiktok".into(), id, h });
            }
        }
        return (!h.is_empty()).then(|| Route::SocialProfilePage { platform: "tiktok".into(), handle: h });
    }
    if let Some((rest, orig_rest)) = after("instagram.com/") {
        for kind in ["p/", "reel/", "reels/", "tv/"] {
            if rest.starts_with(kind) {
                let code = word(&orig_rest[kind.len()..]).replace('.', "");
                return (!code.is_empty()).then(|| Route::SocialPostPage { platform: "instagram".into(), id: code, h: String::new() });
            }
        }
        let h = word(&rest);
        let reserved = ["explore", "stories", "accounts", "direct", "about", "developer", "legal"];
        return (!h.is_empty() && !reserved.contains(&h.as_str())).then(|| Route::SocialProfilePage { platform: "instagram".into(), handle: h });
    }
    let x = after("://x.com/").or_else(|| after("://twitter.com/")).or_else(|| after("://mobile.twitter.com/")).or_else(|| if lower.starts_with("x.com/") { Some((lower[6..].to_string(), String::new())) } else { None });
    if let Some((rest, _)) = x {
        let h = word(&rest).replace('.', "");
        if let Some(sx) = rest.find("/status/") {
            let id: String = rest[sx + 8..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if !id.is_empty() {
                return Some(Route::SocialPostPage { platform: "x".into(), id, h });
            }
        }
        let reserved = ["home", "explore", "search", "i", "settings", "notifications", "messages"];
        return (!h.is_empty() && !reserved.contains(&h.as_str())).then(|| Route::SocialProfilePage { platform: "x".into(), handle: h });
    }
    None
}

// ------------------------------------------------------------------ Übersicht mit Feed

#[component]
pub fn SocialHome(p: String) -> Element {
    let platforms = enabled_platforms();
    let current = if platforms.contains(&p.as_str()) { p.clone() } else { String::new() };
    let mut fresh = use_signal(|| false);
    let cur = current.clone();
    let mut feed = use_resource(use_reactive!(|cur| async move {
        let f = *fresh.peek();
        api::get::<SocialFeed>(&format!("/api/social/feed?platform={}&fresh={f}", api::enc(&cur))).await
    }));
    let status = use_resource(|| async { api::get::<Vec<SocialStatus>>("/api/social/status").await.unwrap_or_default() });
    let mut shown = use_signal(|| 30usize);
    let cur2 = current.clone();
    use_effect(use_reactive!(|cur2| { let _ = cur2; shown.set(30); }));
    use_hook(|| { spawn(async { reload_social_follows().await; }); });
    let nav = navigator();

    if !SETTINGS().social_enabled || platforms.is_empty() {
        return rsx! {
            div { class: "p-6",
                EmptyState { title: "Social ist ausgeschaltet", text: "TikTok, Instagram und X lassen sich unter Einstellungen → Social einschalten.".to_string(),
                    Link { to: Route::SettingsPage { tab: "social".into() }, class: "mt-2 text-sm underline", "Zu den Einstellungen" }
                }
            }
        };
    }

    let follows: Vec<SocialFollow> = SOCIAL_FOLLOWS().into_iter().filter(|f| platforms.contains(&f.platform.as_str())).collect();
    let visible_follows: Vec<SocialFollow> = follows.iter().filter(|f| current.is_empty() || f.platform == current).cloned().collect();
    // Hinweis, wenn eine Plattform ohne Anmeldung nicht (gut) geht
    // – nur wenn dort gerade etwas scheitert
    let failing: Vec<String> = match &*feed.read() {
        Some(Ok(f)) => f.errors.clone(),
        _ => vec![],
    };
    let hints: Vec<SocialStatus> = status()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.needs_login && !s.login && platforms.contains(&s.platform.as_str()))
        .filter(|s| current.is_empty() || s.platform == current)
        .filter(|s| failing.iter().any(|e| e.starts_with(&format!("{} @", platform_name(&s.platform)))))
        .collect();
    let tab = |key: &str, label: &str, active: bool| -> Element {
        let key = key.to_string();
        let label = label.to_string();
        rsx! {
            button { key: "{key}",
                class: if active { "flex shrink-0 items-center gap-2 rounded-full bg-foreground px-3.5 py-1.5 text-sm font-medium text-background" } else { "flex shrink-0 items-center gap-2 rounded-full bg-secondary px-3.5 py-1.5 text-sm hover:bg-accent" },
                onclick: { let key = key.clone(); move |_| { nav.replace(Route::SocialHome { p: key.clone() }); } },
                if key.is_empty() { icons::Sparkles { class: "size-4" } } else { PlatformGlyph { platform: key.clone(), class: "size-4" } }
                "{label}"
                {
                    let n = SOCIAL_FOLLOWS().iter().filter(|f| key.is_empty() || f.platform == key).count();
                    rsx! { if n > 0 { span { class: if active { "rounded-full bg-background/20 px-1.5 text-[11px] tabular-nums" } else { "rounded-full bg-background/60 px-1.5 text-[11px] tabular-nums text-muted-foreground" }, "{n}" } } }
                }
            }
        }
    };

    rsx! {
        div { class: "mx-auto max-w-[1400px] px-3 pb-8 pt-4 md:px-6",
            // Kopf
            div { class: "mb-4 flex flex-wrap items-center gap-3",
                div { class: "flex items-center gap-2.5",
                    span { class: "flex size-9 items-center justify-center rounded-xl {accent_bg(&current)}",
                        if current.is_empty() { icons::Sparkles { class: "size-5" } } else { PlatformGlyph { platform: current.clone(), class: "size-5 {accent(&current)}" } }
                    }
                    h1 { class: "text-xl font-semibold tracking-tight", if current.is_empty() { "Social" } else { "{platform_name(&current)}" } }
                }
                Button { variant: ButtonVariant::Ghost, size: ButtonSize::Sm, class: "ml-auto text-muted-foreground", title: "Neu laden",
                    onclick: move |_| { fresh.set(true); feed.restart(); spawn(async move { gloo_timers::future::TimeoutFuture::new(500).await; fresh.set(false); }); },
                    icons::RefreshCw {} "Aktualisieren"
                }
            }
            // Plattform-Reiter
            div { class: "shelf -mx-3 mb-5 flex gap-2 overflow-x-auto px-3 pb-1 md:mx-0 md:px-0",
                {tab("", "Alle", current.is_empty())}
                for pl in platforms.iter() {
                    {tab(pl, platform_name(pl), current == *pl)}
                }
            }

            div { class: "flex flex-col gap-6 lg:flex-row lg:items-start",
                // Feed
                div { class: "min-w-0 flex-1",
                    for h in hints {
                        div { key: "{h.platform}", class: "mb-4 flex items-start gap-3 rounded-xl border border-amber-500/30 bg-amber-500/10 p-3 text-sm",
                            icons::CircleAlert { class: "mt-0.5 size-4 shrink-0 text-amber-400" }
                            div {
                                if h.platform == "instagram" {
                                    "Instagram zeigt Profile nur angemeldet. Ein Admin kann unter "
                                } else {
                                    "X begrenzt Abrufe ohne Anmeldung – zuverlässiger wird es, wenn ein Admin unter "
                                }
                                Link { to: Route::SettingsPage { tab: "social".into() }, class: "underline", "Einstellungen → Social" }
                                " eine cookies.txt hinterlegt. Einzelne Beiträge (Links) gehen auch so."
                            }
                        }
                    }
                    match feed() {
                        None => rsx! { FeedSkeleton {} },
                        Some(Err(e)) => rsx! { ErrorBox { message: e.message, retry: move |_| feed.restart() } },
                        Some(Ok(f)) => {
                            let total = f.posts.len();
                            rsx! {
                                if !f.errors.is_empty() {
                                    details { class: "mb-4 rounded-xl border bg-card p-3 text-sm",
                                        summary { class: "cursor-pointer text-muted-foreground",
                                            if f.errors.len() == 1 { "1 Konto konnte nicht geladen werden" } else { "{f.errors.len()} Konten konnten nicht geladen werden" }
                                        }
                                        ul { class: "mt-2 space-y-1 text-xs text-muted-foreground",
                                            for e in f.errors.iter() { li { key: "{e}", "{e}" } }
                                        }
                                    }
                                }
                                if total == 0 {
                                    if visible_follows.is_empty() {
                                        EmptyState { title: "Noch niemandem gefolgt",
                                            text: "Füge rechts Konten hinzu – per Link oder @Name. Ihre neuesten Beiträge landen hier, ohne Werbung und ohne Tracking.".to_string() }
                                    } else if f.errors.is_empty() {
                                        EmptyState { title: "Keine Beiträge", text: "Deine Konten haben gerade nichts Neues.".to_string() }
                                    }
                                } else {
                                    div { class: "mx-auto flex max-w-2xl flex-col gap-4",
                                        for post in f.posts.iter().take(shown()).cloned() {
                                            PostCard { key: "{post.platform}-{post.id}", post }
                                        }
                                        if total > shown() {
                                            LoadMore { loading: false, onclick: move |_| shown.set(shown() + 30) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Konten verwalten (Handy: oben, gefolgte Konten als Leiste)
                aside { class: "order-first w-full shrink-0 lg:sticky lg:top-4 lg:order-last lg:w-80",
                    AddAccount { platform: if current.is_empty() { platforms.first().map(|x| x.to_string()).unwrap_or_default() } else { current.clone() }, on_added: move |_| feed.restart() }
                    if !visible_follows.is_empty() {
                        div { class: "shelf -mx-3 mt-3 flex gap-3 overflow-x-auto px-3 lg:hidden",
                            for f in visible_follows.iter().cloned() {
                                Link { key: "m-{f.platform}-{f.handle}", to: Route::SocialProfilePage { platform: f.platform.clone(), handle: f.handle.clone() },
                                    class: "flex w-16 shrink-0 flex-col items-center gap-1 text-center",
                                    Avatar { src: f.avatar.clone(), name: f.name.clone(), platform: f.platform.clone(), size: "size-12".to_string() }
                                    span { class: "w-full truncate text-[11px] text-muted-foreground", "{f.handle}" }
                                }
                            }
                        }
                    }
                    div { class: "mt-4 hidden rounded-2xl border bg-card p-3 lg:block",
                        div { class: "mb-2 flex items-center justify-between px-1",
                            h2 { class: "text-sm font-semibold", "Gefolgt" }
                            span { class: "text-xs text-muted-foreground tabular-nums", "{visible_follows.len()}" }
                        }
                        if visible_follows.is_empty() {
                            p { class: "px-1 py-2 text-xs text-muted-foreground", "Noch keine Konten." }
                        }
                        for pl in platforms.iter().filter(|pl| current.is_empty() || current == **pl) {
                            {
                                let list: Vec<SocialFollow> = visible_follows.iter().filter(|f| f.platform == *pl).cloned().collect();
                                rsx! {
                                    if !list.is_empty() {
                                        div { key: "{pl}", class: "mb-1",
                                            if current.is_empty() {
                                                div { class: "flex items-center gap-1.5 px-1 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground",
                                                    PlatformGlyph { platform: pl.to_string(), class: "size-3" } "{platform_name(pl)}"
                                                }
                                            }
                                            for f in list {
                                                FollowRow { key: "{f.platform}-{f.handle}", f, on_removed: move |_| feed.restart() }
                                            }
                                        }
                                    }
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
fn FollowRow(f: SocialFollow, on_removed: EventHandler<()>) -> Element {
    let label = if f.name.is_empty() { f.handle.clone() } else { f.name.clone() };
    rsx! {
        div { class: "group flex items-center gap-2.5 rounded-lg px-1 py-1.5 hover:bg-accent/50",
            Link { to: Route::SocialProfilePage { platform: f.platform.clone(), handle: f.handle.clone() }, class: "flex min-w-0 flex-1 items-center gap-2.5",
                Avatar { src: f.avatar.clone(), name: label.clone(), platform: f.platform.clone(), size: "size-8".to_string() }
                span { class: "min-w-0",
                    span { class: "block truncate text-sm", "{label}" }
                    span { class: "block truncate text-xs text-muted-foreground", "@{f.handle}" }
                }
            }
            Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, class: "opacity-60 group-hover:opacity-100", title: "Entfolgen",
                onclick: {
                    let f = f.clone();
                    move |_| {
                        let f = f.clone();
                        spawn(async move {
                            match api::delete::<serde_json::Value>(&format!("/api/social/follows/{}/{}", f.platform, f.handle)).await {
                                Ok(_) => { reload_social_follows().await; toast(format!("@{} entfolgt", f.handle)); on_removed.call(()); }
                                Err(e) => toast_err(e),
                            }
                        });
                    }
                },
                icons::X {}
            }
        }
    }
}

/// Konto hinzufügen: Link oder @Name, Plattform wählbar (aus Links erkannt)
#[component]
fn AddAccount(platform: String, on_added: EventHandler<()>) -> Element {
    let mut text = use_signal(String::new);
    let mut pl = use_signal(|| platform.clone());
    let p2 = platform.clone();
    use_effect(use_reactive!(|p2| pl.set(p2)));
    let mut busy = use_signal(|| false);
    let nav = navigator();
    let platforms = enabled_platforms();
    let mut submit = move |follow: bool| {
        let t = text().trim().to_string();
        if t.is_empty() {
            return;
        }
        // Link erkannt: Beitrag öffnen oder Profil (folgen)
        let (platform, handle) = match social_route(&t) {
            Some(Route::SocialProfilePage { platform, handle }) => (platform, handle),
            Some(r @ Route::SocialPostPage { .. }) => { nav.push(r); text.set(String::new()); return; }
            _ => (pl(), t.trim_start_matches('@').to_lowercase()),
        };
        if !follow {
            nav.push(Route::SocialProfilePage { platform, handle });
            text.set(String::new());
            return;
        }
        busy.set(true);
        spawn(async move {
            match api::post::<_, SocialFollow>("/api/social/follows", &SocialFollow { platform: platform.clone(), handle: handle.clone(), ..Default::default() }).await {
                Ok(f) => {
                    reload_social_follows().await;
                    toast(format!("Du folgst {} auf {}", if f.name.is_empty() { format!("@{}", f.handle) } else { f.name.clone() }, platform_name(&platform)));
                    text.set(String::new());
                    on_added.call(());
                }
                Err(e) => toast_err(e),
            }
            busy.set(false);
        });
    };
    rsx! {
        div { class: "rounded-2xl border bg-card p-3",
            h2 { class: "mb-2 px-1 text-sm font-semibold", "Konto hinzufügen" }
            div { class: "mb-2 flex gap-1",
                for p in platforms {
                    button { key: "{p}",
                        class: if pl() == p { "flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-accent px-2 py-1.5 text-xs font-medium" } else { "flex flex-1 items-center justify-center gap-1.5 rounded-lg px-2 py-1.5 text-xs text-muted-foreground hover:bg-accent/50" },
                        onclick: move |_| pl.set(p.to_string()),
                        PlatformGlyph { platform: p.to_string(), class: "size-3.5" }
                        "{platform_name(p)}"
                    }
                }
            }
            form { class: "flex gap-2",
                onsubmit: move |e| { e.prevent_default(); submit(true); },
                div { class: "relative min-w-0 flex-1",
                    icons::AtSign { class: "pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" }
                    input { class: "h-9 w-full rounded-lg border border-input bg-input/30 pl-8 pr-2 text-[16px] outline-none focus:border-ring sm:text-sm",
                        placeholder: "Name oder Link", value: "{text}", autocomplete: "off", autocapitalize: "off",
                        oninput: move |e| text.set(e.value()) }
                }
                Button { size: ButtonSize::Sm, class: "h-9", disabled: busy() || text().trim().is_empty(), button_type: "submit",
                    if busy() { icons::LoaderCircle { class: "animate-spin" } } else { icons::UserPlus {} }
                    "Folgen"
                }
            }
            button { class: "mt-2 px-1 text-xs text-muted-foreground hover:text-foreground disabled:opacity-50", disabled: text().trim().is_empty(),
                onclick: move |_| submit(false),
                "Nur ansehen →"
            }
        }
    }
}

#[component]
fn FeedSkeleton() -> Element {
    rsx! {
        div { class: "mx-auto flex max-w-2xl flex-col gap-4",
            for i in 0..3 {
                div { key: "{i}", class: "animate-pulse rounded-2xl border bg-card p-4",
                    div { class: "flex items-center gap-3",
                        div { class: "size-10 rounded-full bg-muted" }
                        div { class: "flex-1 space-y-2", div { class: "h-3 w-32 rounded bg-muted" } div { class: "h-2.5 w-20 rounded bg-muted" } }
                    }
                    div { class: "mt-4 h-3 w-3/4 rounded bg-muted" }
                    div { class: "mt-3 aspect-video rounded-xl bg-muted" }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Bausteine

#[component]
fn Avatar(src: String, name: String, platform: String, #[props(default)] size: Option<String>) -> Element {
    let size = size.unwrap_or_else(|| "size-10".into());
    rsx! {
        span { class: "relative shrink-0",
            if src.is_empty() {
                span { class: "flex {size} items-center justify-center rounded-full bg-muted text-sm font-semibold", {name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()} }
            } else {
                img { class: "{size} rounded-full object-cover", src: "{src}", alt: "", loading: "lazy" }
            }
            span { class: "absolute -bottom-0.5 -right-0.5 flex size-4 items-center justify-center rounded-full border-2 border-card bg-background {accent(&platform)}",
                PlatformGlyph { platform: platform.clone(), class: "size-2.5" }
            }
        }
    }
}

fn stat(n: Option<u64>) -> String {
    n.map(util::compact).unwrap_or_default()
}

/// Ein Beitrag: Kopf mit Konto, Text, Bilder/Videos, Zahlen
#[component]
pub fn PostCard(post: SocialPost, #[props(default)] full: bool) -> Element {
    let mut expanded = use_signal(|| full);
    let long = post.text.chars().count() > 280 || post.text.lines().count() > 6;
    let name = if post.author_name.is_empty() { post.author.clone() } else { post.author_name.clone() };
    let profile = Route::SocialProfilePage { platform: post.platform.clone(), handle: post.author.clone() };
    rsx! {
        article { class: "overflow-hidden rounded-2xl border bg-card",
            if let Some(by) = &post.repost_by {
                div { class: "flex items-center gap-1.5 px-4 pt-3 text-xs text-muted-foreground", icons::Repeat2 { class: "size-3.5" } "@{by} hat geteilt" }
            }
            header { class: "flex items-center gap-3 px-4 pt-3",
                Link { to: profile.clone(), class: "flex min-w-0 flex-1 items-center gap-3",
                    Avatar { src: post.author_avatar.clone(), name: name.clone(), platform: post.platform.clone() }
                    span { class: "min-w-0",
                        span { class: "block truncate text-sm font-semibold", "{name}" }
                        span { class: "block truncate text-xs text-muted-foreground",
                            "@{post.author}"
                            if post.published > 0 { " · {util::ago(post.published)}" }
                        }
                    }
                }
                if !full {
                    Link { to: Route::SocialPostPage { platform: post.platform.clone(), id: post.id.clone(), h: post.author.clone() },
                        class: "rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground", title: "Beitrag öffnen",
                        icons::ChevronRight { class: "size-4" }
                    }
                }
                a { class: "rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground", href: "{post.url}", target: "_blank", rel: "noreferrer noopener", title: "Auf {platform_name(&post.platform)} öffnen",
                    icons::ExternalLink { class: "size-4" }
                }
            }
            if !post.text.is_empty() {
                div { class: "px-4 pt-2.5",
                    RichText { text: post.text.clone(), class: if expanded() || !long { "text-[15px] leading-relaxed" } else { "line-clamp-6 text-[15px] leading-relaxed" } }
                    if long && !expanded() {
                        button { class: "mt-1 text-sm text-muted-foreground hover:text-foreground", onclick: move |_| expanded.set(true), "Mehr anzeigen" }
                    }
                }
            }
            if !post.media.is_empty() {
                MediaView { media: post.media.clone(), platform: post.platform.clone() }
            }
            footer { class: "flex flex-wrap items-center gap-x-5 gap-y-1 px-4 py-3 text-xs text-muted-foreground tabular-nums",
                if post.likes.is_some() { span { class: "flex items-center gap-1.5", icons::Heart { class: "size-4" } {stat(post.likes)} } }
                if post.comments.is_some() { span { class: "flex items-center gap-1.5", icons::MessageCircle { class: "size-4" } {stat(post.comments)} } }
                if post.reposts.is_some() { span { class: "flex items-center gap-1.5", icons::Repeat2 { class: "size-4" } {stat(post.reposts)} } }
                if post.views.is_some() { span { class: "flex items-center gap-1.5", icons::Eye { class: "size-4" } {stat(post.views)} } }
                if let Some(d) = post.duration { if d > 0.0 { span { class: "ml-auto", {util::duration(d)} } } }
            }
        }
    }
}

/// Bilder und Videos eines Beitrags; mehrere als wischbares Karussell
#[component]
fn MediaView(media: Vec<SocialMedia>, platform: String) -> Element {
    let mut idx = use_signal(|| 0usize);
    let n = media.len();
    let i = idx().min(n.saturating_sub(1));
    let m = media[i].clone();
    let portrait = m.height > m.width && m.width > 0;
    let lp = platform.clone();
    rsx! {
        div { class: "relative mt-3 flex items-center justify-center bg-black",
            if m.kind == "video" {
                video { key: "{m.url}", class: if portrait { "max-h-[78vh] w-auto max-w-full" } else { "max-h-[78vh] w-full" },
                    style: if portrait { format!("aspect-ratio: {} / {}", m.width, m.height) } else { String::new() },
                    src: "{m.url}", poster: "{m.thumb}", controls: true, playsinline: true, preload: "none",
                    "loop": lp == "tiktok",
                }
            } else {
                img { key: "{m.url}", class: "max-h-[78vh] w-full object-contain", src: "{m.url}", alt: "", loading: "lazy" }
            }
            if n > 1 {
                if i > 0 {
                    button { class: "absolute left-2 top-1/2 flex size-8 -translate-y-1/2 items-center justify-center rounded-full bg-black/60 text-white hover:bg-black/80", aria_label: "Zurück",
                        onclick: move |_| idx.set(i - 1), icons::ChevronLeft { class: "size-5" } }
                }
                if i + 1 < n {
                    button { class: "absolute right-2 top-1/2 flex size-8 -translate-y-1/2 items-center justify-center rounded-full bg-black/60 text-white hover:bg-black/80", aria_label: "Weiter",
                        onclick: move |_| idx.set(i + 1), icons::ChevronRight { class: "size-5" } }
                }
                div { class: "absolute bottom-2 left-1/2 flex -translate-x-1/2 gap-1",
                    for k in 0..n {
                        span { key: "{k}", class: if k == i { "size-1.5 rounded-full bg-white" } else { "size-1.5 rounded-full bg-white/40" } }
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Profil

#[component]
pub fn SocialProfilePage(platform: String, handle: String) -> Element {
    let key = (platform.clone(), handle.clone());
    let mut fresh = use_signal(|| false);
    let mut prof = use_resource(use_reactive!(|key| async move {
        let f = *fresh.peek();
        api::get::<SocialProfile>(&format!("/api/social/profile/{}/{}?fresh={f}", key.0, api::enc(&key.1))).await
    }));
    let mut open = use_signal(|| None::<SocialPost>);
    let grid = platform != "x";
    rsx! {
        div { class: "mx-auto max-w-5xl px-3 pb-8 pt-4 md:px-6",
            Link { to: Route::SocialHome { p: platform.clone() }, class: "mb-3 inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground",
                icons::ChevronLeft { class: "size-4" } "{platform_name(&platform)}"
            }
            match prof() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! {
                    ProfileHead { prof: SocialProfile { platform: platform.clone(), handle: handle.clone(), name: handle.clone(), url: profile_url(&platform, &handle), ..Default::default() } }
                    ErrorBox { message: e.message, retry: move |_| { fresh.set(true); prof.restart(); } }
                },
                Some(Ok(p)) => rsx! {
                    ProfileHead { prof: p.clone() }
                    if p.posts.is_empty() {
                        EmptyState { title: "Keine Beiträge" }
                    } else if grid {
                        div { class: if platform == "tiktok" { "grid grid-cols-3 gap-1 sm:grid-cols-4 lg:grid-cols-5" } else { "grid grid-cols-3 gap-1" },
                            for post in p.posts.iter().cloned() {
                                PostTile { key: "{post.id}", post: post.clone(), onclick: move |p: SocialPost| open.set(Some(p)) }
                            }
                        }
                    } else {
                        div { class: "mx-auto flex max-w-2xl flex-col gap-4",
                            for post in p.posts.iter().cloned() { PostCard { key: "{post.id}", post } }
                        }
                    }
                },
            }
        }
        if let Some(post) = open() {
            div { class: "fixed inset-0 z-[90] flex items-center justify-center bg-black/80 p-0 sm:p-4", onclick: move |_| open.set(None),
                div { class: "max-h-[100dvh] w-full max-w-xl overflow-y-auto sm:max-h-[94vh]", onclick: move |e| e.stop_propagation(),
                    div { class: "flex justify-end p-2 sm:px-0",
                        button { class: "flex size-8 items-center justify-center rounded-full bg-white/10 text-white hover:bg-white/20", aria_label: "Schliessen",
                            onclick: move |_| open.set(None), icons::X { class: "size-4" } }
                    }
                    PostCard { key: "{post.id}", post: post.clone(), full: true }
                }
            }
        }
    }
}

fn profile_url(p: &str, h: &str) -> String {
    match p {
        "tiktok" => format!("https://www.tiktok.com/@{h}"),
        "instagram" => format!("https://www.instagram.com/{h}/"),
        _ => format!("https://x.com/{h}"),
    }
}

#[component]
fn ProfileHead(prof: SocialProfile) -> Element {
    let following = is_social_following(&prof.platform, &prof.handle);
    let name = if prof.name.is_empty() { prof.handle.clone() } else { prof.name.clone() };
    rsx! {
        div { class: "mb-6 flex flex-col gap-4 sm:flex-row sm:items-start",
            if prof.avatar.is_empty() {
                span { class: "flex size-20 shrink-0 items-center justify-center rounded-full bg-muted text-2xl font-semibold sm:size-24", {name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()} }
            } else {
                img { class: "size-20 shrink-0 rounded-full object-cover sm:size-24", src: "{prof.avatar}", alt: "" }
            }
            div { class: "min-w-0 flex-1",
                div { class: "flex flex-wrap items-center gap-2",
                    h1 { class: "text-xl font-semibold tracking-tight", "{name}" }
                    if prof.verified { icons::BadgeCheck { class: "size-5 text-sky-400" } }
                    span { class: "flex items-center gap-1 rounded-full px-2 py-0.5 text-xs {accent_bg(&prof.platform)} {accent(&prof.platform)}",
                        PlatformGlyph { platform: prof.platform.clone(), class: "size-3" } "{platform_name(&prof.platform)}"
                    }
                }
                div { class: "text-sm text-muted-foreground", "@{prof.handle}" }
                div { class: "mt-2 flex flex-wrap gap-x-4 gap-y-1 text-sm",
                    if let Some(f) = prof.followers { span { b { {util::compact(f)} } span { class: "text-muted-foreground", " Follower" } } }
                    if let Some(f) = prof.following { span { b { {util::compact(f)} } span { class: "text-muted-foreground", " folgt" } } }
                    if let Some(f) = prof.post_count { span { b { {util::compact(f)} } span { class: "text-muted-foreground", " Beiträge" } } }
                }
                if !prof.bio.is_empty() {
                    RichText { text: prof.bio.clone(), class: "mt-2 max-w-2xl text-sm text-muted-foreground" }
                }
                div { class: "mt-3 flex flex-wrap gap-2",
                    Button { variant: if following { ButtonVariant::Secondary } else { ButtonVariant::Default }, size: ButtonSize::Sm,
                        onclick: {
                            let p = prof.clone();
                            move |_| toggle_social_follow(p.platform.clone(), p.handle.clone(), p.name.clone(), p.avatar.clone())
                        },
                        if following { icons::UserCheck {} "Gefolgt" } else { icons::UserPlus {} "Folgen" }
                    }
                    a { class: "inline-flex h-8 items-center gap-1.5 rounded-md border border-input bg-input/30 px-3 text-sm hover:bg-accent",
                        href: "{prof.url}", target: "_blank", rel: "noreferrer noopener",
                        icons::ExternalLink { class: "size-4" } "Auf {platform_name(&prof.platform)} öffnen"
                    }
                }
            }
        }
    }
}

/// Kachel im Profil-Raster (TikTok hochkant, Instagram quadratisch)
#[component]
fn PostTile(post: SocialPost, onclick: EventHandler<SocialPost>) -> Element {
    let m = post.media.first().cloned().unwrap_or_default();
    let thumb = if m.thumb.is_empty() { m.url.clone() } else { m.thumb.clone() };
    let ratio = if post.platform == "tiktok" { "aspect-[9/16]" } else { "aspect-square" };
    let p2 = post.clone();
    rsx! {
        button { class: "group relative {ratio} overflow-hidden rounded-md bg-muted", onclick: move |_| onclick.call(p2.clone()),
            if !thumb.is_empty() && !thumb.starts_with("/api/") {
                img { class: "absolute inset-0 size-full object-cover transition-transform group-hover:scale-105", src: "{thumb}", alt: "", loading: "lazy" }
            }
            div { class: "absolute inset-x-0 bottom-0 flex items-center gap-2 bg-gradient-to-t from-black/70 to-transparent px-2 pb-1.5 pt-6 text-[11px] font-medium text-white tabular-nums",
                if let Some(v) = post.views { span { class: "flex items-center gap-1", icons::Play { class: "size-3 fill-current" } {util::compact(v)} } }
                else if let Some(l) = post.likes { span { class: "flex items-center gap-1", icons::Heart { class: "size-3" } {util::compact(l)} } }
            }
            if post.media.len() > 1 {
                icons::Images { class: "absolute right-1.5 top-1.5 size-4 text-white drop-shadow" }
            } else if m.kind == "video" && post.platform != "tiktok" {
                icons::Play { class: "absolute right-1.5 top-1.5 size-4 fill-current text-white drop-shadow" }
            }
        }
    }
}

// ------------------------------------------------------------------ Einzelner Beitrag

#[component]
pub fn SocialPostPage(platform: String, id: String, h: String) -> Element {
    let key = (platform.clone(), id.clone(), h.clone());
    let mut post = use_resource(use_reactive!(|key| async move {
        api::get::<SocialPost>(&format!("/api/social/post/{}/{}?h={}", key.0, key.1, api::enc(&key.2))).await
    }));
    rsx! {
        div { class: "mx-auto max-w-2xl px-3 pb-8 pt-4 md:px-6",
            Link { to: Route::SocialHome { p: platform.clone() }, class: "mb-3 inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground",
                icons::ChevronLeft { class: "size-4" } "{platform_name(&platform)}"
            }
            match post() {
                None => rsx! { Loading { label: "Beitrag wird geladen …".to_string() } },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message, retry: move |_| post.restart() } },
                Some(Ok(p)) => rsx! { PostCard { post: p, full: true } },
            }
        }
    }
}
