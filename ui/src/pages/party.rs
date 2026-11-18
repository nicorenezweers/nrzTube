//! Watch-Party (gemeinsam schauen): Raum erstellen, per Link einladen (auch Gäste ohne Konto),
//! Wiedergabe für alle gleich halten, Warteschlange und Chat.
//!
//! Abgleich: Der Server kennt Position, Zustand und Zeitpunkt. Jeder Teilnehmer rechnet die
//! Soll-Position aus und springt nur bei mehr als 1,5 s Abweichung. Eigene Play/Pause/Sprünge
//! gehen als Aktion an den Server – ausser direkt nach einem Abgleich (sonst Echo).

use crate::Route;
use crate::api;
use crate::components::*;
use crate::player::PlayerHost;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::util;
use dioxus::prelude::*;
use nrztube_types::*;
use wasm_bindgen::JsCast;

/// Aktiver Raum (ID) – der Player holt dann die Streams über die Party
pub static PARTY: GlobalSignal<Option<String>> = Signal::global(|| None);
pub static PARTY_STATE: GlobalSignal<Option<PartyState>> = Signal::global(|| None);
static PARTY_CHAT: GlobalSignal<Vec<PartyChatMsg>> = Signal::global(Vec::new);
/// Bis wann eigene Player-Ereignisse nicht gemeldet werden (ms, nach einem Abgleich)
static APPLY_UNTIL: GlobalSignal<f64> = Signal::global(|| 0.0);
/// Uhrenabgleich: Serverzeit minus eigene Zeit (ms)
static OFFSET: GlobalSignal<f64> = Signal::global(|| 0.0);
/// Browser hat Abspielen ohne Klick blockiert
static NEED_TAP: GlobalSignal<bool> = Signal::global(|| false);

fn now_ms() -> f64 {
    js_sys::Date::now()
}

fn video_el() -> Option<web_sys::HtmlVideoElement> {
    web_sys::window()?.document()?.get_element_by_id("nrz-video")?.dyn_into().ok()
}

fn expected(s: &PartyState) -> f64 {
    if s.playing { s.position + (now_ms() + OFFSET() - s.at as f64) / 1000.0 } else { s.position }
}

/// Darf ich steuern?
/// Meine Rechte im Raum (Gastgeber: alle)
fn my_perms(s: &PartyState) -> PartyPerms {
    s.members.iter().find(|m| m.id == s.me).map(|m| if m.host { PartyPerms::ALL } else { m.perms }).unwrap_or_default()
}

fn can_control(s: &PartyState) -> bool {
    my_perms(s).control
}

/// Player an den Raum angleichen
fn apply_state(s: &PartyState) {
    let Some(v) = s.video.clone() else { return };
    let same = NOW.peek().as_ref().map(|n| n.id == v.id).unwrap_or(false);
    if !same {
        // Neues Video: Angaben setzen und Player umschalten; Ereignisse beim Laden nicht melden
        *APPLY_UNTIL.write() = now_ms() + 5000.0;
        *DETAILS.write() = Some(VideoDetails {
            id: v.id.clone(),
            title: v.title.clone(),
            author: v.author.clone(),
            author_id: v.author_id.clone(),
            author_thumb: v.author_thumb.clone(),
            thumb: v.thumb.clone(),
            duration: v.duration,
            live: v.live,
            ..Default::default()
        });
        *NOW.write() = Some(NowPlaying { id: v.id.clone(), list: None, local_list: None, start: expected(s).max(0.0) });
        return;
    }
    let Some(el) = video_el() else { return };
    if el.ready_state() < 1 {
        return;
    }
    let want = expected(s);
    let live = v.live;
    let drift = (el.current_time() - want).abs();
    if !live && drift > 1.5 && want.is_finite() {
        *APPLY_UNTIL.write() = now_ms() + 1200.0;
        el.set_current_time(want.max(0.0));
    }
    if s.playing && el.paused() {
        *APPLY_UNTIL.write() = now_ms() + 1200.0;
        spawn(async move {
            let ok = document::eval("const v=document.getElementById('nrz-video'); if(!v) return true; return await v.play().then(()=>true).catch(()=>false);")
                .join::<bool>()
                .await
                .unwrap_or(true);
            *NEED_TAP.write() = !ok;
        });
    } else if !s.playing && !el.paused() {
        *APPLY_UNTIL.write() = now_ms() + 1200.0;
        let _ = el.pause();
    }
}

