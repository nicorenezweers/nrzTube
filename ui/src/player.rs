//! Der Player: ein `<video>`-Element mit HLS (hls.js), eigener Steuerleiste, Kapiteln,
//! SponsorBlock, Untertiteln, Qualitätswahl, Bildschirmfoto, Bild-im-Bild, externem Player,
//! Tastenkürzeln und Mini-Player. Er lebt im Layout und läuft beim Seitenwechsel weiter.

use crate::Route;
use crate::api;
use crate::components::{watch_href, watch_route};
use crate::state::*;
use crate::util::{self, duration};
use dioxus::prelude::*;
use nrztube_types::*;
use std::collections::HashSet;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

const VID: &str = "nrz-video";
const BOX: &str = "nrz-player";
const WRAP: &str = "nrz-wrap";

fn video() -> Option<web_sys::HtmlVideoElement> {
    web_sys::window()?.document()?.get_element_by_id(VID)?.dyn_into().ok()
}

#[derive(Clone, Copy, PartialEq)]
enum Menu {
    None,
    Settings,
    Quality,
    Audio,
    Speed,
    Captions,
    External,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, Default)]
struct Level {
    index: i32,
    height: u32,
    bitrate: u64,
    fps: u32,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, Default)]
struct Levels {
    levels: Vec<Level>,
    current: i32,
    auto: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
struct AudioTrack {
    index: i32,
    name: String,
    #[serde(default)]
    lang: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
struct AudioTracks {
    tracks: Vec<AudioTrack>,
    current: i32,
}

#[derive(Clone, PartialEq)]
struct Loaded {
    info: StreamInfo,
    resume: f64,
}

/// Steuerung, die auch Tastenkürzel benutzen
#[derive(Clone, Copy)]
struct Ctl {
    menu: Signal<Menu>,
    theater_flash: Signal<Option<String>>,
    captions: Signal<Option<String>>,
    segments: Signal<Vec<Segment>>,
    manual: Signal<Option<Segment>>,
}

impl Ctl {
    fn toggle_play(&self) {
        if let Some(v) = video() {
            if v.paused() {
                let _ = v.play();
            } else {
                let _ = v.pause();
            }
        }
    }
    fn seek_by(&self, d: f64) {
        if let Some(v) = video() {
            let t = (v.current_time() + d).clamp(0.0, v.duration().max(0.0));
            v.set_current_time(t);
            self.flash(if d >= 0.0 { format!("+{} s", d.abs() as u32) } else { format!("−{} s", d.abs() as u32) });
        }
    }
    fn seek_to(&self, t: f64) {
        if let Some(v) = video() {
            v.set_current_time(t.max(0.0));
        }
    }
    fn volume_by(&self, d: f64) {
        if let Some(v) = video() {
            let n = (v.volume() + d).clamp(0.0, 1.0);
            v.set_volume(n);
            v.set_muted(false);
            self.flash(format!("Lautstärke {} %", (n * 100.0).round()));
        }
    }
    fn toggle_mute(&self) {
        if let Some(v) = video() {
            v.set_muted(!v.muted());
            self.flash(if v.muted() { "Ton aus".into() } else { "Ton an".into() });
        }
    }
    fn speed_by(&self, d: f64) {
        if let Some(v) = video() {
            let n = ((v.playback_rate() + d) * 100.0).round() / 100.0;
            let n = n.clamp(0.25, 4.0);
            v.set_playback_rate(n);
            self.flash(format!("{n}×"));
        }
    }
    fn frame(&self, d: f64) {
        if let Some(v) = video() {
            let _ = v.pause();
            v.set_current_time((v.current_time() + d / 30.0).max(0.0));
        }
    }
    fn flash(&self, text: String) {
        let mut f = self.theater_flash;
        f.set(Some(text.clone()));
        dioxus::core::spawn_forever(async move {
            gloo_timers::future::TimeoutFuture::new(900).await;
            if f.peek().as_deref() == Some(text.as_str()) {
                f.set(None);
            }
        });
    }
    fn screenshot(&self) {
        let title = DETAILS.peek().as_ref().map(|d| d.title.clone()).unwrap_or_else(|| "nrzTube".into());
        let t = video().map(|v| v.current_time()).unwrap_or(0.0);
        let name: String = format!("{title} {}", duration(t).replace(':', "-"))
            .chars()
            .filter(|c| !"/\\:*?\"<>|".contains(*c))
            .collect();
        let fmt = SETTINGS.peek().screenshot_format.clone();
        util::eval(&format!("window.nrzScreenshot({}, {}, {})", util::js_str(VID), util::js_str(&name), util::js_str(&fmt)));
        self.flash("Bildschirmfoto gespeichert".into());
    }
    fn set_captions(&self, lang: Option<String>) {
        let mut c = self.captions;
        c.set(lang.clone());
        apply_captions(lang.as_deref());
    }
}

fn apply_captions(lang: Option<&str>) {
    let Some(v) = video() else { return };
    let tracks = v.text_tracks();
    let Some(tracks) = tracks else { return };
    for i in 0..tracks.length() {
        if let Some(t) = tracks.get(i) {
            let on = lang.is_some_and(|l| t.language() == l);
            t.set_mode(if on { web_sys::TextTrackMode::Showing } else { web_sys::TextTrackMode::Disabled });
        }
    }
}

/// Standbild vor dem Start: YouTube-Vorschaubild oder (Twitch) das Bild aus den Angaben
fn poster_for(id: &str) -> String {
    if is_twitch_id(id) {
        return DETAILS.peek().as_ref().filter(|d| d.id == id).map(|d| d.thumb.clone()).unwrap_or_default();
    }
    format!("/vi/{id}/hqdefault.jpg")
}

fn current_item(id: &str) -> VideoItem {
    match DETAILS.peek().as_ref().filter(|d| d.id == id) {
        Some(d) => VideoItem {
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
        },
        None => VideoItem { id: id.to_string(), thumb: poster_for(id), ..Default::default() },
    }
}

/// Gespielte Sekunden seit der letzten Meldung (für die Empfehlungen)
static PLAYED: GlobalSignal<f64> = Signal::global(|| 0.0);

fn save_history(id: String, progress: f64) {
    save_history_ext(id, progress, false);
}

fn save_history_ext(id: String, progress: f64, opened: bool) {
    let item = current_item(&id);
    if item.title.is_empty() {
        return;
    }
    let played = std::mem::take(&mut *PLAYED.write());
    let duration = video().map(|v| v.duration()).filter(|d| d.is_finite()).unwrap_or(0.0);
    dioxus::core::spawn_forever(async move {
        let _ = api::post::<_, serde_json::Value>("/api/history", &HistoryUpdate { video: item, progress, played, duration, opened }).await;
    });
}

/// Nächstes Video: Playlist, lokale Playlist oder (mit Autoplay) erstes ähnliches Video
async fn next_video(now: &NowPlaying, forward: bool) -> Option<Route> {
    if let Some(pid) = now.local_list {
        if let Ok(p) = api::get::<LocalPlaylist>(&format!("/api/playlists/{pid}")).await {
            let i = p.videos.iter().position(|v| v.id == now.id)?;
            let j = if forward { i + 1 } else { i.checked_sub(1)? };
            let v = p.videos.get(j)?;
            return Some(Route::Watch { v: v.id.clone(), list: format!("local:{pid}"), t: String::new() });
        }
    }
    let d = DETAILS.peek().clone()?;
    if let Some(pl) = &d.playlist {
        let j = if forward { pl.index + 1 } else { pl.index.checked_sub(1)? };
        if let Some(v) = pl.videos.get(j) {
            return Some(watch_route(&v.id, Some(&pl.id), None));
        }
        return None;
    }
    if forward && SETTINGS.peek().autoplay_next {
        return d.related.first().map(|v| watch_route(&v.id, None, None));
    }
    None
}

#[component]
pub fn PlayerHost(big: bool) -> Element {
    if NOW().is_none() {
        return rsx! {};
    }
    rsx! { PlayerInner { big } }
}

#[component]
fn PlayerInner(big: bool) -> Element {
    let now = NOW().unwrap_or_else(|| NowPlaying { id: String::new(), list: None, local_list: None, start: 0.0 });
    let nav = navigator();
    let settings = SETTINGS();
    let id = now.id.clone();

    let mut playing = use_signal(|| false);
    let mut time = use_signal(|| 0.0f64);
    let mut dur = use_signal(|| 0.0f64);
    let mut buffered = use_signal(|| 0.0f64);
    let mut volume = use_signal(|| 1.0f64);
    let mut muted = use_signal(|| false);
    let mut rate = use_signal(|| 1.0f64);
    let mut waiting = use_signal(|| false);
    let mut ended = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut hover = use_signal(|| None::<(f64, f64)>);
    let mut levels = use_signal(Levels::default);
    let mut audios = use_signal(AudioTracks::default);
    let mut audio_only = use_signal(|| false);
    let mut fullscreen = use_signal(|| false);
    let mut notice = use_signal(|| None::<(String, f64)>);
    let mut skipped = use_signal(HashSet::<String>::new);
    let mut last_saved = use_signal(|| 0.0f64);
    let mut last_t = use_signal(|| 0.0f64);
    let mut late_caps = use_signal(Vec::<Caption>::new);
    // Voller Abruf nach dem schnellen: Kurve "Meistgesehen" und Storyboard kommen erst damit
    let mut late_full = use_signal(|| None::<StreamInfo>);
    // Sekundentakt für "Live seit"
    let mut tick = use_signal(|| 0u64);
    let mut playing_once = use_signal(|| false);
    let id_reset = id.clone();
    use_effect(use_reactive!(|id_reset| { let _ = &id_reset; playing_once.set(false); }));
    let mut started = use_signal(String::new);
    // Neues Video gewählt: das alte sofort anhalten und entfernen (schwarz + Ladekreis)
    let id_switch = id.clone();
    let mut last_id = use_signal(String::new);
    use_effect(use_reactive!(|id_switch| {
        let prev = last_id.peek().clone();
        if !prev.is_empty() && prev != id_switch {
            if let Some(v) = video() {
                let _ = v.pause();
            }
            util::eval(&format!("window.nrzDetach({0}); (function(){{ const v = document.getElementById({0}); if (v) {{ v.removeAttribute('src'); v.load(); }} }})()", util::js_str(VID)));
        }
        last_id.set(id_switch.clone());
    }));
    let mut ui_visible = use_signal(|| true);
    let mut ui_timer = use_signal(|| 0u32);
    // Maus steht auf der Steuerleiste: nicht ausblenden
    let mut over_ctl = use_signal(|| false);
    let ctl = Ctl {
        menu: use_signal(|| Menu::None),
        theater_flash: use_signal(|| None),
        captions: use_signal(|| None),
        segments: use_signal(Vec::new),
        manual: use_signal(|| None),
    };
    let mut menu = ctl.menu;

    // Streams und letzte Position laden
    let mut loaded = use_resource(move || async move {
        let Some(n) = NOW() else { return None };
        let tw = is_twitch_id(&n.id);
        // In einer Watch-Party über den Raum (funktioniert auch für Gäste ohne Konto)
        let party = crate::pages::party::PARTY.peek().clone().filter(|_| {
            crate::pages::party::PARTY_STATE.peek().as_ref().and_then(|s| s.video.as_ref()).map(|v| v.id == n.id).unwrap_or(false)
        });
        let url = match party {
            Some(pid) => format!("/api/party/{pid}/streams"),
            None if tw => format!("/api/twitch/streams/{}", n.id),
            None => format!("/api/video/{}/streams", n.id),
        };
        let info = api::get::<StreamInfo>(&url).await;
        let mut resume = n.start;
        // Livestreams haben keine Position, VODs und Clips schon
        if resume <= 0.0 && !n.id.starts_with("tw:live:") && SETTINGS.peek().remember_position {
            if let Ok(Some(h)) = api::get::<Option<HistoryEntry>>(&format!("/api/history/{}", n.id)).await {
                let d = info.as_ref().ok().and_then(|i| i.duration).unwrap_or(0.0);
                if h.progress > 5.0 && (d == 0.0 || h.progress < d - 15.0) {
                    resume = h.progress;
                }
            }
        }
        Some(info.map(|info| Loaded { info, resume }))
    });

    // SponsorBlock-Segmente
    let sb_id = id.clone();
    let mut segments = ctl.segments;
    use_effect(use_reactive!(|sb_id| {
        segments.set(Vec::new());
        skipped.set(HashSet::new());
        // Gäste einer Watch-Party (ohne Konto) fragen SponsorBlock nicht ab
        let logged_in = AUTH.peek().as_ref().and_then(|a| a.me.as_ref()).is_some();
        if SETTINGS.peek().sb_enabled && !is_twitch_id(&sb_id) && logged_in {
            spawn(async move {
                if let Ok(s) = api::get::<Vec<Segment>>(&format!("/api/sponsorblock/{sb_id}")).await {
                    segments.set(s);
                }
            });
        }
    }));

    // Quelle an das <video>-Element hängen
    let ctl_c = ctl;
    use_effect(move || {
        let Some(Some(Ok(l))) = loaded.read().clone() else { return };
        let s = SETTINGS.peek().clone();
        let audio = audio_only();
        error.set(None);
        ended.set(false);
        let (src, kind) = if audio {
            match l.info.formats.iter().filter(|f| f.kind == "audio").max_by_key(|f| (f.ext == "m4a") as u32 * 10_000_000 + f.bitrate) {
                Some(f) => (f.url.clone(), "file"),
                None => return,
            }
        } else if let Some(h) = &l.info.hls {
            let q = if s.codec == "avc1" { "?codec=avc1" } else { "" };
            (format!("{h}.m3u8{q}"), "hls")
        } else if let Some(f) = l.info.formats.iter().filter(|f| f.kind == "av").max_by_key(|f| f.height) {
            (f.url.clone(), "file")
        } else {
            error.set(Some("Für dieses Video gibt es keine abspielbare Quelle.".into()));
            return;
        };
        let max_h: u32 = s.quality.parse().unwrap_or(0);
        let start = if started.peek().as_str() == l.info.id { video().map(|v| v.current_time()).unwrap_or(l.resume) } else { l.resume };
        // Bevorzugte Audiospur für mehrsprachige Videos (hls.js wählt sie beim Laden)
        util::eval(&format!("window.__nrzAudioPref = {}", util::js_str(if s.audio_lang.is_empty() { "de" } else { &s.audio_lang })));
        let autoplay = s.autoplay || started.peek().as_str() == l.info.id;
        util::eval(&format!(
            "window.nrzAttach({}, {}, {}, {}, {}, {})",
            util::js_str(VID),
            util::js_str(&src),
            start,
            autoplay,
            max_h,
            util::js_str(kind)
        ));
        if let Some(v) = video() {
            v.set_volume(s.volume.clamp(0.0, 1.0));
            v.set_playback_rate(if s.speed > 0.0 { s.speed } else { 1.0 });
            v.set_loop(s.loop_video);
        }
        // Untertitel nach dem Laden der Spuren einschalten
        let caps = l.info.captions.clone();
        let want = if s.captions {
            caps.iter().find(|c| c.lang.starts_with(&s.caption_lang) && !c.auto)
                .or_else(|| caps.iter().find(|c| c.lang.starts_with(&s.caption_lang)))
                .or_else(|| caps.first())
                .map(|c| c.lang.clone())
        } else {
            None
        };
        spawn(async move {
            gloo_timers::future::TimeoutFuture::new(600).await;
            ctl_c.set_captions(want);
        });
        let title = DETAILS.peek().as_ref().map(|d| (d.title.clone(), d.author.clone())).unwrap_or_default();
        util::eval(&format!(
            "window.nrzMediaSession({}, {}, {})",
            util::js_str(&title.0),
            util::js_str(&title.1),
            util::js_str(&format!("{}{}", util::origin(), poster_for(&l.info.id)))
        ));
    });

    // Schnell geladen (ohne alle Untertitel): vollen Abruf abwarten und Untertitel nachreichen.
    // Ausserdem das nächste Video vorladen, damit es ohne Wartezeit startet.
    use_effect(move || {
        let Some(Some(Ok(l))) = loaded.read().clone() else { return };
        late_caps.set(Vec::new());
        let id = l.info.id.clone();
        if l.info.partial {
            spawn(async move {
                for _ in 0..10 {
                    gloo_timers::future::TimeoutFuture::new(2000).await;
                    if NOW.peek().as_ref().map(|n| n.id != id).unwrap_or(true) {
                        return;
                    }
                    if let Ok(full) = api::get::<StreamInfo>(&format!("/api/video/{id}/streams")).await {
                        if !full.partial {
                            let s = SETTINGS.peek().clone();
                            let had = (ctl_c.captions)().is_some();
                            late_caps.set(full.captions.clone());
                            late_full.set(Some(full.clone()));
                            if s.captions && !had {
                                let want = full.captions.iter().find(|c| c.lang.starts_with(&s.caption_lang) && !c.auto)
                                    .or_else(|| full.captions.iter().find(|c| c.lang.starts_with(&s.caption_lang)))
                                    .map(|c| c.lang.clone());
                                if want.is_some() {
                                    gloo_timers::future::TimeoutFuture::new(300).await;
                                    ctl_c.set_captions(want);
                                }
                            }
                            return;
                        }
                    }
                }
            });
        }
        if let Some(n) = NOW.peek().clone() {
            spawn(async move {
                gloo_timers::future::TimeoutFuture::new(4000).await;
                if let Some(Route::Watch { v, .. }) = next_video(&n, true).await {
                    prefetch(vec![v]);
                }
            });
        }
    });

    // "Live seit": jede Sekunde neu zeichnen, nur solange ein Livestream läuft
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(1000).await;
            let live = loaded.peek().as_ref().and_then(|x| x.as_ref()).and_then(|x| x.as_ref().ok()).map(|l| l.info.live).unwrap_or(false);
            if live {
                let n = *tick.peek() + 1;
                tick.set(n);
            }
        }
    });

    // Sprung anfordern (Kapitel, Zeitmarken)
    use_effect(move || {
        if let Some(t) = SEEK() {
            ctl_c.seek_to(t);
            if let Some(v) = video() {
                let _ = v.play();
            }
            *SEEK.write() = None;
        }
    });

    // Tastenkürzel (einmal registrieren)
    use_hook(move || {
        let ctl = ctl;
        let handler = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
            if e.ctrl_key() || e.meta_key() || e.alt_key() {
                return;
            }
            if let Some(t) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                let tag = t.tag_name().to_lowercase();
                if tag == "input" || tag == "textarea" || tag == "select" || t.is_content_editable() {
                    return;
                }
                // Leertaste auf einem Knopf (z. B. nach Klick auf Vollbild): nicht den Knopf auslösen,
                // sondern abspielen/pausieren
                if e.key() == " " && (tag == "button" || tag == "a" || tag == "video") && NOW.peek().is_some() {
                    e.prevent_default();
                    e.stop_propagation();
                    let _ = t.blur();
                }
            }
            let key = e.key();
            if key == "/" {
                if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("search")) {
                    if let Ok(i) = el.dyn_into::<web_sys::HtmlElement>() {
                        e.prevent_default();
                        let _ = i.focus();
                    }
                }
                return;
            }
            if key == "?" {
                let open = *SHORTCUTS.peek();
                *SHORTCUTS.write() = !open;
                return;
            }
            if NOW.peek().is_none() || video().is_none() {
                return;
            }
            let skip = SETTINGS.peek().skip_seconds.max(1) as f64;
            let mut handled = true;
            match key.as_str() {
                " " | "k" | "K" => ctl.toggle_play(),
                "j" | "J" => ctl.seek_by(-skip),
                "l" | "L" => ctl.seek_by(skip),
                "ArrowLeft" => ctl.seek_by(-5.0),
                "ArrowRight" => ctl.seek_by(5.0),
                "ArrowUp" => ctl.volume_by(0.05),
                "ArrowDown" => ctl.volume_by(-0.05),
                "m" | "M" => ctl.toggle_mute(),
                "f" | "F" => util::eval(&format!("window.nrzFullscreen({})", util::js_str(BOX))),
                "t" | "T" => update_settings(|s| s.theater = !s.theater),
                "i" | "I" => util::eval(&format!("window.nrzPip({})", util::js_str(VID))),
                "s" | "S" => ctl.screenshot(),
                "c" | "C" => {
                    let caps = match video() {
                        Some(v) => v.text_tracks().map(|t| {
                            (0..t.length()).filter_map(|i| t.get(i)).map(|t| t.language()).collect::<Vec<_>>()
                        }).unwrap_or_default(),
                        None => vec![],
                    };
                    let cur = ctl.captions.peek().clone();
                    if cur.is_some() {
                        ctl.set_captions(None);
                        ctl.flash("Untertitel aus".into());
                    } else if let Some(first) = caps.iter().find(|l| l.starts_with(&SETTINGS.peek().caption_lang)).or(caps.first()) {
                        ctl.set_captions(Some(first.clone()));
                        ctl.flash(format!("Untertitel: {first}"));
                    }
                }
                "<" => ctl.speed_by(-0.25),
                ">" => ctl.speed_by(0.25),
                "," => ctl.frame(-1.0),
                "." => ctl.frame(1.0),
                "Home" => ctl.seek_to(0.0),
                "End" => {
                    if let Some(v) = video() {
                        v.set_current_time(v.duration());
                    }
                }
                "Enter" => {
                    if let Some(seg) = ctl.manual.peek().clone() {
                        ctl.seek_to(seg.end);
                    } else {
                        handled = false;
                    }
                }
                "n" | "N" | "p" | "P" => {
                    let fwd = key.eq_ignore_ascii_case("n");
                    if let Some(n) = NOW.peek().clone() {
                        dioxus::core::spawn_forever(async move {
                            if let Some(r) = next_video(&n, fwd).await {
                                navigator().push(r);
                            }
                        });
                    }
                }
                k if k.len() == 1 && k.chars().all(|c| c.is_ascii_digit()) => {
                    if let Some(v) = video() {
                        let n: f64 = k.parse().unwrap_or(0.0);
                        v.set_current_time(v.duration() * n / 10.0);
                    }
                }
                _ => handled = false,
            }
            if handled {
                e.prevent_default();
            }
        });
        if let Some(d) = web_sys::window().and_then(|w| w.document()) {
            let _ = d.add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
        }
        handler.forget();
    });

    let info = loaded.read().clone();
    // Nur Daten des aktuellen Videos – während das nächste lädt, nichts Altes zeigen
    let (info, load_error) = match info {
        Some(Some(Ok(l))) if l.info.id == id => (Some(l.info), None),
        Some(Some(Err(e))) => (None, Some(e.message)),
        _ => (None, None),
    };
    let details = DETAILS().filter(|d| d.id == id);
    let chapters: Vec<Chapter> = if settings.hide_chapters {
        vec![]
    } else {
        details
            .as_ref()
            .map(|d| d.chapters.clone())
            .filter(|c| !c.is_empty())
            .or_else(|| info.as_ref().map(|i| i.chapters.clone()))
            .unwrap_or_default()
    };
    let d_total = if dur() > 0.0 && dur().is_finite() { dur() } else { info.as_ref().and_then(|i| i.duration).unwrap_or(0.0) };
    let cur_chapter = chapters.iter().rev().find(|c| c.start <= time()).map(|c| c.title.clone());
    let captions = {
        let c = info.as_ref().map(|i| i.captions.clone()).unwrap_or_default();
        if late_caps.read().len() > c.len() { late_caps.read().clone() } else { c }
    };
    let late = late_full.read().clone().filter(|f| Some(&f.id) == info.as_ref().map(|i| &i.id));
    let heatmap = if settings.hide_heatmap {
        vec![]
    } else {
        info.as_ref().map(|i| i.heatmap.clone()).filter(|h| !h.is_empty()).or_else(|| late.as_ref().map(|f| f.heatmap.clone())).unwrap_or_default()
    };
    let storyboard = info.as_ref().and_then(|i| i.storyboard.clone()).or_else(|| late.as_ref().and_then(|f| f.storyboard.clone()));
    let is_live = info.as_ref().map(|i| i.live).unwrap_or(false);
    let live_since = info.as_ref().and_then(|i| i.live_since).or_else(|| late.as_ref().and_then(|f| f.live_since));
    let _ = tick();
    let live_for = live_since.map(|t| (util::now() - t).max(0) as f64);
    let sb_modes = settings.sb_categories.clone();
    let sb_min = settings.sb_min_length;
    let segs: Vec<Segment> = segments()
        .into_iter()
        .filter(|s| sb_modes.get(&s.category).map(|m| m != "off").unwrap_or(false))
        .collect();
    let highlight = segs.iter().find(|s| s.action == "poi").cloned();
    let full_label = segs.iter().find(|s| s.action == "full").map(|s| cat_name(&s.category));
    let pct = |t: f64| if d_total > 0.0 { (t / d_total * 100.0).clamp(0.0, 100.0) } else { 0.0 };

    let ontime = {
        let id = id.clone();
        let segs = segs.clone();
        let modes = sb_modes.clone();
        let mut manual = ctl.manual;
        move |_| {
            let Some(v) = video() else { return };
            let t = v.current_time();
            let dt = t - *last_t.peek();
            if dt > 0.0 && dt < 1.5 && !v.paused() {
                *PLAYED.write() += dt;
            }
            last_t.set(t);
            time.set(t);
            let b = v.buffered();
            if b.length() > 0 {
                buffered.set(b.end(b.length() - 1).unwrap_or(0.0));
            }
            // SponsorBlock
            let mut in_manual = None;
            for s in &segs {
                let mode = modes.get(&s.category).map(String::as_str).unwrap_or("off");
                if s.action != "skip" && s.action != "mute" {
                    continue;
                }
                if s.end - s.start < sb_min {
                    continue;
                }
                let inside = t >= s.start && t < s.end - 0.25;
                if !inside {
                    continue;
                }
                if mode == "skip" && !skipped.peek().contains(&s.uuid) {
                    skipped.write().insert(s.uuid.clone());
                    if s.action == "mute" {
                        v.set_muted(true);
                    } else {
                        v.set_current_time(s.end);
                    }
                    if SETTINGS.peek().sb_notify {
                        notice.set(Some((format!("{} übersprungen", cat_name(&s.category)), s.start)));
                        spawn(async move {
                            gloo_timers::future::TimeoutFuture::new(4000).await;
                            notice.set(None);
                        });
                    }
                } else if mode == "manual" {
                    in_manual = Some(s.clone());
                }
            }
            if *manual.peek() != in_manual {
                manual.set(in_manual);
            }
            // Verlauf alle 15 Sekunden
            if (t - *last_saved.peek()).abs() > 15.0 {
                last_saved.set(t);
                save_history(id.clone(), t);
            }
        }
    };

    let id_play = id.clone();
    let id_pause = id.clone();
    let now_end = now.clone();
    let progress_click = move |e: MouseEvent| {
        let x = e.client_coordinates().x;
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("nrz-progress")) {
            let r = el.get_bounding_client_rect();
            let p = ((x - r.left()) / r.width()).clamp(0.0, 1.0);
            ctl_c.seek_to(p * d_total);
        }
    };
    let chapters_h = chapters.clone();
    let progress_move = move |e: MouseEvent| {
        let x = e.client_coordinates().x;
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("nrz-progress")) {
            let r = el.get_bounding_client_rect();
            let p = ((x - r.left()) / r.width()).clamp(0.0, 1.0);
            hover.set(Some((p * 100.0, p * d_total)));
        }
    };
    let hover_chapter = hover().and_then(|(_, t)| chapters_h.iter().rev().find(|c| c.start <= t).map(|c| c.title.clone()));

    let mut poke = move || {
        ui_visible.set(true);
        let n = *ui_timer.peek() + 1;
        ui_timer.set(n);
        spawn(async move {
            gloo_timers::future::TimeoutFuture::new(2500).await;
            if *ui_timer.peek() == n && !*over_ctl.peek() {
                ui_visible.set(false);
            }
        });
    };

    let shell_class = format!(
        "player-shell {} {} {}",
        if !playing() { "paused" } else { "" },
        if ui_visible() || menu() != Menu::None { "show-ui" } else { "" },
        if playing() && !ui_visible() && menu() == Menu::None { "hide-cursor" } else { "" }
    );
    let wrap_class = if big {
        // Handy: Player bleibt beim Scrollen oben stehen
        if settings.theater { "sticky top-0 z-30 w-full bg-black md:static" } else { "sticky top-0 z-30 mx-auto w-full max-w-6xl bg-black px-0 pt-0 md:static md:bg-transparent md:px-6 md:pt-4" }
    } else {
        "mini-player"
    };
    let video_size = if big {
        if settings.theater { "aspect-video max-h-[calc(100dvh-8.5rem)] w-full mx-auto" } else { "aspect-video w-full md:rounded-xl" }
    } else {
        "aspect-video w-full"
    };
    let title = details.as_ref().map(|d| d.title.clone()).unwrap_or_default();
    let author = details.as_ref().map(|d| d.author.clone()).unwrap_or_default();
    let now_for_expand = now.clone();
    let now_for_bar = now.clone();
    let collapsed = MINI_COLLAPSED() && !big;
    // Mini-Player ziehbar machen bzw. beim grossen Player die Position wieder lösen
    use_effect(use_reactive!(|big| {
        if big {
            util::eval(&format!("window.nrzMiniClear({})", util::js_str(WRAP)));
        } else {
            util::eval(&format!("setTimeout(() => window.nrzMini({}), 30)", util::js_str(WRAP)));
        }
    }));

    rsx! {
        div { id: WRAP, class: if collapsed { "{wrap_class} mini-collapsed" } else { "{wrap_class}" },
            div { id: BOX, class: "{shell_class} {video_size}",
                onmousemove: move |_| poke(),
                // Tippen (Handy) zeigt die Leiste ebenfalls
                ontouchstart: move |_| poke(),
                // Maus raus: Leiste sofort weg, solange das Video läuft
                onmouseleave: move |_| { if playing() { let n = *ui_timer.peek() + 1; ui_timer.set(n); over_ctl.set(false); ui_visible.set(false); } },
                video {
                    id: VID,
                    class: "absolute inset-0",
                    playsinline: true,
                    // Vorschaubild sofort, bis das erste Bild des Videos da ist
                    // Kein vergrössertes Vorschaubild beim Laden – nur bei "Nur Ton"
                    poster: if audio_only() { poster_for(&id) } else { String::new() },
                    onclick: move |_| { if menu() != Menu::None { menu.set(Menu::None) } else { ctl_c.toggle_play() } },
                    ondoubleclick: move |_| util::eval(&format!("window.nrzFullscreen({})", util::js_str(BOX))),
                    onplay: move |_| {
                        playing.set(true);
                        ended.set(false);
                        crate::pages::party::local_event("play", video().map(|v| v.current_time()).unwrap_or(0.0));
                    },
                    onplaying: move |_| {
                        waiting.set(false);
                        playing_once.set(true);
                        if *started.peek() != id_play {
                            started.set(id_play.clone());
                            *PLAYED.write() = 0.0;
                            save_history_ext(id_play.clone(), video().map(|v| v.current_time()).unwrap_or(0.0), true);
                        }
                    },
                    onpause: move |_| {
                        playing.set(false);
                        // Pause durch Ende ist kein "Pause" für die Party
                        if !video().map(|v| v.ended()).unwrap_or(false) {
                            crate::pages::party::local_event("pause", video().map(|v| v.current_time()).unwrap_or(0.0));
                        }
                        save_history(id_pause.clone(), video().map(|v| v.current_time()).unwrap_or(0.0));
                    },
                    onwaiting: move |_| waiting.set(true),
                    oncanplay: move |_| waiting.set(false),
                    ontimeupdate: ontime,
                    ondurationchange: move |_| { if let Some(v) = video() { dur.set(v.duration()); } },
                    onloadedmetadata: move |_| { if let Some(v) = video() { dur.set(v.duration()); } },
                    onvolumechange: move |_| { if let Some(v) = video() { volume.set(v.volume()); muted.set(v.muted()); } },
                    onratechange: move |_| { if let Some(v) = video() { rate.set(v.playback_rate()); } },
                    onseeked: move |_| {
                        // Zurückgespult: Segmente dürfen wieder übersprungen werden
                        let t = video().map(|v| v.current_time()).unwrap_or(0.0);
                        let segs = segments.peek().clone();
                        skipped.write().retain(|u| segs.iter().find(|s| &s.uuid == u).map(|s| s.start < t).unwrap_or(false));
                        crate::pages::party::local_event("seek", t);
                    },
                    onended: move |_| {
                        playing.set(false);
                        ended.set(true);
                        let n = now_end.clone();
                        save_history(n.id.clone(), 0.0);
                        // Watch-Party: der Raum schaltet weiter (Warteschlange)
                        if crate::pages::party::PARTY.peek().is_some() {
                            crate::pages::party::local_event("ended", video().map(|v| v.current_time()).unwrap_or(0.0));
                            return;
                        }
                        if !SETTINGS.peek().loop_video {
                            spawn(async move {
                                let next = next_video(&n, true).await;
                                // Stream-Modus: fertig Geschautes aus der Stream-Liste nehmen
                                let s = SETTINGS.peek().clone();
                                let in_stream = n.local_list.is_some() && n.local_list == stream_list_id();
                                if in_stream && s.stream_mode && s.stream_remove_done {
                                    if let Some(pid) = n.local_list {
                                        let _ = api::delete::<serde_json::Value>(&format!("/api/playlists/{pid}/videos/{}", n.id)).await;
                                        reload_playlists().await;
                                    }
                                }
                                match next {
                                    Some(r) => { navigator().push(r); }
                                    None if in_stream => toast("Stream-Liste fertig abgespielt"),
                                    None => {}
                                }
                            });
                        }
                    },
                    onerror: move |_| {
                        if let Some(v) = video() {
                            if v.error().is_some() && !v.current_src().is_empty() {
                                error.set(Some("Das Video lässt sich nicht abspielen.".into()));
                            }
                        }
                    },
                    for c in captions.iter() {
                        track { key: "{c.lang}-{c.auto}", kind: "subtitles", src: "{c.url}", srclang: "{c.lang}", label: "{c.name}" }
                    }
                }

                // Livestream: zurück zur aktuellen Stelle (oben rechts, mit der Steuerleiste sichtbar)
                if is_live && big {
                    div { class: "player-controls absolute right-3 top-3 z-10 flex items-center",
                        button { class: "flex items-center gap-1.5 rounded-full bg-black/60 px-3 py-1.5 text-xs font-semibold text-white backdrop-blur hover:bg-black/80",
                            title: "Zur aktuellen Stelle springen",
                            onclick: move |e| { e.stop_propagation(); util::eval(&format!("window.nrzGoLive({})", util::js_str(VID))); },
                            icons::RefreshCw { class: "size-3.5" } "Sync"
                        }
                        if CHAT_HIDDEN() && settings.twitch_chat {
                            button { class: "ml-2 flex items-center gap-1.5 rounded-full bg-black/60 px-3 py-1.5 text-xs font-semibold text-white backdrop-blur hover:bg-black/80",
                                title: "Chat einblenden",
                                onclick: move |e| { e.stop_propagation(); *CHAT_HIDDEN.write() = false; },
                                icons::MessageSquare { class: "size-3.5" } "Chat"
                            }
                        }
                    }
                }
                // Hinweise
                if info.is_none() && load_error.is_none() {
                    div { class: "absolute inset-0 flex items-center justify-center bg-black",
                        icons::LoaderCircle { class: "size-10 animate-spin text-white" }
                    }
                } else if waiting() && playing() {
                    div { class: "pointer-events-none absolute inset-0 flex items-center justify-center",
                        icons::LoaderCircle { class: "size-10 animate-spin text-white/90" }
                    }
                }
                if let Some(e) = load_error.clone().or(error()) {
                    div { class: "absolute inset-0 flex flex-col items-center justify-center gap-3 bg-black/85 p-6 text-center text-white",
                        icons::CircleAlert { class: "size-8" }
                        p { class: "max-w-md text-sm whitespace-pre-line", "{e}" }
                        button { class: "rounded-md bg-white/15 px-3 py-1.5 text-sm hover:bg-white/25",
                            onclick: move |_| { error.set(None); loaded.restart(); }, "Nochmal versuchen" }
                    }
                }
                if let Some(f) = (ctl.theater_flash)() {
                    div { class: "pointer-events-none absolute left-1/2 top-[18%] -translate-x-1/2 rounded-lg bg-black/70 px-3 py-1.5 text-sm font-medium text-white", "{f}" }
                }
                if let Some((n, start)) = notice() {
                    div { class: "absolute bottom-20 left-3 flex items-center gap-2 rounded-lg bg-black/80 px-3 py-2 text-sm text-white",
                        icons::SkipForward { class: "size-4" }
                        "{n}"
                        button { class: "ml-1 rounded bg-white/15 px-2 py-0.5 text-xs hover:bg-white/25",
                            onclick: move |_| { ctl_c.seek_to(start); notice.set(None); }, "Rückgängig" }
                    }
                }
                if let Some(seg) = (ctl.manual)() {
                    button { class: "absolute bottom-20 right-3 flex items-center gap-2 rounded-lg bg-white/90 px-3 py-2 text-sm font-medium text-black shadow-lg hover:bg-white",
                        onclick: move |_| ctl_c.seek_to(seg.end),
                        icons::SkipForward { class: "size-4" }
                        "Überspringen: {cat_name(&seg.category)}"
                    }
                } else if let Some(h) = highlight.clone().filter(|h| time() < h.start - 1.0 && time() < 30.0) {
                    button { class: "absolute bottom-20 right-3 flex items-center gap-2 rounded-lg bg-black/80 px-3 py-2 text-sm text-white hover:bg-black",
                        onclick: move |_| ctl_c.seek_to(h.start),
                        icons::Star { class: "size-4" } "Zum Highlight springen"
                    }
                }
                if let Some(l) = full_label.clone().filter(|_| time() < 8.0) {
                    div { class: "absolute left-3 top-3 rounded-md bg-black/80 px-2.5 py-1 text-xs text-white", "Ganzes Video: {l}" }
                }
                if ended() && big {
                    if let Some(d) = details.clone().filter(|_| !settings.hide_end_suggestions && !settings.hide_related) {
                        div { class: "absolute inset-0 flex flex-col items-center justify-center gap-4 bg-black/85 p-6",
                            div { class: "grid w-full max-w-3xl grid-cols-2 gap-3 md:grid-cols-4",
                                for v in d.related.iter().take(4) {
                                    Link { key: "{v.id}", to: watch_route(&v.id, None, None), class: "group text-left text-white",
                                        img { class: "aspect-video w-full rounded-lg object-cover", src: "{v.thumb}", alt: "" }
                                        div { class: "mt-1 line-clamp-2 text-xs", "{v.title}" }
                                    }
                                }
                            }
                            button { class: "flex items-center gap-2 rounded-md bg-white/15 px-3 py-1.5 text-sm text-white hover:bg-white/25",
                                onclick: move |_| { ctl_c.seek_to(0.0); if let Some(v) = video() { let _ = v.play(); } },
                                icons::Repeat { class: "size-4" } "Nochmal ansehen"
                            }
                        }
                    }
                }

                // Steuerleiste
                if info.is_some() {
                    div { class: "player-controls absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/85 via-black/40 to-transparent px-3 pb-2 pt-10 text-white",
                        onmouseenter: move |_| over_ctl.set(true),
                        onmouseleave: move |_| over_ctl.set(false),
                        onclick: move |e| e.stop_propagation(),
                        if !is_live {
                            div { id: "nrz-progress", class: "progress-wrap",
                                onclick: progress_click,
                                onmousemove: progress_move,
                                onmouseleave: move |_| hover.set(None),
                                if !heatmap.is_empty() && big {
                                    svg { class: "heatmap", view_box: "0 0 1000 100", preserve_aspect_ratio: "none",
                                        path { fill: "rgb(255 255 255 / 0.5)", d: heat_path(&heatmap) }
                                    }
                                }
                                div { class: "progress-track",
                                    div { class: "progress-buffer", style: "width: {pct(buffered())}%" }
                                    if settings.sb_show_bar {
                                        for s in segs.iter().filter(|s| s.action != "full" && s.action != "chapter") {
                                            div { key: "{s.uuid}", class: "progress-seg",
                                                style: "left: {pct(s.start)}%; width: {(pct(s.end) - pct(s.start)).max(0.4)}%; background: {cat_color(&s.category)}",
                                                title: "{cat_name(&s.category)}",
                                            }
                                        }
                                    }
                                    div { class: "progress-play", style: "width: {pct(time())}%" }
                                    for c in chapters.iter().skip(1) {
                                        div { class: "progress-gap", style: "left: {pct(c.start)}%" }
                                    }
                                    div { class: "progress-knob", style: "left: {pct(time())}%" }
                                }
                                if let Some((p, t)) = hover() {
                                    div { class: "pointer-events-none absolute bottom-6 flex -translate-x-1/2 flex-col items-center gap-1 whitespace-nowrap text-xs",
                                        style: if storyboard.is_some() && big { format!("left: clamp(6rem, {p}%, calc(100% - 6rem))") } else { format!("left: clamp(3rem, {p}%, calc(100% - 3rem))") },
                                        if big {
                                            if let Some(sb) = storyboard.as_ref() {
                                                {storyboard_frame(sb, t)}
                                            }
                                        }
                                        div { class: "rounded bg-black/90 px-2 py-1 text-center",
                                            if let Some(c) = hover_chapter.clone() { div { class: "font-medium", "{c}" } }
                                            div { class: "tabular-nums", {duration(t)} }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "flex items-center gap-1",
                            CtlBtn { title: if playing() { "Pause (K)" } else { "Abspielen (K)" }, onclick: move |_| ctl_c.toggle_play(),
                                if playing() { icons::Pause { class: "size-5" } } else { icons::Play { class: "size-5" } }
                            }
                            if big {
                                CtlBtn { title: "Nächstes Video (N)", onclick: move |_| {
                                        if let Some(n) = NOW.peek().clone() { spawn(async move { if let Some(r) = next_video(&n, true).await { navigator().push(r); } }); }
                                    },
                                    icons::SkipForward { class: "size-5" }
                                }
                                div { class: "group/vol flex items-center",
                                    CtlBtn { title: "Ton (M)", desktop: true, onclick: move |_| ctl_c.toggle_mute(),
                                        if muted() || volume() == 0.0 { icons::VolumeX { class: "size-5" } } else { icons::Volume2 { class: "size-5" } }
                                    }
                                    input { r#type: "range", class: "range always w-0 opacity-0 transition-all group-hover/vol:w-20 group-hover/vol:opacity-100",
                                        min: "0", max: "1", step: "0.02",
                                        value: format!("{}", if muted() { 0.0 } else { volume() }),
                                        style: format!("--value: {}%", if muted() { 0.0 } else { volume() * 100.0 }),
                                        oninput: move |e| {
                                            let n: f64 = e.value().parse().unwrap_or(1.0);
                                            if let Some(v) = video() { v.set_volume(n); v.set_muted(false); }
                                        },
                                        onchange: move |e| { let n: f64 = e.value().parse().unwrap_or(1.0); update_settings(|s| s.volume = n); },
                                    }
                                }
                            }
                            div { class: "ml-1 min-w-0 truncate whitespace-nowrap text-xs tabular-nums text-white/90",
                                if is_live {
                                    span { class: "rounded bg-red-600 px-1.5 py-0.5 font-semibold", "LIVE" }
                                    if let Some(secs) = live_for {
                                        span { class: "ml-2 text-white/80", "Live seit {live_duration(secs)}" }
                                    }
                                } else {
                                    {format!("{} / {}", duration(time()), duration(d_total))}
                                }
                                if big {
                                    if let Some(c) = cur_chapter.clone() { span { class: "ml-2 text-white/70", "• {c}" } }
                                }
                            }
                            div { class: "flex-1" }
                            if big {
                                CtlBtn { title: "Untertitel (C)", active: (ctl.captions)().is_some(), onclick: move |_| menu.set(if menu() == Menu::Captions { Menu::None } else { Menu::Captions }),
                                    icons::Captions { class: "size-5" }
                                }
                                CtlBtn { title: "Einstellungen", onclick: move |_| {
                                        if menu() == Menu::Settings { menu.set(Menu::None); return; }
                                        menu.set(Menu::Settings);
                                        spawn(async move {
                                            if let Ok(l) = dioxus::document::eval(&format!("return window.nrzLevels({})", util::js_str(VID))).join::<Levels>().await { levels.set(l); }
                                            if let Ok(a) = dioxus::document::eval(&format!("return window.nrzAudioTracks({})", util::js_str(VID))).join::<AudioTracks>().await { audios.set(a); }
                                        });
                                    },
                                    icons::Settings { class: "size-5" }
                                }
                                CtlBtn { title: "Bildschirmfoto (S)", desktop: true, onclick: move |_| ctl_c.screenshot(), icons::Camera { class: "size-5" } }
                                if !is_twitch_id(&id) { CtlBtn { title: "Externer Player", desktop: true, onclick: move |_| menu.set(if menu() == Menu::External { Menu::None } else { Menu::External }), icons::ExternalLink { class: "size-5" } } }
                                CtlBtn { title: "In neuem Fenster", desktop: true, onclick: move |_| {
                                        let t = video().map(|v| v.current_time()).unwrap_or(0.0);
                                        if let Some(v) = video() { let _ = v.pause(); }
                                        let n = NOW.peek().clone();
                                        if let Some(n) = n { util::open_window(&format!("{}&t={}", watch_href(&n.id, n.list.as_deref()), t as u64), true); }
                                    },
                                    icons::AppWindow { class: "size-5" }
                                }
                                CtlBtn { title: "Bild-im-Bild (I)", onclick: move |_| util::eval(&format!("window.nrzPip({})", util::js_str(VID))), icons::PictureInPicture2 { class: "size-5" } }
                                CtlBtn { title: "Kinomodus (T)", desktop: true, onclick: move |_| update_settings(|s| s.theater = !s.theater), icons::RectangleHorizontal { class: "size-5" } }
                                CtlBtn { title: "Vollbild (F)", onclick: move |_| { util::eval(&format!("window.nrzFullscreen({})", util::js_str(BOX))); fullscreen.set(!fullscreen()); },
                                    if fullscreen() { icons::Minimize { class: "size-5" } } else { icons::Maximize { class: "size-5" } }
                                }
                            } else {
                                CtlBtn { title: "Gross anzeigen", onclick: move |_| {
                                        let n = now_for_expand.clone();
                                        let list = n.local_list.map(|p| format!("local:{p}")).or(n.list.clone()).unwrap_or_default();
                                        nav.push(Route::Watch { v: n.id.clone(), list, t: String::new() });
                                    },
                                    icons::Expand { class: "size-4" }
                                }
                                CtlBtn { title: "Schliessen", onclick: move |_| {
                                        if let Some(v) = video() { let _ = v.pause(); }
                                        util::eval(&format!("window.nrzDetach({})", util::js_str(VID)));
                                        *NOW.write() = None;
                                    },
                                    icons::X { class: "size-4" }
                                }
                            }
                        }
                    }
                }

                // Menüs
                if menu() != Menu::None && big {
                    div { class: "absolute bottom-14 right-3 z-10 max-h-[70%] w-64 overflow-y-auto rounded-xl border border-white/10 bg-black/90 p-1.5 text-sm text-white shadow-2xl",
                        onclick: move |e| e.stop_propagation(),
                        match menu() {
                            Menu::Settings => rsx! {
                                MenuItem { label: "Qualität", value: quality_label(&levels(), audio_only()), onclick: move |_| menu.set(Menu::Quality) }
                                if audios().tracks.len() > 1 {
                                    MenuItem { label: "Audiospur", value: audios().tracks.iter().find(|t| t.index == audios().current).map(|t| short_track(&t.name)).unwrap_or_default(), onclick: move |_| menu.set(Menu::Audio) }
                                }
                                MenuItem { label: "Geschwindigkeit", value: format!("{}×", rate()), onclick: move |_| menu.set(Menu::Speed) }
                                MenuItem { label: "Nur Ton", value: if audio_only() { "an" } else { "aus" }, onclick: move |_| { audio_only.set(!audio_only()); menu.set(Menu::None); } }
                                MenuItem { label: "Wiederholen", value: if settings.loop_video { "an" } else { "aus" }, onclick: move |_| {
                                    let n = !SETTINGS.peek().loop_video; update_settings(|s| s.loop_video = n); if let Some(v) = video() { v.set_loop(n); } } }
                                MenuItem { label: "Tastenkürzel", value: "?", onclick: move |_| { *SHORTCUTS.write() = true; menu.set(Menu::None); } }
                                // Auf dem Handy sind diese Knöpfe nicht in der Leiste
                                div { class: "sm:hidden",
                                    MenuItem { label: "Bildschirmfoto", value: "", onclick: move |_| { menu.set(Menu::None); ctl_c.screenshot(); } }
                                    MenuItem { label: "Externer Player", value: "", onclick: move |_| menu.set(Menu::External) }
                                    if SETTINGS().stream_enabled {
                                    MenuItem { label: "Zur Stream-Liste", value: "", onclick: move |_| { menu.set(Menu::None); if let Some(n) = NOW.peek().clone() { add_to_stream(current_item(&n.id)); } } }
                                    }
                                }
                            },
                            Menu::Quality => rsx! {
                                MenuBack { onclick: move |_| menu.set(Menu::Settings), "Qualität" }
                                MenuChoice { label: "Automatisch", selected: levels().auto && !audio_only(), onclick: move |_| {
                                    audio_only.set(false);
                                    util::eval(&format!("window.nrzSetLevel({}, -1)", util::js_str(VID))); menu.set(Menu::None); } }
                                for l in levels().levels.iter().rev().cloned() {
                                    MenuChoice { key: "{l.index}", label: format!("{}p{}", l.height, if l.fps > 30 { l.fps.to_string() } else { String::new() }),
                                        selected: !levels().auto && levels().current == l.index,
                                        onclick: move |_| { util::eval(&format!("window.nrzSetLevel({}, {})", util::js_str(VID), l.index)); menu.set(Menu::None); } }
                                }
                                if levels().levels.is_empty() { div { class: "px-3 py-2 text-xs text-white/60", "Einzelne Datei – keine Auswahl" } }
                            },
                            Menu::Audio => rsx! {
                                MenuBack { onclick: move |_| menu.set(Menu::Settings), "Audiospur" }
                                for t in audios().tracks.iter().cloned() {
                                    MenuChoice { key: "{t.index}", label: track_label(&t), selected: audios().current == t.index,
                                        onclick: move |_| {
                                            util::eval(&format!("window.nrzSetAudio({}, {})", util::js_str(VID), t.index));
                                            audios.with_mut(|a| a.current = t.index);
                                            menu.set(Menu::None);
                                        } }
                                }
                            },
                            Menu::Speed => rsx! {
                                MenuBack { onclick: move |_| menu.set(Menu::Settings), "Geschwindigkeit" }
                                for s in [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0] {
                                    MenuChoice { key: "{s}", label: if s == 1.0 { "Normal".to_string() } else { format!("{s}×") }, selected: (rate() - s).abs() < 0.01,
                                        onclick: move |_| { if let Some(v) = video() { v.set_playback_rate(s); } update_settings(|x| x.speed = s); menu.set(Menu::None); } }
                                }
                            },
                            Menu::Captions => rsx! {
                                MenuChoice { label: "Aus", selected: (ctl.captions)().is_none(), onclick: move |_| { ctl_c.set_captions(None); menu.set(Menu::None); } }
                                for c in captions.iter().cloned() {
                                    MenuChoice { key: "{c.lang}-{c.auto}", label: c.name.clone(), selected: (ctl.captions)().as_deref() == Some(c.lang.as_str()),
                                        onclick: move |_| { ctl_c.set_captions(Some(c.lang.clone())); menu.set(Menu::None); } }
                                }
                                if captions.is_empty() { div { class: "px-3 py-2 text-xs text-white/60", "Keine Untertitel vorhanden" } }
                            },
                            Menu::External => rsx! { ExternalMenu { id: id.clone(), title: title.clone(), onclose: move |_| menu.set(Menu::None) } },
                            Menu::None => rsx! {},
                        }
                    }
                }
            }
            if !big {
                // Leiste: ziehen, ein-/ausklappen, gross, schliessen
                div { class: "mini-drag flex items-center gap-1 py-1.5 pl-1.5 pr-1",
                    ondoubleclick: move |_| util::eval(&format!("window.nrzMiniReset({})", util::js_str(WRAP))),
                    title: "Ziehen zum Verschieben, Doppelklick: zurück nach unten rechts",
                    icons::GripVertical { class: "size-4 shrink-0 text-muted-foreground/70" }
                    if collapsed {
                        img { class: "h-9 w-16 shrink-0 rounded object-cover", src: "{poster_for(&now.id)}", alt: "" }
                    }
                    Link { to: watch_route(&now.id, now.list.as_deref(), None), class: "min-w-0 flex-1 px-1.5",
                        div { class: "truncate text-sm font-medium", if title.is_empty() { "Lädt …" } else { "{title}" } }
                        div { class: "truncate text-xs text-muted-foreground", "{author}" }
                    }
                    if collapsed {
                        button { class: "flex size-8 shrink-0 items-center justify-center rounded-md hover:bg-accent", title: if playing() { "Pause" } else { "Abspielen" },
                            onclick: move |_| ctl_c.toggle_play(),
                            if playing() { icons::Pause { class: "size-4" } } else { icons::Play { class: "size-4 fill-current" } }
                        }
                    }
                    button { class: "flex size-8 shrink-0 items-center justify-center rounded-md hover:bg-accent", title: if collapsed { "Aufklappen" } else { "Einklappen" },
                        onclick: move |_| { let c = MINI_COLLAPSED(); *MINI_COLLAPSED.write() = !c; },
                        if collapsed { icons::ChevronUp { class: "size-4" } } else { icons::ChevronDown { class: "size-4" } }
                    }
                    button { class: "flex size-8 shrink-0 items-center justify-center rounded-md hover:bg-accent", title: "Gross anzeigen",
                        onclick: move |_| {
                            let n = now_for_bar.clone();
                            let list = n.local_list.map(|p| format!("local:{p}")).or(n.list.clone()).unwrap_or_default();
                            *MINI_COLLAPSED.write() = false;
                            nav.push(Route::Watch { v: n.id.clone(), list, t: String::new() });
                        },
                        icons::Maximize2 { class: "size-4" }
                    }
                    button { class: "flex size-8 shrink-0 items-center justify-center rounded-md hover:bg-accent", title: "Schliessen",
                        onclick: move |_| {
                            if let Some(v) = video() { let _ = v.pause(); }
                            util::eval(&format!("window.nrzDetach({})", util::js_str(VID)));
                            *MINI_COLLAPSED.write() = false;
                            *NOW.write() = None;
                        },
                        icons::X { class: "size-4" }
                    }
                }
            }
        }
    }
}

fn quality_label(l: &Levels, audio: bool) -> String {
    if audio {
        return "Nur Ton".into();
    }
    if l.auto || l.current < 0 {
        return "Automatisch".into();
    }
    l.levels.iter().find(|x| x.index == l.current).map(|x| format!("{}p", x.height)).unwrap_or_else(|| "Automatisch".into())
}

/// Kurve "Meistgesehen": weich (Catmull-Rom als Bézier), Punkte jeweils in der Mitte ihres
/// Abschnitts, damit die Spitzen genau über den beliebten Stellen der Zeitleiste liegen
fn heat_path(h: &[f32]) -> String {
    if h.is_empty() {
        return String::new();
    }
    let n = h.len() as f32;
    let pts: Vec<(f32, f32)> = std::iter::once((0.0, 100.0 - h[0].clamp(0.0, 1.0) * 92.0))
        .chain(h.iter().enumerate().map(|(i, v)| ((i as f32 + 0.5) / n * 1000.0, 100.0 - v.clamp(0.0, 1.0) * 92.0)))
        .chain(std::iter::once((1000.0, 100.0 - h[h.len() - 1].clamp(0.0, 1.0) * 92.0)))
        .collect();
    let mut d = format!("M0,100 L{:.1},{:.1} ", pts[0].0, pts[0].1);
    for i in 0..pts.len() - 1 {
        let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = if i + 2 < pts.len() { pts[i + 2] } else { p2 };
        let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
        let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
        d += &format!("C{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} ", c1.0, c1.1.min(100.0), c2.0, c2.1.min(100.0), p2.0, p2.1);
    }
    d += "L1000,100 Z";
    d
}

/// Vorschaubild für eine Stelle der Zeitleiste aus dem Storyboard-Bogen
fn storyboard_frame(sb: &Storyboard, t: f64) -> Element {
    if sb.interval <= 0.0 || sb.sheets.is_empty() || sb.width == 0 {
        return rsx! {};
    }
    let per = (sb.rows * sb.cols) as usize;
    let idx = (t / sb.interval).floor().max(0.0) as usize;
    let sheet = (idx / per).min(sb.sheets.len() - 1);
    let within = idx % per;
    let (col, row) = ((within as u32) % sb.cols, (within as u32) / sb.cols);
    // Anzeige 192 px breit, Höhe nach Seitenverhältnis
    let w = 192.0;
    let k = w / sb.width as f64;
    let h = sb.height as f64 * k;
    let style = format!(
        "width: {w}px; height: {h:.0}px; background-image: url('{}'); background-size: {:.0}px {:.0}px; background-position: -{:.0}px -{:.0}px;",
        sb.sheets[sheet],
        sb.cols as f64 * w,
        sb.rows as f64 * h,
        col as f64 * w,
        row as f64 * h
    );
    rsx! { div { class: "overflow-hidden rounded-md border border-white/20 bg-black bg-no-repeat shadow-xl", style: "{style}" } }
}

pub fn cat_name(c: &str) -> String {
    SB_CATEGORIES.iter().find(|x| x.0 == c).map(|x| x.1.to_string()).unwrap_or_else(|| c.to_string())
}

fn cat_color(c: &str) -> &'static str {
    SB_CATEGORIES.iter().find(|x| x.0 == c).map(|x| x.2).unwrap_or("#888")
}

#[component]
fn CtlBtn(title: String, onclick: EventHandler<MouseEvent>, children: Element, #[props(default)] active: bool, #[props(default)] desktop: bool) -> Element {
    // `desktop`: auf dem Handy ausgeblendet (dort im Zahnrad-Menü)
    let vis = if desktop { "hidden sm:flex" } else { "flex" };
    rsx! {
        button {
            class: if active { "{vis} size-9 shrink-0 items-center justify-center rounded-md text-white bg-white/15" } else { "{vis} size-9 shrink-0 items-center justify-center rounded-md text-white/90 hover:bg-white/15 hover:text-white" },
            title: "{title}",
            aria_label: "{title}",
            onclick: move |e| { e.stop_propagation(); onclick.call(e) },
            {children}
        }
    }
}

#[component]
fn MenuItem(label: String, value: String, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button { class: "flex w-full items-center justify-between gap-3 rounded-lg px-3 py-2 hover:bg-white/10", onclick: move |e| onclick.call(e),
            span { "{label}" }
            span { class: "flex items-center gap-1 text-white/60", "{value}" icons::ChevronRight { class: "size-4" } }
        }
    }
}

#[component]
fn MenuBack(onclick: EventHandler<MouseEvent>, children: Element) -> Element {
    rsx! {
        button { class: "mb-1 flex w-full items-center gap-2 border-b border-white/10 px-2 py-2 font-medium", onclick: move |e| onclick.call(e),
            icons::ChevronLeft { class: "size-4" } {children}
        }
    }
}

#[component]
fn MenuChoice(label: String, selected: bool, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button { class: "flex w-full items-center gap-2 rounded-lg px-3 py-1.5 text-left hover:bg-white/10", onclick: move |e| onclick.call(e),
            span { class: "w-4", if selected { icons::Check { class: "size-4" } } }
            "{label}"
        }
    }
}

