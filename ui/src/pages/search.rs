//! Suche mit Filtern (Sortierung, Datum, Art, Länge) und Nachladen.

use crate::Route;
use crate::api;
use crate::components::*;
use crate::state::SETTINGS;
use dioxus::prelude::*;
use nrztube_types::*;

#[component]
pub fn Search(q: String, sort: String, date: String, kind: String, duration: String) -> Element {
    let nav = navigator();
    let mut items = use_signal(Vec::<Item>::new);
    let mut cont = use_signal(|| None::<String>);
    let mut loading = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let key = (q.clone(), sort.clone(), date.clone(), kind.clone(), duration.clone());
    use_effect(use_reactive!(|key| {
        let (q, sort, date, kind, duration) = key.clone();
        items.set(vec![]);
        cont.set(None);
        error.set(None);
        if q.trim().is_empty() {
            return;
        }
        loading.set(true);
        spawn(async move {
            let url = format!("/api/search?q={}&sort={sort}&date={date}&type={kind}&duration={duration}", api::enc(&q));
            match api::get::<Page<Item>>(&url).await {
                Ok(p) => { items.set(p.items); cont.set(p.continuation); }
                Err(e) => error.set(Some(e.message)),
            }
            loading.set(false);
        });
    }));
    let more = move |_| {
        let Some(c) = cont() else { return };
        loading.set(true);
        spawn(async move {
            if let Ok(p) = api::get::<Page<Item>>(&format!("/api/search?cont={}", api::enc(&c))).await {
                items.write().extend(p.items);
                cont.set(p.continuation);
            }
            loading.set(false);
        });
    };
    let (sort0, date0, kind0, duration0, q0) = (sort.clone(), date.clone(), kind.clone(), duration.clone(), q.clone());
    let set = move |field: &'static str, value: String| {
        let (sort, date, kind, duration, q) = (&sort0, &date0, &kind0, &duration0, &q0);
        let (mut s, mut d, mut k, mut du) = (sort.clone(), date.clone(), kind.clone(), duration.clone());
        match field {
            "sort" => s = value,
            "date" => d = value,
            "kind" => k = value,
            _ => du = value,
        }
        nav.push(Route::Search { q: q.clone(), sort: s, date: d, kind: k, duration: du });
    };
    let set2 = set.clone();
    let set3 = set.clone();
    let set4 = set.clone();
    let opts = |list: &[(&str, &str)]| list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    rsx! {
        div { class: "mx-auto max-w-[1800px] px-3 py-4 md:p-6",
            div { class: "shelf -mx-3 mb-5 flex items-center gap-2 overflow-x-auto px-3 md:mx-0 md:flex-wrap md:px-0",
                Select { value: sort.clone(), options: opts(&[("", "Relevanz"), ("date", "Neueste"), ("views", "Meiste Aufrufe"), ("rating", "Bewertung")]), onchange: move |v| set("sort", v) }
                Select { value: date.clone(), options: opts(&[("", "Jederzeit"), ("hour", "Letzte Stunde"), ("today", "Heute"), ("week", "Diese Woche"), ("month", "Dieser Monat"), ("year", "Dieses Jahr")]), onchange: move |v| set2("date", v) }
                Select { value: kind.clone(), options: opts(&[("", "Alle Arten"), ("video", "Videos"), ("channel", "Kanäle"), ("playlist", "Playlists"), ("movie", "Filme")]), onchange: move |v| set3("kind", v) }
                Select { value: duration.clone(), options: opts(&[("", "Jede Länge"), ("short", "Unter 4 Minuten"), ("medium", "4–20 Minuten"), ("long", "Über 20 Minuten")]), onchange: move |v| set4("duration", v) }
            }
            if kind.is_empty() || kind == "channel" {
                TwitchSearchRow { key: "{q}", q: q.clone() }
            }
            if let Some(e) = error() {
                ErrorBox { message: e }
            } else if items().is_empty() && loading() {
                Loading {}
            } else if items().is_empty() && !q.trim().is_empty() {
                EmptyState { title: "Keine Treffer", text: format!("Für „{q}“ wurde nichts gefunden.") }
            } else {
                ItemGrid { items: items() }
                if cont().is_some() { LoadMore { loading: loading(), onclick: more } }
            }
        }
    }
}

/// Passende Twitch-Kanäle über den YouTube-Treffern (nur gute Treffer)
#[component]
fn TwitchSearchRow(q: String) -> Element {
    let q2 = q.clone();
    let res = use_resource(use_reactive!(|q2| async move {
        if !SETTINGS.peek().twitch_enabled || q2.trim().len() < 3 {
            return vec![];
        }
        api::get::<Vec<TwitchChannel>>(&format!("/api/twitch/search?q={}", api::enc(&q2))).await.unwrap_or_default()
    }));
    let ql: String = q.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    // Exakt passend, live oder gross
    let list: Vec<TwitchChannel> = res()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.login.contains(&ql) || ql.contains(&c.login))
        .filter(|c| c.live.is_some() || c.followers.unwrap_or(0) >= if c.login == ql { 1_000 } else { 10_000 })
        .collect();
    let mut list = list;
    list.sort_by_key(|c| std::cmp::Reverse(c.live.is_some() as u64 * 1_000_000_000 + c.followers.unwrap_or(0)));
    list.truncate(3);
    if list.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "mb-6 flex flex-col gap-2",
            for c in list {
                Link { key: "{c.login}", to: Route::TwitchChannelPage { login: c.login.clone(), tab: String::new() },
                    class: "flex items-center gap-3 rounded-xl border bg-card p-3 transition-colors hover:bg-accent/40",
                    span { class: "relative shrink-0",
                        img { class: if c.live.is_some() { "size-12 rounded-full object-cover ring-2 ring-red-500" } else { "size-12 rounded-full object-cover" }, src: "{c.avatar}", alt: "" }
                    }
                    div { class: "min-w-0 flex-1",
                        div { class: "flex items-center gap-1.5 font-medium", "{c.name}" TwitchGlyph { class: "size-3.5 text-[#a970ff]" } }
                        div { class: "truncate text-xs text-muted-foreground",
                            if let Some(st) = &c.live { span { class: "font-medium text-red-400", "LIVE " } "{st.title}" }
                            else if let Some(f) = c.followers { "Twitch • {crate::util::compact(f)} Follower" }
                        }
                    }
                    if let Some(st) = &c.live {
                        span { class: "hidden shrink-0 text-xs text-muted-foreground tabular-nums sm:block", "{crate::util::compact(st.viewers)} Zuschauer" }
                    }
                    icons::ChevronRight { class: "size-4 shrink-0 text-muted-foreground" }
                }
            }
        }
    }
}
