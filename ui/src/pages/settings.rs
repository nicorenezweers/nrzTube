//! Einstellungen: Allgemein, Player, Datenschutz, SponsorBlock, DeArrow, Ablenkungsfrei,
//! Daten und Konto. Jede Änderung wird sofort gespeichert.

use crate::Route;
use crate::api;
use crate::components::*;
use crate::pages::subscriptions::DataSection;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use dioxus::prelude::*;
use nrztube_types::*;

const TABS: &[(&str, &str)] = &[
    ("general", "Allgemein"),
    ("reco", "Empfehlungen"),
    ("stream", "Stream-Modus"),
    ("twitch", "Twitch"),
    ("player", "Player"),
    ("privacy", "Datenschutz & Jugendschutz"),
    ("sponsorblock", "SponsorBlock"),
    ("dearrow", "DeArrow"),
    ("distraction", "Ablenkungsfrei"),
    ("data", "Daten"),
    ("recap", "Recap"),
    ("account", "Konto"),
];

fn o(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

macro_rules! toggle {
    ($s:expr, $field:ident, $title:expr) => {
        toggle!($s, $field, $title, None::<&str>)
    };
    ($s:expr, $field:ident, $title:expr, $text:expr) => {
        rsx! {
            SettingRow { text: $text.map(|t: &str| t.to_string()), title: $title.to_string(),
                Toggle { checked: $s.$field, onchange: move |v: bool| update_settings(|x| x.$field = v) }
            }
        }
    };
}

#[component]
pub fn SettingsPage(tab: String) -> Element {
    let current = if tab.is_empty() { "general".to_string() } else { tab.clone() };
    rsx! {
        div { class: "mx-auto max-w-5xl p-4 md:p-6",
            PageTitle { title: "Einstellungen" }
            div { class: "flex flex-col gap-6 md:flex-row",
                nav { class: "shelf flex shrink-0 gap-1 overflow-x-auto md:w-56 md:flex-col",
                    for (k, l) in TABS.iter() {
                        Link { key: "{k}", to: Route::SettingsPage { tab: k.to_string() },
                            class: if *k == current { "shrink-0 rounded-lg bg-accent px-3 py-2 text-sm font-medium" } else { "shrink-0 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-accent/60 hover:text-foreground" },
                            "{l}"
                        }
                    }
                }
                div { class: "min-w-0 flex-1",
                    match current.as_str() {
                        "player" => rsx! { PlayerSettings {} },
                        "reco" => rsx! { RecoSettings {} },
                        "stream" => rsx! { StreamSettings {} },
                        "twitch" => rsx! { TwitchSettings {} },
                        "privacy" => rsx! { PrivacySettings {} },
                        "sponsorblock" => rsx! { SponsorBlockSettings {} },
                        "dearrow" => rsx! { DeArrowSettings {} },
                        "distraction" => rsx! { DistractionSettings {} },
                        "data" => rsx! { DataSection {} },
                        "recap" => rsx! { crate::pages::recap::RecapSettings {} },
                        "account" => rsx! { AccountSettings {} },
                        _ => rsx! { GeneralSettings {} },
                    }
                }
            }
        }
    }
}

#[component]
fn Section(title: String, #[props(default)] text: Option<String>, children: Element) -> Element {
    rsx! {
        section { class: "mb-6 rounded-2xl border bg-card px-5 py-3",
            h2 { class: "pt-2 text-base font-semibold", "{title}" }
            if let Some(t) = text { p { class: "text-xs text-muted-foreground", "{t}" } }
            div { class: "divide-y", {children} }
        }
    }
}

#[component]
fn GeneralSettings() -> Element {
    let s = SETTINGS();
    rsx! {
        Section { title: "Allgemein",
            SettingRow { title: "Startseite",
                Select { value: s.start_page.clone(), options: o(&[("home", "Startseite (Vorschläge)"), ("feed", "Abos-Feed"), ("subscriptions", "Abos & Profile"), ("playlists", "Playlists"), ("history", "Verlauf")]), onchange: move |v| update_settings(|x| x.start_page = v) }
            }
            SettingRow { title: "Region", text: "Für Trends, Suche und Sprache der Inhalte".to_string(),
                Select { value: s.region.clone(), options: o(&[("CH", "Schweiz"), ("DE", "Deutschland"), ("AT", "Österreich"), ("US", "USA"), ("GB", "Grossbritannien"), ("FR", "Frankreich"), ("IT", "Italien"), ("ES", "Spanien"), ("NL", "Niederlande"), ("JP", "Japan")]), onchange: move |v| update_settings(|x| x.region = v) }
            }
            SettingRow { title: "Sprache der YouTube-Texte",
                Select { value: s.language.clone(), options: o(&[("de", "Deutsch"), ("en", "Englisch"), ("fr", "Französisch"), ("it", "Italienisch"), ("es", "Spanisch")]), onchange: move |v| update_settings(|x| x.language = v) }
            }
            {toggle!(s, list_view, "Listenansicht", Some("Videos als Liste statt als Raster"))}
            {toggle!(s, open_in_new_window, "Videos in neuem Fenster öffnen", Some("Jedes Video öffnet ein eigenes Fenster (mehrere Fenster gleichzeitig)"))}
        }
    }
}

#[component]
fn RecoSettings() -> Element {
    let s = SETTINGS();
    let mut taste = use_resource(|| async { api::get::<Taste>("/api/taste").await });
    let t = taste.read().clone();
    rsx! {
        Section { title: "Startseite mit Vorschlägen", text: "nrzTube lernt aus deinem Verlauf: welche Kanäle du öffnest, wie lange du wirklich schaust, was du magst oder ablehnst und wonach du suchst. Gerechnet wird auf deinem eigenen Server, gespeichert nur in der lokalen Datenbank – Google erfährt davon nichts.".to_string(),
            {toggle!(s, reco_enabled, "Vorschläge auf der Startseite", Some("Aus: Die Startseite zeigt den Abo-Feed"))}
            {toggle!(s, ryd, "Dislike-Zahlen zeigen", Some("Von Return YouTube Dislike – der Server fragt dort nur die Video-ID ab"))}
        }
        Section { title: "Was nrzTube über deinen Geschmack weiss",
            match t {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message } },
                Some(Ok(t)) => rsx! {
                    div { class: "grid grid-cols-2 gap-3 py-3 sm:grid-cols-4",
                        Stat { label: "Minuten geschaut", value: format!("{}", t.minutes.round() as u32) }
                        Stat { label: "Videos im Verlauf", value: t.videos.to_string() }
                        Stat { label: "Gefällt mir", value: t.likes.to_string() }
                        Stat { label: "Nicht interessiert", value: t.dislikes.to_string() }
                    }
                    if !t.searches.is_empty() {
                        div { class: "pb-3 text-xs text-muted-foreground", "Letzte Suchen: " {t.searches.join(", ")} }
                    }
                    if t.channels.is_empty() {
                        div { class: "py-3 text-sm text-muted-foreground", "Noch keine Daten – schau ein paar Videos." }
                    }
                    div { class: "flex flex-col divide-y",
                        for (i, c) in t.channels.iter().take(15).cloned().enumerate() {
                            {
                                let max = t.channels.first().map(|x| x.score).unwrap_or(1.0).max(0.01);
                                let w = ((c.score / max).clamp(0.0, 1.0) * 100.0) as u32;
                                rsx! {
                                    div { key: "{c.channel_id}", class: "flex items-center gap-3 py-2.5",
                                        span { class: "w-5 text-right text-xs text-muted-foreground tabular-nums", "{i + 1}" }
                                        if c.thumb.is_empty() {
                                            span { class: "size-8 shrink-0 rounded-full bg-muted" }
                                        } else {
                                            img { class: "size-8 shrink-0 rounded-full object-cover", src: "{c.thumb}", alt: "" }
                                        }
                                        div { class: "min-w-0 flex-1",
                                            div { class: "flex items-center justify-between gap-2 text-sm",
                                                Link { to: channel_route(&c.channel_id), class: "truncate font-medium hover:underline", "{c.name}" }
                                                span { class: "shrink-0 text-xs text-muted-foreground tabular-nums",
                                                    "{c.minutes.round() as u32} min • {c.videos} Videos"
                                                    if c.likes > 0 { " • 👍 {c.likes}" }
                                                    if c.dislikes > 0 { " • 👎 {c.dislikes}" }
                                                }
                                            }
                                            div { class: "mt-1 h-1.5 overflow-hidden rounded-full bg-muted",
                                                div { class: if c.score < 0.0 { "h-full bg-destructive" } else { "h-full bg-foreground/70" }, style: "width: {w}%" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            }
            div { class: "flex justify-end py-3",
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, onclick: move |_| {
                        spawn(async move {
                            match api::delete::<serde_json::Value>("/api/taste").await {
                                Ok(_) => { toast("Empfehlungsdaten zurückgesetzt"); taste.restart(); }
                                Err(e) => toast_err(e),
                            }
                        });
                    },
                    icons::RotateCcw {} "Empfehlungsdaten zurücksetzen"
                }
            }
        }
    }
}

#[component]
fn Stat(label: String, value: String) -> Element {
    rsx! {
        div { class: "rounded-xl bg-muted/50 p-3",
            div { class: "text-xl font-semibold tabular-nums", "{value}" }
            div { class: "text-xs text-muted-foreground", "{label}" }
        }
    }
}

#[component]
fn StreamSettings() -> Element {
    let s = SETTINGS();
    rsx! {
        Section { title: "Stream-Modus", text: "Für Livestreams: Im privaten Modus sammelst du Videos per Rechtsklick (Handy: lange drücken) in der Stream-Liste. Im Stream-Modus spielst du sie der Reihe nach ab – ohne Verlauf, Abos, Profile und persönliche Vorschläge auf dem Bildschirm.".to_string(),
            SettingRow { title: "Stream-Liste und Stream-Modus", text: "Videos per Rechtsklick sammeln und am Stück abspielen (z. B. für Livestreams). Aus: alle Stream-Knöpfe sind ausgeblendet.".to_string(),
                Toggle { checked: s.stream_enabled, onchange: move |v: bool| update_settings(|x| { x.stream_enabled = v; if !v { x.stream_mode = false; } }) }
            }
            {toggle!(s, stream_mode, "Stream-Modus", Some("Startseite zeigt die Stream-Liste; Verlauf, Abos und Vorschläge sind ausgeblendet"))}
            {toggle!(s, stream_private, "Privat streamen", Some("Was im Stream-Modus läuft, kommt nicht in Verlauf und Empfehlungsdaten"))}
            {toggle!(s, stream_remove_done, "Fertig Geschautes entfernen", Some("Am Ende eines Videos fällt es aus der Stream-Liste, das nächste startet"))}
            SettingRow { title: "Videos aus der Stream-Liste", text: "In Abo-Feed und auf der Startseite".to_string(),
                Select { value: if s.stream_in_feed == "show" { "show".to_string() } else { "hide".to_string() }, options: o(&[("hide", "Ausblenden"), ("show", "Normal anzeigen")]), onchange: move |v| update_settings(|x| x.stream_in_feed = v) }
            }
        }
    }
}

#[component]
fn TwitchSettings() -> Element {
    let s = SETTINGS();
    rsx! {
        Section { title: "Twitch", text: "Livestreams, VODs und Clips von Twitch – ohne Konto und ohne Werbung, alles über deinen Server. YouTube- und Twitch-Kanäle derselben Person werden automatisch verknüpft (über ihre gegenseitigen Links oder den gleichen Namen).".to_string(),
            {toggle!(s, twitch_enabled, "Twitch anzeigen", Some("Seitenleiste, Suche, Kanal-Verknüpfungen und Live-Hinweise"))}
            {toggle!(s, twitch_live_row, "„Jetzt live“ auf Startseite und im Feed", Some("YouTube-Abos, die gerade live sind, gefolgte Twitch-Kanäle und die Twitch-Kanäle deiner Abos"))}
            {toggle!(s, twitch_chat, "Livechat neben Livestreams", Some("Twitch und YouTube; auf breiten Bildschirmen rechts angedockt"))}
        }
        TwitchHidden {}
        TwitchAccount {}
    }
}

/// Kategorien ausblenden (wie unwanted-twitch); Kanäle und Wörter teilen sich YouTube und Twitch
#[component]
fn TwitchHidden() -> Element {
    let s = SETTINGS();
    let mut games = use_signal(|| s.twitch_hidden_games.join("\n"));
    let n_ch = s.hidden_channels.iter().filter(|c| c.starts_with("tw:")).count();
    rsx! {
        Section { title: "Ausblenden", text: "Kategorien, die nirgends erscheinen sollen (Übersicht, Live-Leisten, Kanalseiten). Kanäle und Wörter blendest du unter Datenschutz aus – oder per Rechtsklick auf ein Video.".to_string(),
            div { class: "py-3",
                textarea { class: "h-28 w-full rounded-md border border-input bg-input/30 p-3 text-sm outline-none", placeholder: "Eine Kategorie pro Zeile, z. B. Slots", value: "{games}",
                    oninput: move |e| games.set(e.value()),
                    onchange: move |_| { let v: Vec<String> = games().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(); update_settings(|x| x.twitch_hidden_games = v); } }
                if n_ch > 0 {
                    p { class: "mt-2 text-xs text-muted-foreground", "{n_ch} Twitch-Kanäle ausgeblendet (unter Datenschutz änderbar)." }
                }
            }
        }
    }
}

/// Twitch-Konto per Code verbinden (twitch.tv/activate) – nrzTube sieht nie das Passwort
#[component]
fn TwitchAccount() -> Element {
    let mut code = use_signal(|| None::<DeviceCode>);
    let mut busy = use_signal(|| false);
    use_hook(|| { spawn(async { reload_accounts().await; }); });
    // Während ein Code angezeigt wird: regelmässig nachfragen
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(3000).await;
            let Some(c) = code.peek().clone() else { continue };
            match api::post::<_, serde_json::Value>("/api/accounts/twitch/poll", &serde_json::json!({ "device_code": c.device_code })).await {
                Ok(v) if v.get("status").and_then(|x| x.as_str()) == Some("ok") => {
                    code.set(None);
                    reload_accounts().await;
                    toast(format!("Mit Twitch verbunden als {}", v.get("login").and_then(|x| x.as_str()).unwrap_or("")));
                }
                Ok(_) => {}
                Err(e) => { code.set(None); toast_err(e); }
            }
        }
    });
    let acc = ACCOUNTS();
    rsx! {
        Section { title: "Twitch-Konto", text: "Zum Mitschreiben im Chat und um deine gefolgten Kanäle zu übernehmen. Angemeldet wird mit einem Code auf twitch.tv – dein Passwort sieht nrzTube nie. Gespeichert wird nur ein Zugangsschlüssel auf deinem Server.".to_string(),
            if let Some(login) = acc.twitch.clone() {
                SettingRow { title: format!("Verbunden als {login}"), text: "Schreiben im Twitch-Chat ist aktiv".to_string(),
                    div { class: "flex shrink-0 gap-2",
                        Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, disabled: busy(), onclick: move |_| {
                            busy.set(true);
                            spawn(async move {
                                match api::post::<_, serde_json::Value>("/api/accounts/twitch/import", &()).await {
                                    Ok(v) => { toast(format!("{} Kanäle übernommen", v.get("added").and_then(|x| x.as_u64()).unwrap_or(0))); reload_tw_follows().await; reload_tw_live().await; }
                                    Err(e) => toast_err(e),
                                }
                                busy.set(false);
                            });
                        }, if busy() { icons::LoaderCircle { class: "animate-spin" } } else { icons::Download {} } "Gefolgte übernehmen" }
                        Button { variant: ButtonVariant::Ghost, size: ButtonSize::Sm, onclick: move |_| {
                            spawn(async move { let _ = api::delete::<serde_json::Value>("/api/accounts/twitch").await; reload_accounts().await; toast("Twitch getrennt"); });
                        }, icons::LogOut {} "Trennen" }
                    }
                }
            } else if let Some(c) = code() {
                div { class: "flex flex-col items-center gap-3 py-4 text-center",
                    p { class: "text-sm text-muted-foreground", "Öffne twitch.tv/activate (angemeldet) und gib diesen Code ein:" }
                    div { class: "rounded-xl border bg-background px-6 py-3 font-mono text-3xl font-bold tracking-[0.3em]", "{c.user_code}" }
                    div { class: "flex gap-2",
                        a { href: "{c.verification_uri}?device-code={c.user_code}", target: "_blank", rel: "noreferrer noopener",
                            class: "inline-flex h-8 items-center gap-1.5 rounded-md bg-[#9146ff] px-3 text-sm font-medium text-white hover:bg-[#a970ff]",
                            TwitchGlyph { class: "size-4" } "twitch.tv/activate öffnen"
                        }
                        Button { variant: ButtonVariant::Ghost, size: ButtonSize::Sm, onclick: move |_| code.set(None), "Abbrechen" }
                    }
                    p { class: "flex items-center gap-1.5 text-xs text-muted-foreground", icons::LoaderCircle { class: "size-3 animate-spin" } "Warte auf Bestätigung …" }
                }
            } else {
                SettingRow { title: "Nicht verbunden",
                    Button { size: ButtonSize::Sm, disabled: busy(), onclick: move |_| {
                        busy.set(true);
                        spawn(async move {
                            match api::post::<_, DeviceCode>("/api/accounts/twitch/start", &()).await {
                                Ok(c) => code.set(Some(c)),
                                Err(e) => toast_err(e),
                            }
                            busy.set(false);
                        });
                    }, TwitchGlyph { class: "size-4" } "Mit Twitch anmelden" }
                }
            }
        }
    }
}

#[component]
fn PlayerSettings() -> Element {
    let s = SETTINGS();
    rsx! {
        Section { title: "Wiedergabe",
            {toggle!(s, autoplay, "Automatisch abspielen", Some("Video startet beim Öffnen"))}
            {toggle!(s, autoplay_next, "Nächstes Video automatisch", Some("Nach dem Ende das erste ähnliche Video abspielen (Playlists laufen immer weiter)"))}
            {toggle!(s, remember_position, "An letzter Stelle weiterschauen")}
            {toggle!(s, loop_video, "Video wiederholen")}
            {toggle!(s, mini_player, "Mini-Player", Some("Beim Verlassen der Video-Seite in einer Ecke weiterspielen"))}
            {toggle!(s, theater, "Kinomodus", Some("Player über die ganze Breite (Taste T)"))}
            SettingRow { title: "Standard-Qualität",
                Select { value: s.quality.clone(), options: o(&[("auto", "Automatisch"), ("2160", "2160p (4K)"), ("1440", "1440p"), ("1080", "1080p"), ("720", "720p"), ("480", "480p"), ("360", "360p"), ("240", "240p"), ("144", "144p")]), onchange: move |v| update_settings(|x| x.quality = v) }
            }
            SettingRow { title: "Video-Codec", text: "H.264 spart Akku und läuft überall, VP9 erlaubt bis 4K".to_string(),
                Select { value: s.codec.clone(), options: o(&[("avc1", "H.264 (bis 1080p)"), ("any", "Alle (VP9, bis 4K)")]), onchange: move |v| update_settings(|x| x.codec = v) }
            }
            {toggle!(s, can_i_react, "Can I React", Some("Reaktionsrichtlinien der Kanäle unter dem Video (canireact.com)"))}
            SettingRow { title: "Audiospur", text: "Bei Videos mit mehreren Tonspuren (z. B. synchronisiert); sonst die Originalspur".to_string(),
                Select { value: s.audio_lang.clone(), options: o(&[("de", "Deutsch"), ("original", "Original"), ("en", "Englisch"), ("es", "Spanisch"), ("fr", "Französisch"), ("it", "Italienisch"), ("pt", "Portugiesisch"), ("pl", "Polnisch"), ("tr", "Türkisch"), ("ru", "Russisch"), ("ja", "Japanisch")]), onchange: move |v| update_settings(|x| x.audio_lang = v) }
            }
            SettingRow { title: "Standard-Geschwindigkeit",
                Select { value: format!("{}", s.speed), options: o(&[("0.5", "0.5×"), ("0.75", "0.75×"), ("1", "Normal"), ("1.25", "1.25×"), ("1.5", "1.5×"), ("1.75", "1.75×"), ("2", "2×")]), onchange: move |v: String| update_settings(|x| x.speed = v.parse().unwrap_or(1.0)) }
            }
            SettingRow { title: "Sprung mit J / L",
                Select { value: s.skip_seconds.to_string(), options: o(&[("5", "5 Sekunden"), ("10", "10 Sekunden"), ("15", "15 Sekunden"), ("30", "30 Sekunden")]), onchange: move |v: String| update_settings(|x| x.skip_seconds = v.parse().unwrap_or(10)) }
            }
        }
        Section { title: "Untertitel",
            {toggle!(s, captions, "Untertitel automatisch einschalten")}
            SettingRow { title: "Bevorzugte Sprache",
                Select { value: s.caption_lang.clone(), options: o(&[("de", "Deutsch"), ("en", "Englisch"), ("fr", "Französisch"), ("it", "Italienisch"), ("es", "Spanisch")]), onchange: move |v| update_settings(|x| x.caption_lang = v) }
            }
        }
        Section { title: "Bildschirmfoto & externer Player",
            SettingRow { title: "Format der Bildschirmfotos", text: "Taste S oder Kamera-Symbol im Player".to_string(),
                Select { value: s.screenshot_format.clone(), options: o(&[("png", "PNG"), ("jpg", "JPEG")]), onchange: move |v| update_settings(|x| x.screenshot_format = v) }
            }
            SettingRow { title: "Externer Player", text: "Über das Symbol im Player: VLC, IINA, Infuse, M3U (mpv) oder Adresse kopieren".to_string(),
                Select { value: s.external_player.clone(), options: o(&[("vlc", "VLC"), ("iina", "IINA"), ("infuse", "Infuse"), ("m3u", "M3U-Datei"), ("copy", "Adresse kopieren")]), onchange: move |v| update_settings(|x| x.external_player = v) }
            }
        }
        Section { title: "Tastenkürzel", text: "Player und Navigation lassen sich komplett per Tastatur steuern. Taste ? zeigt die Übersicht jederzeit.".to_string(),
            SettingRow { title: "Alle Tastenkürzel",
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, onclick: move |_| *SHORTCUTS.write() = true,
                    icons::Keyboard {} "Anzeigen"
                }
            }
        }
    }
}

#[component]
fn PrivacySettings() -> Element {
    let s = SETTINGS();
    let mut channels = use_signal(|| s.hidden_channels.join("\n"));
    let mut words = use_signal(|| s.hidden_keywords.join("\n"));
    rsx! {
        Section { title: "Datenschutz", text: "nrzTube holt alles über den eigenen Server: kein Google-Cookie, kein Google-JavaScript, keine Werbung. Vorschaubilder, Videos und Untertitel laufen über den Proxy.".to_string(),
            {toggle!(s, save_history, "Verlauf speichern")}
            {toggle!(s, save_position, "Position im Video speichern")}
        }
        Section { title: "Jugendschutz",
            {toggle!(s, family_friendly, "Nur familienfreundliche Inhalte", Some("Eingeschränkter Modus von YouTube für Suche und Listen; nicht jugendfreie Videos werden gesperrt"))}
        }
        Section { title: "Kanäle und Wörter ausblenden", text: "Eine Zeile pro Eintrag – Kanal-ID (UC…), Twitch-Kanal (tw:login) oder Kanalname; Wörter im Titel. Gilt für YouTube und Twitch.".to_string(),
            div { class: "grid gap-3 py-3 md:grid-cols-2",
                textarea { class: "h-32 rounded-md border border-input bg-input/30 p-3 text-sm outline-none", placeholder: "Kanäle", value: "{channels}",
                    oninput: move |e| channels.set(e.value()),
                    onchange: move |_| { let v: Vec<String> = channels().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(); update_settings(|x| x.hidden_channels = v); } }
                textarea { class: "h-32 rounded-md border border-input bg-input/30 p-3 text-sm outline-none", placeholder: "Wörter", value: "{words}",
                    oninput: move |e| words.set(e.value()),
                    onchange: move |_| { let v: Vec<String> = words().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(); update_settings(|x| x.hidden_keywords = v); } }
            }
        }
    }
}

#[component]
fn SponsorBlockSettings() -> Element {
    let s = SETTINGS();
    rsx! {
        Section { title: "SponsorBlock", text: "Gesponserte Abschnitte, Intros und mehr überspringen – Daten von sponsor.ajay.app, abgefragt mit Hash-Präfix (die Video-ID verlässt den Server nicht).".to_string(),
            {toggle!(s, sb_enabled, "SponsorBlock verwenden")}
            {toggle!(s, sb_show_bar, "Segmente in der Zeitleiste zeigen")}
            {toggle!(s, sb_notify, "Hinweis beim Überspringen")}
            SettingRow { title: "Mindestlänge", text: "Kürzere Segmente nicht automatisch überspringen".to_string(),
                Select { value: format!("{}", s.sb_min_length), options: o(&[("0", "Keine"), ("1", "1 Sekunde"), ("3", "3 Sekunden"), ("5", "5 Sekunden")]), onchange: move |v: String| update_settings(|x| x.sb_min_length = v.parse().unwrap_or(0.0)) }
            }
        }
        Section { title: "Kategorien",
            for (cat, label, color) in SB_CATEGORIES.iter() {
                SettingRow { key: "{cat}", title: label.to_string(),
                    div { class: "flex items-center gap-2",
                        span { class: "size-3 rounded-full", style: "background: {color}" }
                        Select { value: s.sb_categories.get(*cat).cloned().unwrap_or_else(|| "off".into()),
                            options: if *cat == "poi_highlight" { o(&[("show", "Knopf zeigen"), ("off", "Aus")]) } else { o(&[("skip", "Automatisch überspringen"), ("manual", "Knopf zum Überspringen"), ("show", "Nur anzeigen"), ("off", "Aus")]) },
                            onchange: move |v| update_settings(|x| { x.sb_categories.insert(cat.to_string(), v); }) }
                    }
                }
            }
        }
    }
}

#[component]
fn DeArrowSettings() -> Element {
    let s = SETTINGS();
    rsx! {
        Section { title: "DeArrow", text: "Reisserische Titel und Vorschaubilder durch Vorschläge der Community ersetzen (dearrow.ajay.app).".to_string(),
            {toggle!(s, dearrow_titles, "Bessere Titel")}
            {toggle!(s, dearrow_thumbs, "Bessere Vorschaubilder", Some("Standbild aus dem Video statt Clickbait-Bild"))}
            {toggle!(s, dearrow_show_original, "Originaltitel auf der Video-Seite zeigen")}
        }
    }
}

#[component]
fn DistractionSettings() -> Element {
    let s = SETTINGS();
    rsx! {
        Section { title: "Seiten und Navigation",
            {toggle!(s, hide_trending, "Trends ausblenden")}
            {toggle!(s, hide_playlists, "Playlists ausblenden")}
            {toggle!(s, hide_sidebar_subscriptions, "Abos in der Seitenleiste ausblenden")}
            {toggle!(s, hide_search_suggestions, "Suchvorschläge ausblenden")}
        }
        Section { title: "Video-Seite",
            {toggle!(s, hide_comments, "Kommentare ausblenden")}
            {toggle!(s, hide_related, "Ähnliche Videos ausblenden")}
            {toggle!(s, hide_end_suggestions, "Vorschläge am Ende ausblenden")}
            {toggle!(s, hide_description, "Beschreibung ausblenden")}
            {toggle!(s, hide_chapters, "Kapitel ausblenden")}
            {toggle!(s, hide_heatmap, "„Meistgesehen“-Kurve ausblenden")}
            {toggle!(s, hide_likes, "Likes ausblenden")}
        }
        Section { title: "Listen und Karten",
            {toggle!(s, hide_shorts, "Shorts ausblenden")}
            {toggle!(s, hide_live, "Livestreams ausblenden")}
            {toggle!(s, hide_upcoming, "Geplante Videos ausblenden")}
            {toggle!(s, hide_watched, "Gesehene Videos ausblenden")}
            {toggle!(s, hide_views, "Aufrufe ausblenden")}
            {toggle!(s, hide_upload_date, "Upload-Datum ausblenden")}
            {toggle!(s, hide_subscriber_counts, "Abonnentenzahlen ausblenden")}
            {toggle!(s, hide_channel_avatars, "Kanalbilder ausblenden")}
            {toggle!(s, hide_comment_avatars, "Bilder in Kommentaren ausblenden")}
            {toggle!(s, blur_thumbnails, "Vorschaubilder unscharf")}
        }
        Section { title: "Kanäle",
            {toggle!(s, hide_posts, "Beiträge ausblenden")}
            {toggle!(s, hide_channel_playlists, "Playlists im Kanal ausblenden")}
        }
    }
}

#[component]
fn AccountSettings() -> Element {
    let auth = AUTH().unwrap_or_default();
    let me = auth.me.clone().unwrap_or_default();
    let mut old = use_signal(String::new);
    let mut new = use_signal(String::new);
    let field = "h-9 w-full rounded-md border border-input bg-input/30 px-3 text-sm outline-none";
    use_hook(|| { spawn(async { reload_accounts().await; }); });
    let yt_on = ACCOUNTS().youtube;
    rsx! {
        Invites {}
        Section { title: "YouTube-Anmeldung für den Livechat", text: "Damit du in YouTube-Livechats mitschreiben kannst: cookies.txt von youtube.com (angemeldet, Netscape-Format, z. B. mit der Erweiterung „Get cookies.txt LOCALLY“). Nur für dein Konto, nur zum Schreiben im Chat – Ansehen läuft weiter ohne Anmeldung.".to_string(),
            SettingRow { title: if yt_on { "Hinterlegt" } else { "Nicht hinterlegt" },
                div { class: "flex shrink-0 gap-2",
                    label { class: "inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md border border-input bg-input/30 px-3 text-sm hover:bg-accent",
                        icons::Upload { class: "size-4" }
                        if yt_on { "Ersetzen" } else { "cookies.txt" }
                        input { r#type: "file", class: "hidden", accept: ".txt,text/plain",
                            onchange: move |e| {
                                let Some(file) = e.files().into_iter().next() else { return };
                                spawn(async move {
                                    match file.read_string().await {
                                        Ok(text) => match api::post_text::<serde_json::Value>("/api/accounts/youtube", text).await {
                                            Ok(_) => toast("YouTube-Anmeldung gespeichert"),
                                            Err(e) => toast_err(e),
                                        },
                                        Err(e) => toast_err(e),
                                    }
                                    reload_accounts().await;
                                });
                            }
                        }
                    }
                    if yt_on {
                        Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Entfernen", onclick: move |_| {
                            spawn(async move { let _ = api::post_text::<serde_json::Value>("/api/accounts/youtube", String::new()).await; reload_accounts().await; toast("YouTube-Anmeldung entfernt"); });
                        }, icons::Trash2 {} }
                    }
                }
            }
        }
        Section { title: "Konto",
            SettingRow { title: me.username.clone(), text: if me.admin { Some("Administrator".to_string()) } else { Some("Mitglied".to_string()) },
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, onclick: move |_| {
                    spawn(async move { let _ = api::post::<_, serde_json::Value>("/api/auth/logout", &()).await; load_auth().await; });
                }, icons::LogOut {} "Abmelden" }
            }
            div { class: "grid gap-2 py-3 sm:grid-cols-[1fr_1fr_auto]",
                input { class: field, r#type: "password", placeholder: "Altes Passwort", value: "{old}", oninput: move |e| old.set(e.value()) }
                input { class: field, r#type: "password", placeholder: "Neues Passwort (min. 8)", value: "{new}", oninput: move |e| new.set(e.value()) }
                Button { size: ButtonSize::Sm, class: "h-9", onclick: move |_| {
                    spawn(async move {
                        match api::post::<_, serde_json::Value>("/api/auth/password", &serde_json::json!({ "old": old(), "new": new() })).await {
                            Ok(_) => { toast("Passwort geändert – bitte neu anmelden"); load_auth().await; }
                            Err(e) => toast_err(e),
                        }
                    });
                }, "Ändern" }
            }
        }
        if me.admin { AdminSettings {} }
        p { class: "text-center text-xs text-muted-foreground", "nrzTube {auth.version} – Oberfläche mit nrzUI, Daten über InnerTube und yt-dlp, SponsorBlock und DeArrow von Ajay Ramachandran." }
    }
}

#[component]
fn AdminSettings() -> Element {
    let mut info = use_resource(|| async { api::get::<AdminInfo>("/api/admin").await });
    let mut user = use_signal(String::new);
    let mut pw = use_signal(String::new);
    let mut updating = use_signal(|| false);
    let field = "h-9 w-full rounded-md border border-input bg-input/30 px-3 text-sm outline-none";
    let Some(Ok(a)) = info() else { return rsx! { Loading {} } };
    let open = a.registration_open;
    rsx! {
        Section { title: "Verwaltung",
            SettingRow { title: "Registrierung offen", text: "Jeder mit Zugriff auf diese Adresse kann ein Konto anlegen".to_string(),
                Toggle { checked: open, onchange: move |v: bool| { spawn(async move { let _ = api::post::<_, serde_json::Value>("/api/admin/registration", &serde_json::json!({ "open": v })).await; info.restart(); }); } }
            }
            SettingRow { title: "yt-dlp", text: format!("Version {} – aktualisiert sich täglich selbst", a.ytdlp_version),
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, disabled: updating(), onclick: move |_| {
                    updating.set(true);
                    spawn(async move { let _ = api::post::<_, serde_json::Value>("/api/admin/ytdlp", &()).await; updating.set(false); info.restart(); toast("yt-dlp geprüft"); });
                }, if updating() { icons::LoaderCircle { class: "animate-spin" } } else { icons::RefreshCw {} } "Jetzt aktualisieren" }
            }
            SettingRow { title: "YouTube-Anmeldung",
                text: if a.youtube_cookies { "Hinterlegt – wird nur für altersbeschränkte Videos benutzt".to_string() } else { "Für altersbeschränkte Videos: cookies.txt von youtube.com (Netscape-Format, z. B. Erweiterung „Get cookies.txt LOCALLY“). Am besten ein eigenes Zweitkonto.".to_string() },
                div { class: "flex shrink-0 gap-2",
                    label { class: "inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md border border-input bg-input/30 px-3 text-sm hover:bg-accent",
                        icons::Upload { class: "size-4" }
                        if a.youtube_cookies { "Ersetzen" } else { "cookies.txt" }
                        input { r#type: "file", class: "hidden", accept: ".txt,text/plain",
                            onchange: move |e| {
                                let Some(file) = e.files().into_iter().next() else { return };
                                spawn(async move {
                                    match file.read_string().await {
                                        Ok(text) => match api::post_text::<serde_json::Value>("/api/admin/cookies", text).await {
                                            Ok(_) => toast("YouTube-Anmeldung gespeichert"),
                                            Err(e) => toast_err(e),
                                        },
                                        Err(e) => toast_err(e),
                                    }
                                    info.restart();
                                });
                            }
                        }
                    }
                    if a.youtube_cookies {
                        Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Entfernen", onclick: move |_| {
                            spawn(async move { let _ = api::post_text::<serde_json::Value>("/api/admin/cookies", String::new()).await; info.restart(); toast("YouTube-Anmeldung entfernt"); });
                        }, icons::Trash2 {} }
                    }
                }
            }
            div { class: "py-3",
                div { class: "mb-2 text-sm font-medium", "Konten" }
                for u in a.users.iter().cloned() {
                    div { key: "{u.id}", class: "flex items-center justify-between py-1.5 text-sm",
                        span { "{u.username}" if u.admin { span { class: "ml-2 rounded bg-muted px-1.5 py-0.5 text-xs", "Admin" } } }
                        if !u.admin {
                            Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Konto löschen", onclick: move |_| {
                                spawn(async move { let _ = api::delete::<serde_json::Value>(&format!("/api/admin/users/{}", u.id)).await; info.restart(); });
                            }, icons::Trash2 {} }
                        }
                    }
                }
                div { class: "mt-3 grid gap-2 sm:grid-cols-[1fr_1fr_auto]",
                    input { class: field, placeholder: "Benutzername", value: "{user}", oninput: move |e| user.set(e.value()) }
                    input { class: field, r#type: "password", placeholder: "Passwort (min. 8)", value: "{pw}", oninput: move |e| pw.set(e.value()) }
                    Button { size: ButtonSize::Sm, class: "h-9", onclick: move |_| {
                        spawn(async move {
                            match api::post::<_, serde_json::Value>("/api/admin/users", &Credentials { username: user(), password: pw(), invite: None }).await {
                                Ok(_) => { user.set(String::new()); pw.set(String::new()); info.restart(); toast("Konto angelegt"); }
                                Err(e) => toast_err(e),
                            }
                        });
                    }, icons::Plus {} "Anlegen" }
                }
            }
        }
    }
}