fn send(kind: &str, position: f64, video: Option<VideoItem>, text: String, index: usize, flag: bool) {
    send_action(PartyAction { kind: kind.to_string(), position, video, text, index, flag, ..Default::default() });
}

/// Einstellungen/Rechte (nur Gastgeber)
fn send_action(a: PartyAction) {
    let Some(id) = PARTY.peek().clone() else { return };
    spawn(async move {
        if let Err(e) = api::post::<_, serde_json::Value>(&format!("/api/party/{id}/action"), &a).await {
            toast_err(e);
            // Server-Zustand wiederherstellen
            if let Some(s) = PARTY_STATE.peek().clone() {
                apply_state(&s);
            }
        }
    });
}

/// Vom Player: Abspielen/Pause/Sprung/Ende (wird nur in einer Party gemeldet)
pub fn local_event(kind: &str, position: f64) {
    if PARTY.peek().is_none() || now_ms() < *APPLY_UNTIL.peek() {
        return;
    }
    let Some(s) = PARTY_STATE.peek().clone() else { return };
    let Some(v) = s.video.clone() else { return };
    if NOW.peek().as_ref().map(|n| n.id != v.id).unwrap_or(true) {
        return;
    }
    if kind == "ended" {
        send("ended", position, None, v.id.clone(), 0, false);
        return;
    }
    if !can_control(&s) {
        // Nicht berechtigt: zurück auf den Stand des Raums
        *APPLY_UNTIL.write() = now_ms() + 1200.0;
        apply_state(&s);
        return;
    }
    // Sprung während des Abspielens = "play" an neuer Stelle
    let kind = if kind == "seek" && s.playing { "play" } else { kind };
    // Nichts Neues (z. B. play, obwohl schon läuft und kaum Abweichung)
    if kind == "play" && s.playing && (expected(&s) - position).abs() < 1.5 {
        return;
    }
    if kind == "pause" && !s.playing {
        return;
    }
    send(kind, position, None, String::new(), 0, false);
}

/// In die Warteschlange der aktiven Party (aus Kontextmenü usw.)
pub fn enqueue(v: VideoItem) {
    if PARTY.peek().is_none() {
        return;
    }
    send("queue", 0.0, Some(v), String::new(), 0, false);
    toast("In die Watch-Party eingereiht");
}

/// Neue Party erstellen (optional mit Video) und hingehen
pub fn start_party(video: Option<VideoItem>) {
    start_party_with(video, false, String::new());
}

pub fn start_party_with(video: Option<VideoItem>, public: bool, title: String) {
    spawn(async move {
        match api::post::<_, serde_json::Value>("/api/party", &PartyCreate { video, title, public }).await {
            Ok(v) => {
                let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let k = v.get("invite").and_then(|x| x.as_str()).unwrap_or("").to_string();
                navigator().push(Route::PartyPage { id, k });
            }
            Err(e) => toast_err(e),
        }
    });
}

fn invite_link(s: &PartyState) -> String {
    format!("{}/party/{}?k={}", util::origin(), s.id, s.invite)
}

// ------------------------------------------------------------------ Seiten

/// Watch-Partys: öffentliche finden, eigene sehen, neue starten
#[component]
pub fn PartyStart() -> Element {
    let mine = use_resource(|| async { api::get::<Vec<PartyInfo>>("/api/party/mine").await.unwrap_or_default() });
    let public = use_resource(|| async { api::get::<Vec<PartyInfo>>("/api/party/public").await.unwrap_or_default() });
    let mut is_public = use_signal(|| false);
    let mut title = use_signal(String::new);
    rsx! {
        div { class: "mx-auto max-w-5xl px-4 py-6 md:px-6",
            div { class: "mb-6 flex flex-wrap items-end gap-4 rounded-2xl border bg-card p-5",
                div { class: "min-w-0 flex-1",
                    h1 { class: "text-xl font-semibold tracking-tight", "Watch-Partys" }
                    p { class: "mt-1 max-w-lg text-sm text-muted-foreground", "Zusammen schauen – synchron, mit Warteschlange und Chat. Öffentliche Partys findet hier jeder, private nur mit Einladungslink." }
                }
                div { class: "flex w-full flex-wrap items-center gap-2 sm:w-auto",
                    input { class: "h-9 min-w-0 flex-1 rounded-lg border border-input bg-input/30 px-3 text-sm outline-none sm:w-56", placeholder: "Titel (optional)", value: "{title}", oninput: move |e| title.set(e.value()) }
                    label { class: "flex items-center gap-2 text-sm",
                        Toggle { checked: is_public(), onchange: move |v| is_public.set(v) }
                        "Öffentlich"
                    }
                    Button { onclick: move |_| start_party_with(None, is_public(), title()), icons::Plus {} "Starten" }
                }
            }
            if let Some(list) = mine().filter(|l| !l.is_empty()) {
                h2 { class: "mb-3 text-sm font-semibold text-muted-foreground", "Deine Partys" }
                PartyGrid { list }
            }
            h2 { class: "mb-3 mt-6 text-sm font-semibold text-muted-foreground", "Öffentliche Partys" }
            match public() {
                None => rsx! { Loading {} },
                Some(list) if list.is_empty() => rsx! { p { class: "rounded-xl border border-dashed p-6 text-center text-sm text-muted-foreground", "Gerade läuft keine öffentliche Party. Starte eine und schalte „Öffentlich“ ein." } },
                Some(list) => rsx! { PartyGrid { list } },
            }
        }
    }
}

