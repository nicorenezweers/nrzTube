//! Recap: Sehzeit nach Kanal und Video, pro Tag und Uhrzeit – mit Details auf Klick.
//!
//! Darstellung nach der Dataviz-Anleitung: Kennzahlen als Zahlenkacheln, ein einziges
//! Balkendiagramm (eine Farbe, keine Legende) für die Tage, Ranglisten mit dünnen Balken,
//! Werte als Text in Schriftfarbe, Hover zeigt den genauen Wert.

use crate::api;
use crate::components::*;
use crate::util;
use dioxus::prelude::*;
use nrztube_types::*;

const RANGES: &[(&str, &str)] = &[("7", "7 Tage"), ("30", "30 Tage"), ("365", "Jahr"), ("all", "Gesamt")];

/// 3 Std. 12 Min. / 45 Min. / 30 Sek.
pub fn fmt_time(sec: f64) -> String {
    let s = sec.max(0.0).round() as u64;
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h >= 100 {
        format!("{h} Std.")
    } else if h > 0 {
        format!("{h} Std. {m} Min.")
    } else if m > 0 {
        format!("{m} Min.")
    } else {
        format!("{s} Sek.")
    }
}

fn short_day(d: &str) -> String {
    // JJJJ-MM-TT -> TT.MM.
    let p: Vec<&str> = d.split('-').collect();
    if p.len() == 3 { format!("{}.{}.", p[2], p[1]) } else { d.to_string() }
}

#[derive(Clone, PartialEq)]
enum Detail {
    Channel(String, String),
    Video(String),
}