/// Freunde einladen: Link zum Registrieren (einmalig, 7 Tage), auch bei geschlossener Registrierung
#[component]
fn Invites() -> Element {
    let mut list = use_resource(|| async { api::get::<Vec<Invite>>("/api/invites").await.unwrap_or_default() });
    let link = |code: &str| format!("{}/?invite={code}", crate::util::origin());
    let items = list().unwrap_or_default();
    rsx! {
        Section { title: "Freunde einladen", text: "Ein Einladungslink lässt eine Person ein eigenes nrzTube-Konto anlegen – auch wenn die Registrierung geschlossen ist. Jeder Link gilt einmal und 7 Tage. Zum gemeinsamen Schauen ohne Konto reicht der Link einer Watch-Party.".to_string(),
            SettingRow { title: "Neuer Einladungslink",
                Button { size: ButtonSize::Sm, onclick: move |_| {
                    spawn(async move {
                        match api::post::<_, Invite>("/api/invites", &()).await {
                            Ok(i) => { crate::util::copy(&format!("{}/?invite={}", crate::util::origin(), i.code)); toast("Einladungslink erstellt und kopiert"); list.restart(); }
                            Err(e) => toast_err(e),
                        }
                    });
                }, icons::UserPlus {} "Erstellen" }
            }
            for i in items.iter() {
                {
                    let l = link(&i.code);
                    let code = i.code.clone();
                    let used = i.used_by.clone();
                    let expired = i.expires < crate::util::now();
                    rsx! {
                        div { key: "{i.code}", class: "flex items-center gap-2 py-2 text-sm",
                            icons::Link { class: "size-4 shrink-0 text-muted-foreground" }
                            span { class: "min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground", "{l}" }
                            if let Some(u) = used {
                                span { class: "shrink-0 rounded bg-secondary px-1.5 text-[11px]", "benutzt von {u}" }
                            } else if expired {
                                span { class: "shrink-0 rounded bg-secondary px-1.5 text-[11px] text-muted-foreground", "abgelaufen" }
                            } else {
                                Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Kopieren", onclick: move |_| { crate::util::copy(&l); toast("Kopiert"); }, icons::Copy {} }
                                Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "Zurückziehen", onclick: move |_| {
                                    let c = code.clone();
                                    spawn(async move { let _ = api::delete::<serde_json::Value>(&format!("/api/invites/{c}")).await; list.restart(); });
                                }, icons::Trash2 {} }
                            }
                        }
                    }
                }
            }
        }
    }
}