#[component]
fn PartyGrid(list: Vec<PartyInfo>) -> Element {
    rsx! {
        div { class: "grid gap-3 sm:grid-cols-2 lg:grid-cols-3",
            for p in list {
                Link { key: "{p.id}", to: Route::PartyPage { id: p.id.clone(), k: String::new() },
                    class: "group overflow-hidden rounded-xl border bg-card transition-colors hover:bg-accent/40",
                    div { class: "relative aspect-video bg-muted",
                        if let Some(v) = &p.video { img { class: "absolute inset-0 size-full object-cover", src: "{v.thumb}", alt: "", loading: "lazy" } }
                        span { class: "absolute left-2 top-2 flex items-center gap-1 rounded bg-black/70 px-1.5 py-0.5 text-[11px] text-white",
                            span { class: "size-1.5 rounded-full bg-green-500" } "{p.members} online"
                        }
                        if p.public { span { class: "absolute right-2 top-2 rounded bg-black/70 px-1.5 py-0.5 text-[11px] text-white", "öffentlich" } }
                    }
                    div { class: "p-3",
                        div { class: "truncate text-sm font-medium", "{p.title}" }
                        div { class: "truncate text-xs text-muted-foreground",
                            if let Some(v) = &p.video { "{v.title} • " }
                            "Gastgeber {p.host}"
                        }
                    }
                }
            }
        }
    }
}