/// Video in einem externen Player öffnen: VLC, IINA, Infuse, mpv (über M3U) oder Adresse kopieren
#[component]
fn ExternalMenu(id: String, title: String, onclose: EventHandler<()>) -> Element {
    let vid = id.clone();
    let links = use_resource(move || {
        let vid = vid.clone();
        async move { api::get::<ExternalLinks>(&format!("/api/video/{vid}/external")).await }
    });
    let t = title.clone();
    rsx! {
        div { class: "px-3 py-2 text-xs text-white/60", "Signierte Adresse, einige Stunden gültig" }
        match links() {
            None => rsx! { div { class: "flex items-center gap-2 px-3 py-2", icons::LoaderCircle { class: "size-4 animate-spin" } "Lädt …" } },
            Some(Err(e)) => rsx! { div { class: "px-3 py-2 text-xs", "{e}" } },
            Some(Ok(l)) => {
                let main = l.hls.clone().or(l.progressive.clone()).unwrap_or_default();
                let file = l.progressive.clone().unwrap_or_else(|| main.clone());
                let m3u = format!("#EXTM3U\n#EXTINF:-1,{}\n{}\n", t, main);
                let name: String = t.chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect::<String>();
                let enc = urlencoding::encode(&main).into_owned();
                let enc_file = urlencoding::encode(&file).into_owned();
                let (m1, m2, m3, m4, m5) = (main.clone(), enc.clone(), enc_file.clone(), main.clone(), l.audio.clone());
                rsx! {
                    MenuChoice { label: "VLC", selected: false, onclick: move |_| { util::window().location().set_href(&format!("vlc://{m1}")).ok(); onclose.call(()); } }
                    MenuChoice { label: "IINA (macOS)", selected: false, onclick: move |_| { util::window().location().set_href(&format!("iina://weblink?url={m2}")).ok(); onclose.call(()); } }
                    MenuChoice { label: "Infuse", selected: false, onclick: move |_| { util::window().location().set_href(&format!("infuse://x-callback-url/play?url={m3}")).ok(); onclose.call(()); } }
                    MenuChoice { label: "M3U-Datei (mpv, VLC, …)", selected: false, onclick: move |_| {
                        util::eval(&format!("window.nrzDownload({}, {}, 'audio/x-mpegurl')", util::js_str(&format!("{}.m3u", name.trim())), util::js_str(&m3u)));
                        onclose.call(()); } }
                    MenuChoice { label: "Adresse kopieren", selected: false, onclick: move |_| { util::copy(&m4); toast("Adresse kopiert – z. B. mpv \"<Adresse>\""); onclose.call(()); } }
                    if let Some(a) = m5 {
                        MenuChoice { label: "Nur-Ton-Adresse kopieren", selected: false, onclick: move |_| { util::copy(&a); toast("Adresse kopiert"); onclose.call(()); } }
                    }
                }
            }
        }
    }
}

/// "Deutsch - dubbed" -> "Deutsch (synchronisiert)", "English - original" -> "English (Original)"
fn track_label(t: &AudioTrack) -> String {
    let n = t.name.replace(" - dubbed", " (synchronisiert)").replace(" - dubbed-auto", " (automatisch synchronisiert)").replace(" original", " (Original)").replace(" - original", " (Original)");
    if n.trim().is_empty() { t.lang.clone() } else { n }
}

fn short_track(name: &str) -> String {
    name.split(" - ").next().unwrap_or(name).trim().to_string()
}

/// "1:02:03" bzw. ab einem Tag "11 Tg. 06:46:02"
fn live_duration(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (d, h, m, sec) = (s / 86400, (s % 86400) / 3600, (s % 3600) / 60, s % 60);
    if d > 0 {
        format!("{d} Tg. {h:02}:{m:02}:{sec:02}")
    } else if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}
