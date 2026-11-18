//! Can I React: Reaktionsrichtlinien des Kanals unter dem Video (Stream-Reaktion,
//! Reaction-Video, Bedingungen, Placement- und TOS-Abschnitte, bekannte Reaktionen).

use crate::api;
use crate::state::*;
use crate::util;
use dioxus::prelude::*;
use nrztube_types::*;

#[derive(Clone, Copy, PartialEq)]
enum Verdict {
    Yes,
    Later(f64),
    No,
    Unknown,
}

/// Erlaubt? Bei "erst nach X Stunden" mit Restzeit ab dem Upload
fn verdict(allowed: Option<bool>, generally: Option<bool>, after_hours: Option<f64>, uploaded: Option<i64>) -> Verdict {
    match (allowed, after_hours, uploaded) {
        (Some(true), _, _) => Verdict::Yes,
        (_, Some(h), Some(up)) => {
            let left = (up as f64 + h * 3600.0 - util::now() as f64) / 3600.0;
            if left <= 0.0 {
                if generally.unwrap_or(true) { Verdict::Yes } else { Verdict::No }
            } else if generally.unwrap_or(true) {
                Verdict::Later(left)
            } else {
                Verdict::No
            }
        }
        (Some(false), _, _) => Verdict::No,
        _ => Verdict::Unknown,
    }
}

fn hours_text(h: f64) -> String {
    if h >= 48.0 { format!("{} Tagen", (h / 24.0).ceil() as i64) } else if h >= 1.0 { format!("{} Std.", h.ceil() as i64) } else { format!("{} Min.", (h * 60.0).ceil().max(1.0) as i64) }
}

#[component]
fn Chip(v: Verdict, yes: String, no: String, later: String) -> Element {
    let (cls, dot, text) = match v {
        Verdict::Yes => ("border-green-500/30 bg-green-500/10 text-green-300", "bg-green-500", yes),
        Verdict::Later(h) => ("border-amber-500/30 bg-amber-500/10 text-amber-300", "bg-amber-500", format!("{later} {}", hours_text(h))),
        Verdict::No => ("border-red-500/30 bg-red-500/10 text-red-300", "bg-red-500", no),
        Verdict::Unknown => return rsx! {},
    };
    rsx! {
        span { class: "flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-medium {cls}",
            span { class: "size-1.5 rounded-full {dot}" }
            "{text}"
        }
    }
}