/// Party-Seite (eigener Rahmen, auch ohne Konto): Mitglieder sehen den Raum, angemeldete
/// Nutzer treten mit dem Schlüssel direkt bei, Gäste geben einen Namen ein.
#[component]
pub fn PartyPage(id: String, k: String) -> Element {
    let logged_in = AUTH().and_then(|a| a.me).is_some();
    let key = (id.clone(), k.clone(), logged_in);
    let mut info: Resource<Result<PartyInfo, api::ApiError>> = use_resource(use_reactive!(|key| async move {
        let (id, k, logged_in) = key;
        let i = api::get::<PartyInfo>(&format!("/api/party/{id}?k={}", api::enc(&k))).await?;
        // Mit Konto: gleich beitreten
        if !i.joined && logged_in {
            api::post::<_, serde_json::Value>(&format!("/api/party/{id}/join"), &PartyJoin { k: k.clone(), name: String::new() }).await?;
            return Ok(PartyInfo { joined: true, ..i });
        }
        Ok(i)
    }));
    let mut name = use_signal(|| util::window().local_storage().ok().flatten().and_then(|s| s.get_item("nrzGuestName").ok().flatten()).unwrap_or_default());
    let mut busy = use_signal(|| false);
    let mut err = use_signal(|| None::<String>);
    let field = "h-10 w-full rounded-md border border-input bg-input/30 px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50";
    let join_id = id.clone();
    let join_k = k.clone();
    let content = match info() {
        None => rsx! { Loading { label: "Watch-Party wird geladen …".to_string() } },
        Some(Err(e)) => rsx! { ErrorBox { message: e.message } },
        Some(Ok(i)) if i.joined => rsx! {
            PlayerHost { big: true }
            PartyRoom { id: id.clone(), guest: !logged_in }
        },
        Some(Ok(i)) => rsx! {
            div { class: "flex items-center justify-center p-4 pt-10",
                form { class: "w-full max-w-sm overflow-hidden rounded-2xl border bg-card shadow-xl",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let n = name().trim().to_string();
                        if n.is_empty() { err.set(Some("Bitte einen Namen eingeben".into())); return; }
                        if let Some(s) = util::window().local_storage().ok().flatten() { let _ = s.set_item("nrzGuestName", &n); }
                        busy.set(true);
                        let (id, k) = (join_id.clone(), join_k.clone());
                        spawn(async move {
                            match api::post::<_, serde_json::Value>(&format!("/api/party/{id}/join"), &PartyJoin { k, name: n }).await {
                                Ok(_) => info.restart(),
                                Err(e) => err.set(Some(e.message)),
                            }
                            busy.set(false);
                        });
                    },
                    div { class: "flex aspect-video w-full items-center justify-center bg-gradient-to-br from-secondary to-card",
                        icons::Users { class: "size-12 text-muted-foreground/60" }
                    }
                    div { class: "p-5",
                        p { class: "text-xs text-muted-foreground", "{i.host} lädt dich ein" }
                        h1 { class: "mb-1 text-lg font-semibold", "{i.title}" }
                        if let Some(v) = &i.video { p { class: "mb-4 line-clamp-2 text-sm text-muted-foreground", "Gerade: {v.title}" } }
                        p { class: "mb-4 text-xs text-muted-foreground", "{i.members} schauen gerade mit" }
                        label { class: "mb-1 block text-xs font-medium", "Dein Name" }
                        input { class: "{field} mb-3", value: "{name}", maxlength: "24", autofocus: true, placeholder: "z. B. Alex", oninput: move |e| name.set(e.value()) }
                        if let Some(e) = err() { p { class: "mb-3 text-sm text-destructive", "{e}" } }
                        Button { class: "w-full", button_type: "submit", disabled: busy(),
                            if busy() { icons::LoaderCircle { class: "animate-spin" } } else { icons::Users {} }
                            "Als Gast mitschauen"
                        }
                        a { href: "/", class: "mt-3 block text-center text-xs text-muted-foreground hover:text-foreground", "Ich habe ein nrzTube-Konto – anmelden" }
                    }
                }
            }
        },
    };
    rsx! {
        div { class: "flex h-dvh flex-col overflow-hidden",
            header { class: "glass z-40 flex h-14 shrink-0 items-center gap-2 border-b px-3 md:px-5",
                if logged_in {
                    Link { to: Route::Home {}, class: "flex items-center gap-2 rounded-md px-1 py-1 hover:bg-accent", title: "Zurück zu nrzTube",
                        icons::ChevronLeft { class: "size-4 text-muted-foreground" }
                        span { class: "flex size-7 items-center justify-center rounded-lg bg-primary text-primary-foreground", icons::Play { class: "size-4 fill-current" } }
                        span { class: "font-semibold", "nrzTube" }
                    }
                } else {
                    span { class: "flex size-7 items-center justify-center rounded-lg bg-primary text-primary-foreground", icons::Play { class: "size-4 fill-current" } }
                    span { class: "font-semibold", "nrzTube" }
                }
                span { class: "ml-1 rounded-full border px-2 py-0.5 text-[11px] text-muted-foreground", "Watch-Party" }
                if !logged_in {
                    a { href: "/", class: "ml-auto text-xs text-muted-foreground hover:text-foreground", "Anmelden" }
                }
            }
            main { id: "main-scroll", class: "min-w-0 flex-1 overflow-y-auto pb-8", {content} }
        }
    }
}