#[component]
pub fn RecapSettings() -> Element {
    let mut range = use_signal(|| "30".to_string());
    let mut tab = use_signal(|| "channels");
    let mut detail = use_signal(|| None::<Detail>);
    let data = use_resource(move || async move { api::get::<Recap>(&format!("/api/recap?range={}", range())).await });
    rsx! {
        div { class: "space-y-6",
            div { class: "flex flex-wrap items-center gap-2",
                h2 { class: "mr-auto text-lg font-semibold", "Dein Recap" }
                for (k, l) in RANGES.iter() {
                    button { key: "{k}",
                        class: if range() == *k { "rounded-lg bg-foreground px-3 py-1.5 text-sm font-medium text-background" } else { "rounded-lg bg-secondary px-3 py-1.5 text-sm hover:bg-accent" },
                        onclick: move |_| range.set(k.to_string()),
                        "{l}"
                    }
                }
            }
            match data() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message } },
                Some(Ok(r)) if r.total < 1.0 => rsx! {
                    EmptyState { title: "Noch nichts geschaut".to_string(), text: "Sobald du Videos schaust, siehst du hier, wie lange du welchen Kanal und welches Video geschaut hast. Im privaten Stream-Modus und mit ausgeschaltetem Verlauf wird nichts gezählt.".to_string() }
                },
                Some(Ok(r)) => {
                    let n_days = r.days.len().max(1) as f64;
                    let avg = if r.range == "all" { r.days.iter().map(|d| d.seconds).sum::<f64>() / n_days } else { r.total / n_days };
                    rsx! {
                        // Kennzahlen
                        div { class: "grid grid-cols-2 gap-3 sm:grid-cols-4",
                            Tile { label: "Sehzeit", value: fmt_time(r.total) }
                            Tile { label: "Videos", value: util::number(r.videos as u64) }
                            Tile { label: "Kanäle", value: util::number(r.channels as u64) }
                            Tile { label: "Ø pro Tag", value: fmt_time(avg) }
                        }
                        // Tage
                        section { class: "rounded-xl border bg-card p-4",
                            div { class: "mb-3 flex items-baseline justify-between gap-3",
                                h3 { class: "text-sm font-semibold", "Sehzeit pro Tag" }
                                span { class: "text-xs text-muted-foreground", "{r.active_days} aktive Tage" }
                            }
                            DayBars { days: r.days.clone() }
                            if r.range == "all" {
                                if let Some(s) = &r.since {
                                    p { class: "mt-2 text-xs text-muted-foreground", "Tageswerte gibt es seit {short_day(s)}{&s[..4]} – die Gesamtzeit oben enthält auch ältere Wiedergaben." }
                                }
                            }
                        }
                        // Uhrzeit
                        section { class: "rounded-xl border bg-card p-4",
                            h3 { class: "mb-3 text-sm font-semibold", "Wann du schaust" }
                            HourBars { hours: r.hours.clone() }
                        }
                        // Ranglisten
                        section { class: "rounded-xl border bg-card",
                            div { class: "flex gap-1 border-b px-2",
                                for (k, l) in [("channels", "Kanäle"), ("videos", "Videos")] {
                                    button { key: "{k}",
                                        class: if tab() == k { "border-b-2 border-foreground px-3 py-2.5 text-sm font-medium" } else { "border-b-2 border-transparent px-3 py-2.5 text-sm text-muted-foreground hover:text-foreground" },
                                        onclick: move |_| tab.set(k),
                                        "{l}"
                                    }
                                }
                            }
                            div { class: "p-2",
                                if tab() == "channels" {
                                    {
                                        let max = r.channels_list.first().map(|c| c.seconds).unwrap_or(1.0).max(1.0);
                                        rsx! {
                                            for (i, c) in r.channels_list.iter().enumerate() {
                                                {
                                                    let (cid, cname) = (c.id.clone(), c.name.clone());
                                                    rsx! {
                                                        button { key: "{c.id}", class: "flex w-full items-center gap-3 rounded-lg px-2 py-2 text-left hover:bg-accent/50",
                                                            onclick: move |_| detail.set(Some(Detail::Channel(cid.clone(), cname.clone()))),
                                                            span { class: "w-6 shrink-0 text-right text-xs tabular-nums text-muted-foreground", "{i + 1}" }
                                                            if c.thumb.is_empty() {
                                                                span { class: "flex size-8 shrink-0 items-center justify-center rounded-full bg-muted text-xs font-semibold", {c.name.chars().next().map(|x| x.to_string()).unwrap_or_default()} }
                                                            } else {
                                                                img { class: "size-8 shrink-0 rounded-full object-cover", src: "{c.thumb}", alt: "", loading: "lazy" }
                                                            }
                                                            div { class: "min-w-0 flex-1",
                                                                div { class: "flex items-baseline justify-between gap-2",
                                                                    span { class: "truncate text-sm font-medium",
                                                                        if c.id.starts_with("tw:") { TwitchGlyph { class: "mr-1 inline size-3.5 align-[-2px] text-[#a970ff]" } }
                                                                        "{c.name}"
                                                                    }
                                                                    span { class: "shrink-0 text-xs tabular-nums text-muted-foreground", "{fmt_time(c.seconds)}" }
                                                                }
                                                                div { class: "mt-1 h-1.5 overflow-hidden rounded-full bg-secondary",
                                                                    div { class: "h-full rounded-full bg-foreground/70", style: "width: {(c.seconds / max * 100.0).max(1.0)}%" }
                                                                }
                                                                div { class: "mt-0.5 text-[11px] text-muted-foreground", if c.videos == 1 { "1 Video" } else { "{c.videos} Videos" } }
                                                            }
                                                            icons::ChevronRight { class: "size-4 shrink-0 text-muted-foreground" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    VideoRank { videos: r.videos_list.clone(), onselect: move |id: String| detail.set(Some(Detail::Video(id))) }
                                }
                            }
                        }
                    }
                }
            }
        }
        match detail() {
            Some(Detail::Channel(id, name)) => rsx! { ChannelDetail { id, name, range: range(), onclose: move |_| detail.set(None), onvideo: move |v: String| detail.set(Some(Detail::Video(v))) } },
            Some(Detail::Video(id)) => rsx! { VideoDetail { id, onclose: move |_| detail.set(None) } },
            None => rsx! {},
        }
    }
}

#[component]
fn Tile(label: String, value: String) -> Element {
    rsx! {
        div { class: "rounded-xl border bg-card p-4",
            div { class: "text-xs text-muted-foreground", "{label}" }
            div { class: "mt-1 text-xl font-semibold tabular-nums tracking-tight", "{value}" }
        }
    }
}

/// Ein Balken pro Tag, eine Farbe; Hover zeigt Tag und Wert
#[component]
fn DayBars(days: Vec<RecapDay>) -> Element {
    let max = days.iter().map(|d| d.seconds).fold(0.0, f64::max).max(1.0);
    let gap = if days.len() > 60 { "gap-px" } else { "gap-[2px]" };
    let first = days.first().map(|d| short_day(&d.day)).unwrap_or_default();
    let last = days.last().map(|d| short_day(&d.day)).unwrap_or_default();
    rsx! {
        div { class: "relative",
            div { class: "flex h-32 items-end justify-end {gap}",
                for d in days.iter() {
                    div { key: "{d.day}", class: "group relative flex h-full max-w-8 flex-1 items-end",
                        div { class: "w-full rounded-t-[4px] bg-foreground/70 group-hover:bg-foreground", style: format!("height: {}%", if d.seconds > 0.0 { (d.seconds / max * 100.0).max(2.0) } else { 0.0 }) }
                        div { class: "pointer-events-none absolute -top-9 left-1/2 z-10 hidden -translate-x-1/2 whitespace-nowrap rounded-md border bg-popover px-2 py-1 text-[11px] shadow-lg group-hover:block",
                            span { class: "text-muted-foreground", "{short_day(&d.day)} " }
                            span { class: "font-medium tabular-nums", "{fmt_time(d.seconds)}" }
                        }
                    }
                }
            }
            div { class: "mt-1.5 flex justify-between border-t pt-1 text-[11px] text-muted-foreground",
                span { "{first}" }
                span { "Höchster Tag: {fmt_time(max)}" }
                span { "{last}" }
            }
        }
    }
}

#[component]
fn HourBars(hours: Vec<f64>) -> Element {
    let max = hours.iter().cloned().fold(0.0, f64::max).max(1.0);
    rsx! {
        div { class: "flex h-16 items-end gap-[2px]",
            for (h, v) in hours.iter().enumerate() {
                div { key: "{h}", class: "group relative flex h-full flex-1 items-end",
                    div { class: "w-full rounded-t-[4px] bg-foreground/60 group-hover:bg-foreground", style: format!("height: {}%", if *v > 0.0 { (v / max * 100.0).max(3.0) } else { 0.0 }) }
                    div { class: "pointer-events-none absolute -top-9 left-1/2 z-10 hidden -translate-x-1/2 whitespace-nowrap rounded-md border bg-popover px-2 py-1 text-[11px] shadow-lg group-hover:block",
                        span { class: "text-muted-foreground", "{h}–{h + 1} Uhr " }
                        span { class: "font-medium tabular-nums", "{fmt_time(*v)}" }
                    }
                }
            }
        }
        div { class: "mt-1.5 flex justify-between border-t pt-1 text-[11px] text-muted-foreground",
            span { "0 Uhr" } span { "6" } span { "12" } span { "18" } span { "24 Uhr" }
        }
    }
}

#[component]
fn VideoRank(videos: Vec<RecapVideo>, onselect: EventHandler<String>) -> Element {
    let max = videos.first().map(|v| v.seconds).unwrap_or(1.0).max(1.0);
    rsx! {
        for (i, v) in videos.iter().enumerate() {
            {
                let id = v.video.id.clone();
                rsx! {
                    button { key: "{v.video.id}", class: "flex w-full items-center gap-3 rounded-lg px-2 py-2 text-left hover:bg-accent/50",
                        onclick: move |_| onselect.call(id.clone()),
                        span { class: "w-6 shrink-0 text-right text-xs tabular-nums text-muted-foreground", "{i + 1}" }
                        img { class: "aspect-video w-24 shrink-0 rounded-md bg-muted object-cover", src: "{v.video.thumb}", alt: "", loading: "lazy" }
                        div { class: "min-w-0 flex-1",
                            div { class: "flex items-baseline justify-between gap-2",
                                span { class: "line-clamp-1 text-sm font-medium", "{v.video.title}" }
                                span { class: "shrink-0 text-xs tabular-nums text-muted-foreground", "{fmt_time(v.seconds)}" }
                            }
                            div { class: "truncate text-xs text-muted-foreground", "{v.video.author}" }
                            div { class: "mt-1 h-1.5 overflow-hidden rounded-full bg-secondary",
                                div { class: "h-full rounded-full bg-foreground/70", style: "width: {(v.seconds / max * 100.0).max(1.0)}%" }
                            }
                        }
                        icons::ChevronRight { class: "size-4 shrink-0 text-muted-foreground" }
                    }
                }
            }
        }
    }
}

#[component]
fn ChannelDetail(id: String, name: String, range: String, onclose: EventHandler<()>, onvideo: EventHandler<String>) -> Element {
    let key = (id.clone(), range.clone());
    let data = use_resource(use_reactive!(|key| async move {
        let (id, range) = key;
        api::get::<RecapDetail>(&format!("/api/recap/channel/{}?range={range}", api::enc(&id))).await
    }));
    let label = RANGES.iter().find(|(k, _)| *k == range).map(|(_, l)| *l).unwrap_or("Gesamt");
    rsx! {
        Modal { title: name.clone(), wide: true, onclose: move |_| onclose.call(()),
            match data() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message } },
                Some(Ok(d)) => rsx! {
                    div { class: "mb-4 grid grid-cols-2 gap-3",
                        Tile { label: format!("Sehzeit ({label})"), value: fmt_time(d.total) }
                        Tile { label: "Videos".to_string(), value: d.videos.len().to_string() }
                    }
                    if d.days.iter().any(|x| x.seconds > 0.0) {
                        div { class: "mb-5 rounded-xl border bg-card p-4",
                            h3 { class: "mb-3 text-sm font-semibold", "Pro Tag" }
                            DayBars { days: d.days.clone() }
                        }
                    }
                    h3 { class: "mb-1 text-sm font-semibold", "Videos dieses Kanals" }
                    VideoRank { videos: d.videos.clone(), onselect: move |v: String| onvideo.call(v) }
                },
            }
        }
    }
}

#[component]
fn VideoDetail(id: String, onclose: EventHandler<()>) -> Element {
    let vid = id.clone();
    let data = use_resource(use_reactive!(|vid| async move { api::get::<RecapDetail>(&format!("/api/recap/video/{}", api::enc(&vid))).await }));
    let nav = navigator();
    rsx! {
        Modal { title: "Video im Detail".to_string(), wide: true, onclose: move |_| onclose.call(()),
            match data() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message } },
                Some(Ok(d)) => {
                    let v = d.video.clone().unwrap_or_default();
                    let pct = if v.duration > 0.0 { (v.progress / v.duration * 100.0).clamp(0.0, 100.0) } else { 0.0 };
                    let rounds = if v.duration > 0.0 { d.total / v.duration } else { 0.0 };
                    let vid2 = v.video.id.clone();
                    rsx! {
                        div { class: "mb-4 flex gap-3",
                            img { class: "aspect-video w-40 shrink-0 rounded-lg bg-muted object-cover", src: "{v.video.thumb}", alt: "" }
                            div { class: "min-w-0",
                                div { class: "line-clamp-2 font-medium", "{v.video.title}" }
                                div { class: "text-sm text-muted-foreground", "{v.video.author}" }
                                button { class: "mt-2 inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground",
                                    onclick: move |_| { onclose.call(()); nav.push(watch_route(&vid2, None, None)); },
                                    icons::Play { class: "size-3.5 fill-current" } "Ansehen"
                                }
                            }
                        }
                        div { class: "mb-4 grid grid-cols-2 gap-3 sm:grid-cols-4",
                            Tile { label: "Sehzeit".to_string(), value: fmt_time(d.total) }
                            Tile { label: "Geöffnet".to_string(), value: format!("{}×", v.opens.max(1)) }
                            Tile { label: "Länge".to_string(), value: if v.duration > 0.0 { util::duration(v.duration) } else { "live".into() } }
                            Tile { label: "Zuletzt".to_string(), value: if v.last > 0 { util::ago(v.last) } else { "–".into() } }
                        }
                        if v.duration > 0.0 {
                            div { class: "mb-4 rounded-xl border bg-card p-4",
                                div { class: "mb-1.5 flex justify-between text-xs text-muted-foreground",
                                    span { "Stelle: {util::duration(v.progress)} von {util::duration(v.duration)}" }
                                    span { if rounds >= 1.05 { "{rounds:.1}× komplett geschaut" } else { "{pct:.0} %" } }
                                }
                                div { class: "h-1.5 overflow-hidden rounded-full bg-secondary",
                                    div { class: "h-full rounded-full bg-foreground/70", style: "width: {pct}%" }
                                }
                            }
                        }
                        if d.days.iter().any(|x| x.seconds > 0.0) {
                            div { class: "rounded-xl border bg-card p-4",
                                h3 { class: "mb-3 text-sm font-semibold", "Wann du es geschaut hast" }
                                DayBars { days: d.days.clone() }
                            }
                        }
                    }
                }
            }
        }
    }
}