#[component]
pub fn ReactPanel(video: String, channel: String) -> Element {
    let key = (video.clone(), channel.clone());
    let data = use_resource(use_reactive!(|key| async move {
        let (v, c) = key;
        if !c.starts_with("UC") {
            return None;
        }
        api::get::<ReactInfo>(&format!("/api/canireact/{v}?channel={c}")).await.ok()
    }));
    let mut open = use_signal(|| false);
    if !SETTINGS().can_i_react {
        return rsx! {};
    }
    let Some(Some(r)) = data() else { return rsx! {} };
    let nothing = !r.has_rules && r.info_text.is_none() && r.tos.is_empty() && r.reactions.is_empty() && r.original.is_none();
    if nothing {
        return rsx! {};
    }
    let sv = verdict(r.stream.allowed, r.stream.generally_allowed, r.stream.after_hours, r.uploaded);
    let vv = verdict(r.video.allowed, r.video.generally_allowed, r.video.after_hours, r.uploaded);
    let restrictions = !r.stream.custom.is_empty()
        || r.stream.sponsor_skips_allowed == Some(false)
        || r.stream.credit_chat
        || !r.video.custom.is_empty()
        || r.video.sponsor_cut_allowed == Some(false)
        || r.video.credit_description
        || r.video.title_must_include
        || r.video.monetization_allowed == Some(false)
        || r.video.splitting_allowed == Some(false);
    let views: u64 = r.reactions.iter().map(|x| x.views).sum();
    rsx! {
        div { class: "mt-4 rounded-xl border bg-card",
            button { class: "flex w-full flex-wrap items-center gap-2 px-3 py-2.5 text-left",
                onclick: move |_| open.set(!open()),
                span { class: "mr-1 flex items-center gap-1.5 text-xs font-semibold text-muted-foreground",
                    icons::Clapperboard { class: "size-4" } "Can I React?"
                }
                if r.has_rules {
                    Chip { v: sv, yes: "Stream-Reaktion erlaubt".to_string(), no: "Stream-Reaktion nicht erlaubt".to_string(), later: "Stream-Reaktion in".to_string() }
                    Chip { v: vv, yes: "Reaction-Video erlaubt".to_string(), no: "Reaction-Video nicht erlaubt".to_string(), later: "Reaction-Video in".to_string() }
                    if restrictions {
                        span { class: "flex items-center gap-1 rounded-full border border-amber-500/30 px-2.5 py-1 text-xs text-amber-300", icons::TriangleAlert { class: "size-3" } "Einschränkungen beachten" }
                    } else {
                        span { class: "rounded-full border px-2.5 py-1 text-xs text-muted-foreground", "Keine Einschränkungen" }
                    }
                }
                if !r.tos.is_empty() {
                    span { class: "flex items-center gap-1 rounded-full border border-red-500/30 px-2.5 py-1 text-xs text-red-300", icons::ShieldAlert { class: "size-3" } "Enthält TOS-Abschnitte" }
                }
                if !r.reactions.is_empty() {
                    span { class: "rounded-full border px-2.5 py-1 text-xs text-muted-foreground", "{r.reactions.len()} Reaktionen" }
                }
                span { class: "ml-auto text-muted-foreground",
                    if open() { icons::ChevronUp { class: "size-4" } } else { icons::ChevronDown { class: "size-4" } }
                }
            }
            if let Some(o) = &r.original {
                Link { to: crate::components::watch_route(&o.id, None, None),
                    class: "mx-3 mb-2.5 flex items-center gap-3 rounded-lg bg-secondary/60 p-2 text-sm hover:bg-secondary",
                    img { class: "aspect-video w-24 shrink-0 rounded object-cover", src: "{o.thumb}", alt: "" }
                    div { class: "min-w-0",
                        div { class: "text-[11px] text-muted-foreground", "Das ist eine Reaktion auf" }
                        div { class: "line-clamp-1 font-medium", "{o.title}" }
                        if !o.author.is_empty() { div { class: "text-xs text-muted-foreground", "{o.author}" } }
                    }
                }
            }
            if open() {
                div { class: "space-y-4 border-t px-3 py-3 text-sm",
                    if let Some(t) = &r.info_text { p { class: "whitespace-pre-line text-muted-foreground", "{t}" } }
                    if r.has_rules {
                        div { class: "grid gap-4 sm:grid-cols-2",
                            div {
                                h4 { class: "mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground", "Im Stream" }
                                ul { class: "space-y-1",
                                    match r.stream.sponsor_skips_allowed {
                                        Some(true) => rsx! { Rule { ok: true, "Placements dürfen übersprungen werden" } },
                                        Some(false) => rsx! { Rule { ok: false, "Placements nicht überspringen" } },
                                        None => rsx! {},
                                    }
                                    if r.stream.credit_chat { Rule { ok: false, "Link zum Video in den Chat posten" } }
                                    for c in r.stream.custom.iter() { Rule { ok: false, "{c}" } }
                                    if r.stream.sponsor_skips_allowed.is_none() && !r.stream.credit_chat && r.stream.custom.is_empty() {
                                        li { class: "text-muted-foreground", "Keine weiteren Bedingungen" }
                                    }
                                }
                            }
                            div {
                                h4 { class: "mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground", "Reaction-Video" }
                                ul { class: "space-y-1",
                                    match r.video.sponsor_cut_allowed {
                                        Some(true) => rsx! { Rule { ok: true, "Placements dürfen rausgeschnitten werden" } },
                                        Some(false) => rsx! { Rule { ok: false, "Placements nicht rausschneiden" } },
                                        None => rsx! {},
                                    }
                                    match r.video.monetization_allowed {
                                        Some(true) => rsx! { Rule { ok: true, "Monetarisieren erlaubt" } },
                                        Some(false) => rsx! { Rule { ok: false, "Nicht monetarisieren" } },
                                        None => rsx! {},
                                    }
                                    if r.video.credit_description { Rule { ok: false, "Link zum Video in die ersten 3 Zeilen der Beschreibung" } }
                                    if r.video.title_must_include { Rule { ok: false, "Kanalname in den Titel (z. B. „Reaktion auf …“)" } }
                                    if r.video.splitting_allowed == Some(false) { Rule { ok: false, "Reaktion nicht in mehrere Videos aufteilen" } }
                                    for c in r.video.custom.iter() { Rule { ok: false, "{c}" } }
                                }
                            }
                        }
                    }
                    if !r.sponsor.is_empty() || !r.tos.is_empty() {
                        div {
                            h4 { class: "mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground", "Abschnitte" }
                            div { class: "flex flex-wrap gap-2",
                                for s in r.sponsor.iter().cloned() {
                                    button { class: "rounded-md border border-amber-500/30 px-2 py-1 text-xs text-amber-300 hover:bg-amber-500/10",
                                        title: "Placement – nicht überspringen", onclick: move |_| *SEEK.write() = Some(s.start),
                                        "Placement {util::duration(s.start)}–{util::duration(s.end)}"
                                    }
                                }
                                for s in r.tos.iter().cloned() {
                                    button { class: "rounded-md border border-red-500/30 px-2 py-1 text-xs text-red-300 hover:bg-red-500/10",
                                        title: "Könnte gegen Plattform-Regeln verstossen", onclick: move |_| *SEEK.write() = Some(s.start),
                                        "TOS {util::duration(s.start)}–{util::duration(s.end)}"
                                    }
                                }
                            }
                        }
                    }
                    if !r.reactions.is_empty() {
                        div {
                            h4 { class: "mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground",
                                "Reaktionen"
                                if views > 0 { span { class: "ml-1 normal-case tracking-normal", "· {util::compact(views)} Aufrufe" } }
                            }
                            div { class: "flex flex-col gap-1.5",
                                for x in r.reactions.iter() {
                                    a { href: "{x.url}", target: "_blank", rel: "noreferrer noopener",
                                        class: "flex items-center gap-2 rounded-lg p-1.5 hover:bg-accent/50",
                                        if !x.thumb.is_empty() { img { class: "aspect-video w-20 shrink-0 rounded object-cover", src: "{x.thumb}", alt: "" } }
                                        div { class: "min-w-0 flex-1",
                                            div { class: "line-clamp-1 text-xs font-medium", "{x.title}" }
                                            div { class: "text-[11px] text-muted-foreground", "{x.channel}" if x.views > 0 { " · {util::compact(x.views)} Aufrufe" } if !x.source.is_empty() { " · {x.source}" } }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    p { class: "text-[11px] text-muted-foreground",
                        "Angaben des Kanals über "
                        a { href: "https://canireact.com/de", target: "_blank", rel: "noreferrer noopener", class: "underline", "Can I React" }
                        ". Die Richtlinien sind nicht rechtlich bindend und dienen nur als Leitfaden."
                    }
                }
            }
        }
    }
}

#[component]
fn Rule(ok: bool, children: Element) -> Element {
    rsx! {
        li { class: "flex items-start gap-2",
            if ok { icons::Check { class: "mt-0.5 size-3.5 shrink-0 text-green-400" } } else { icons::Info { class: "mt-0.5 size-3.5 shrink-0 text-amber-400" } }
            span { {children} }
        }
    }
}