/// Der Raum: Verbindung, Abgleich, Bedienfelder
#[component]
fn PartyRoom(id: String, guest: bool) -> Element {
    let mut closed = use_signal(|| false);
    let rid = id.clone();
    use_hook(move || {
        *PARTY.write() = Some(rid.clone());
        *PARTY_CHAT.write() = vec![];
    });
    // Seite verlassen: Verbindung schliessen, Party bleibt aktiv (Einreihen aus der Suche)
    use_drop(|| {
        util::eval("if (window.__nrzParty) { window.__nrzParty.close(); window.__nrzParty = null; }");
    });
    // Ereignisse empfangen
    let eid = id.clone();
    use_future(move || {
        let eid = eid.clone();
        async move {
            let js = format!(
                "if (window.__nrzParty) window.__nrzParty.close();\
                 const es = new EventSource({});\
                 window.__nrzParty = es;\
                 es.onmessage = (e) => dioxus.send(e.data);\
                 es.onerror = () => dioxus.send('__err');\
                 await new Promise(() => {{}});",
                util::js_str(&format!("/api/party/{eid}/events"))
            );
            let mut ev = document::eval(&js);
            while let Ok(msg) = ev.recv::<String>().await {
                if msg == "__err" {
                    continue;
                }
                match serde_json::from_str::<PartyEvent>(&msg) {
                    Ok(PartyEvent::State { s }) => {
                        *OFFSET.write() = s.now as f64 - now_ms();
                        apply_state(&s);
                        *PARTY_STATE.write() = Some(s);
                    }
                    Ok(PartyEvent::Chat { m }) => {
                        let mut c = PARTY_CHAT.write();
                        c.push(m);
                        let n = c.len();
                        if n > 150 {
                            c.drain(0..n - 120);
                        }
                    }
                    Ok(PartyEvent::Closed) => {
                        closed.set(true);
                        util::eval("if (window.__nrzParty) { window.__nrzParty.close(); window.__nrzParty = null; }");
                        if let Some(v) = video_el() { let _ = v.pause(); }
                        return;
                    }
                    Err(_) => {}
                }
            }
        }
    });
    // Regelmässig nachziehen (Abweichungen, gerade geladene Videos)
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(1500).await;
            if let Some(s) = PARTY_STATE.peek().clone() {
                apply_state(&s);
            }
        }
    });

    if closed() {
        return rsx! { EmptyState { title: "Die Watch-Party ist beendet".to_string(), text: "Der Gastgeber hat die Party geschlossen.".to_string() } };
    }
    let Some(s) = PARTY_STATE() else { return rsx! { Loading { label: "Verbinde …".to_string() } } };
    let me_host = s.members.iter().any(|m| m.id == s.me && m.host);
    let control = can_control(&s);
    let online = s.members.iter().filter(|m| m.online).count();
    let link = invite_link(&s);
    let link2 = link.clone();
    let link3 = link.clone();
    let lid = id.clone();
    let s_title = s.title.clone();
    rsx! {
        div { class: "mx-auto max-w-6xl px-4 py-4 md:px-6",
            if NEED_TAP() {
                button { class: "mb-4 flex w-full items-center justify-center gap-2 rounded-xl bg-primary px-4 py-3 text-sm font-medium text-primary-foreground",
                    onclick: move |_| {
                        *NEED_TAP.write() = false;
                        if let Some(s) = PARTY_STATE.peek().clone() { apply_state(&s); }
                        util::eval("const v=document.getElementById('nrz-video'); v && v.play();");
                    },
                    icons::Play { class: "size-4 fill-current" } "Tippen, um mitzuschauen (der Browser hat automatisches Abspielen blockiert)"
                }
            }
            // Kopf
            div { class: "mb-4 flex flex-wrap items-center gap-3",
                div { class: "min-w-0 flex-1",
                    div { class: "flex items-center gap-2 text-xs text-muted-foreground",
                        span { class: "flex items-center gap-1 rounded-full bg-green-500/15 px-2 py-0.5 font-medium text-green-400", span { class: "size-1.5 rounded-full bg-green-500" } "Watch-Party" }
                        "{online} online"
                        if s.public { " • öffentlich" }
                        if !control { " • Steuern ist für dich gesperrt" }
                    }
                    h1 { class: "mt-1 truncate text-lg font-semibold", "{s_title}" }
                    if let Some(v) = &s.video { p { class: "truncate text-sm text-muted-foreground", "{v.title} • {v.author}" } }
                }
                Button { variant: ButtonVariant::Secondary, size: ButtonSize::Sm,
                    onclick: move |_| { util::copy(&link); toast("Einladungslink kopiert – einfach an Freunde schicken"); },
                    icons::Link {} "Einladen"
                }
                Button { variant: ButtonVariant::Ghost, size: ButtonSize::Sm,
                    onclick: move |_| {
                        let id = lid.clone();
                        spawn(async move {
                            let _ = api::post::<_, serde_json::Value>(&format!("/api/party/{id}/leave"), &()).await;
                            *PARTY.write() = None;
                            *PARTY_STATE.write() = None;
                            if let Some(v) = video_el() { let _ = v.pause(); }
                            if guest { util::window().location().set_href("/").ok(); } else { navigator().push(Route::PartyStart {}); }
                            *NOW.write() = None;
                        });
                    },
                    icons::LogOut {} if me_host { "Party beenden" } else { "Verlassen" }
                }
            }
            div { class: "grid gap-6 lg:grid-cols-[minmax(0,1fr)_340px]",
                div { class: "min-w-0 space-y-6",
                    AddVideo { id: id.clone(), guest, perms: my_perms(&s) }
                    Queue { s: s.clone(), perms: my_perms(&s) }
                    if s.video.is_none() {
                        EmptyState { title: "Noch kein Video".to_string(), text: "Füge oben einen YouTube- oder Twitch-Link ein – oder wähle über die Suche ein Video und „In Watch-Party einreihen“.".to_string() }
                    }
                }
                aside { class: "min-w-0 space-y-4",
                    Members { s: s.clone(), host: me_host }
                    PartyChat { can_chat: my_perms(&s).chat }
                    if me_host {
                        HostSettings { s: s.clone() }
                    }
                    div { class: "rounded-xl border bg-card p-3 text-xs text-muted-foreground",
                        div { class: "mb-1.5 font-medium text-foreground", "Einladungslink" }
                        div { class: "flex items-center gap-2",
                            input { class: "h-8 min-w-0 flex-1 rounded-md border border-input bg-input/30 px-2 text-xs", readonly: true, value: "{link3}" }
                            Button { variant: ButtonVariant::Outline, size: ButtonSize::IconSm, title: "Kopieren", onclick: move |_| { util::copy(&link2); toast("Kopiert"); }, icons::Copy {} }
                        }
                        p { class: "mt-1.5", "Wer den Link hat, kann mitschauen – auch ohne nrzTube-Konto." }
                    }
                }
            }
        }
    }
}

#[component]
fn AddVideo(id: String, guest: bool, perms: PartyPerms) -> Element {
    let mut input = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut add = move |now_play: bool| {
        let text = input().trim().to_string();
        if text.is_empty() {
            return;
        }
        busy.set(true);
        let id = id.clone();
        spawn(async move {
            match api::get::<VideoItem>(&format!("/api/party/{id}/resolve?k={}", api::enc(&text))).await {
                Ok(v) => {
                    input.set(String::new());
                    if now_play {
                        send("load", 0.0, Some(v), String::new(), 0, false);
                    } else {
                        send("queue", 0.0, Some(v), String::new(), 0, false);
                    }
                }
                Err(e) => toast_err(e),
            }
            busy.set(false);
        });
    };
    let mut add2 = add.clone();
    if !perms.queue && !perms.control {
        return rsx! {
            div { class: "flex items-center gap-2 rounded-xl border border-dashed p-3 text-sm text-muted-foreground",
                icons::Lock { class: "size-4" } "Videos hinzufügen ist für dich gesperrt – der Gastgeber kann es freigeben."
            }
        };
    }
    rsx! {
        form { class: "flex flex-wrap items-center gap-2 rounded-xl border bg-card p-3",
            onsubmit: move |e| { e.prevent_default(); add(false); },
            icons::Plus { class: "size-4 shrink-0 text-muted-foreground" }
            input { class: "h-9 min-w-0 flex-1 rounded-lg border border-input bg-input/30 px-3 text-sm outline-none focus:border-ring",
                placeholder: "YouTube- oder Twitch-Link einfügen", value: "{input}", oninput: move |e| input.set(e.value()) }
            Button { size: ButtonSize::Sm, variant: ButtonVariant::Secondary, disabled: busy() || !perms.queue, button_type: "submit", "Einreihen" }
            Button { size: ButtonSize::Sm, disabled: busy() || !perms.control, onclick: move |e: MouseEvent| { e.prevent_default(); add2(true); },
                if busy() { icons::LoaderCircle { class: "animate-spin" } } else { icons::Play { class: "fill-current" } }
                "Jetzt abspielen"
            }
            if !guest {
                p { class: "w-full text-xs text-muted-foreground", "Oder in der Suche / auf Kanälen per Rechtsklick → „In Watch-Party einreihen“." }
            }
        }
    }
}

#[component]
fn Queue(s: PartyState, perms: PartyPerms) -> Element {
    let control = perms.control;
    let manage = perms.manage;
    if s.queue.is_empty() {
        return rsx! {};
    }
    rsx! {
        section {
            div { class: "mb-2 flex items-center gap-2",
                h2 { class: "text-sm font-semibold", "Als Nächstes" }
                span { class: "text-xs text-muted-foreground", "{s.queue.len()}" }
                if control {
                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::Sm, class: "ml-auto", onclick: move |_| send("next", 0.0, None, String::new(), 0, false),
                        icons::SkipForward {} "Weiter"
                    }
                }
            }
            div { class: "flex flex-col gap-2",
                for (i, v) in s.queue.iter().enumerate() {
                    div { key: "{i}-{v.id}", class: "group flex items-center gap-3 rounded-xl border bg-card p-2",
                        img { class: "aspect-video w-28 shrink-0 rounded-lg bg-muted object-cover", src: "{v.thumb}", alt: "" }
                        div { class: "min-w-0 flex-1",
                            div { class: "line-clamp-2 text-sm font-medium", "{v.title}" }
                            div { class: "truncate text-xs text-muted-foreground", "{v.author}" }
                        }
                        if manage {
                            Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Jetzt abspielen", onclick: move |_| send("playnow", 0.0, None, String::new(), i, false), icons::Play { class: "fill-current" } }
                            Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Entfernen", onclick: move |_| send("remove", 0.0, None, String::new(), i, false), icons::X {} }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Members(s: PartyState, host: bool) -> Element {
    let mut open = use_signal(|| None::<String>);
    rsx! {
        div { class: "rounded-xl border bg-card p-3",
            div { class: "mb-2 text-sm font-semibold", "Dabei ({s.members.len()})" }
            div { class: "flex flex-col gap-1",
                for m in s.members.iter() {
                    {
                        let mid = m.id.clone();
                        let is_open = open() == Some(m.id.clone());
                        let p = m.perms;
                        rsx! {
                            div { key: "{m.id}", class: "rounded-lg",
                                div { class: "flex items-center gap-2 py-1 text-sm",
                                    span { class: "flex size-7 shrink-0 items-center justify-center rounded-full bg-secondary text-xs font-semibold",
                                        {m.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()}
                                    }
                                    span { class: if m.online { "truncate" } else { "truncate text-muted-foreground" }, "{m.name}" }
                                    if m.id == s.me { span { class: "text-xs text-muted-foreground", "(du)" } }
                                    if m.host { span { class: "rounded bg-amber-500/15 px-1.5 text-[10px] font-medium text-amber-400", "Gastgeber" } }
                                    if m.guest { span { class: "rounded bg-secondary px-1.5 text-[10px] text-muted-foreground", "Gast" } }
                                    span { class: if m.online { "ml-auto size-1.5 shrink-0 rounded-full bg-green-500" } else { "ml-auto size-1.5 shrink-0 rounded-full bg-muted-foreground/40" } }
                                    if host && !m.host {
                                        {
                                            let mid2 = mid.clone();
                                            rsx! {
                                                button { class: if is_open { "rounded p-1 text-foreground" } else { "rounded p-1 text-muted-foreground hover:text-foreground" }, title: "Rechte",
                                                    onclick: move |_| open.set(if is_open { None } else { Some(mid2.clone()) }),
                                                    icons::Shield { class: "size-3.5" }
                                                }
                                            }
                                        }
                                    }
                                }
                                if is_open {
                                    div { class: "mb-2 ml-9 space-y-1 rounded-lg bg-secondary/50 p-2 text-xs",
                                        PermRow { label: "Steuern", on: p.control, onchange: { let id = mid.clone(); move |v| send_action(PartyAction { kind: "perms".into(), text: id.clone(), perms: Some(PartyPerms { control: v, ..p }), ..Default::default() }) } }
                                        PermRow { label: "Einreihen", on: p.queue, onchange: { let id = mid.clone(); move |v| send_action(PartyAction { kind: "perms".into(), text: id.clone(), perms: Some(PartyPerms { queue: v, ..p }), ..Default::default() }) } }
                                        PermRow { label: "Warteschlange verwalten", on: p.manage, onchange: { let id = mid.clone(); move |v| send_action(PartyAction { kind: "perms".into(), text: id.clone(), perms: Some(PartyPerms { manage: v, ..p }), ..Default::default() }) } }
                                        PermRow { label: "Chatten", on: p.chat, onchange: { let id = mid.clone(); move |v| send_action(PartyAction { kind: "perms".into(), text: id.clone(), perms: Some(PartyPerms { chat: v, ..p }), ..Default::default() }) } }
                                        div { class: "flex gap-2 pt-1",
                                            button { class: "rounded-md bg-background px-2 py-1 hover:bg-accent",
                                                onclick: { let id = mid.clone(); move |_| { send("host", 0.0, None, id.clone(), 0, false); open.set(None); } },
                                                "Zum Gastgeber machen"
                                            }
                                            button { class: "rounded-md bg-background px-2 py-1 text-red-400 hover:bg-accent",
                                                onclick: { let id = mid.clone(); move |_| { send("kick", 0.0, None, id.clone(), 0, false); open.set(None); } },
                                                "Entfernen"
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
fn PermRow(label: String, on: bool, onchange: EventHandler<bool>) -> Element {
    rsx! {
        div { class: "flex items-center justify-between gap-3",
            span { "{label}" }
            Toggle { checked: on, onchange: move |v| onchange.call(v) }
        }
    }
}

/// Gastgeber: öffentlich, Gäste, Standardrechte
#[component]
fn HostSettings(s: PartyState) -> Element {
    let d = s.default_perms;
    let set = move |p: PartyPerms| send_action(PartyAction { kind: "settings".into(), perms: Some(p), ..Default::default() });
    rsx! {
        div { class: "space-y-2 rounded-xl border bg-card p-3 text-sm",
            div { class: "font-semibold", "Party-Einstellungen" }
            PermRow { label: "Öffentlich (für alle auffindbar)", on: s.public, onchange: move |v| send_action(PartyAction { kind: "settings".into(), public: Some(v), ..Default::default() }) }
            PermRow { label: "Gäste ohne Konto erlauben", on: s.allow_guests, onchange: move |v| send_action(PartyAction { kind: "settings".into(), guests: Some(v), ..Default::default() }) }
            div { class: "pt-1 text-xs font-medium text-muted-foreground", "Rechte für neue Teilnehmer" }
            div { class: "space-y-1 text-xs",
                PermRow { label: "Steuern (Play, Pause, Springen)", on: d.control, onchange: move |v| set(PartyPerms { control: v, ..d }) }
                PermRow { label: "Videos einreihen", on: d.queue, onchange: move |v| set(PartyPerms { queue: v, ..d }) }
                PermRow { label: "Warteschlange verwalten", on: d.manage, onchange: move |v| set(PartyPerms { manage: v, ..d }) }
                PermRow { label: "Chatten", on: d.chat, onchange: move |v| set(PartyPerms { chat: v, ..d }) }
            }
            button { class: "w-full rounded-md bg-secondary px-2 py-1.5 text-xs hover:bg-accent",
                onclick: move |_| { send_action(PartyAction { kind: "settings".into(), perms: Some(d), flag: true, ..Default::default() }); toast("Rechte für alle übernommen"); },
                "Diese Rechte allen geben"
            }
        }
    }
}

#[component]
fn PartyChat(can_chat: bool) -> Element {
    let mut draft = use_signal(String::new);
    let msgs = PARTY_CHAT();
    rsx! {
        div { class: "flex h-80 flex-col overflow-hidden rounded-xl border bg-card",
            div { class: "flex h-10 shrink-0 items-center gap-2 border-b px-3 text-sm font-medium", icons::MessageSquare { class: "size-4 text-muted-foreground" } "Chat" }
            div { class: "flex min-h-0 flex-1 flex-col-reverse overflow-y-auto px-3 py-2",
                div { class: "flex flex-col gap-1",
                    if msgs.is_empty() { p { class: "py-4 text-center text-xs text-muted-foreground", "Noch keine Nachrichten" } }
                    for (i, m) in msgs.iter().enumerate() {
                        if m.system {
                            p { key: "{i}", class: "text-center text-[11px] text-muted-foreground", "{m.text}" }
                        } else {
                            div { key: "{i}", class: "break-words text-[13px] leading-snug",
                                span { class: "font-semibold", "{m.name}" }
                                span { class: "text-muted-foreground", ": " }
                                "{m.text}"
                            }
                        }
                    }
                }
            }
            form { class: "flex shrink-0 items-center gap-2 border-t p-2",
                onsubmit: move |e| {
                    e.prevent_default();
                    let t = draft().trim().to_string();
                    if t.is_empty() { return; }
                    draft.set(String::new());
                    send("chat", 0.0, None, t, 0, false);
                },
                input { class: "h-9 min-w-0 flex-1 rounded-lg border border-input bg-input/30 px-3 text-sm outline-none focus:border-ring disabled:opacity-50", placeholder: if can_chat { "Nachricht …" } else { "Chatten ist für dich gesperrt" }, disabled: !can_chat, maxlength: "500", value: "{draft}", oninput: move |e| draft.set(e.value()) }
                Button { size: ButtonSize::IconSm, button_type: "submit", disabled: draft().trim().is_empty(), title: "Senden", icons::Send {} }
            }
        }
    }
}
