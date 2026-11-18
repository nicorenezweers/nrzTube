//! Anmeldung und Registrierung. Ohne Anmeldung zeigt nrzTube nur diese Seite.

use crate::api;
use crate::state::*;
use crate::ui::button::Button;
use dioxus::prelude::*;
use nrztube_types::Credentials;

#[component]
pub fn AccountGate(children: Element) -> Element {
    let Some(auth) = AUTH() else {
        return rsx! {
            div { class: "flex h-dvh items-center justify-center text-muted-foreground",
                icons::LoaderCircle { class: "size-6 animate-spin" }
            }
        };
    };
    if auth.me.is_some() {
        return children;
    }
    // Einladung zur Watch-Party: als Gast beitreten (ohne Konto) – die Party-Seite regelt das
    let path = crate::util::window().location().pathname().unwrap_or_default();
    if path.starts_with("/party/") {
        return children;
    }
    rsx! { LoginForm { setup: auth.setup, registration_open: auth.registration_open } }
}

#[component]
fn LoginForm(setup: bool, registration_open: bool) -> Element {
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    // Einladungslink: /?invite=CODE (Registrierung auch bei geschlossener Anmeldung)
    let invite = use_hook(|| query_param("invite"));
    let invite_ok = use_resource({
        let invite = invite.clone();
        move || {
            let invite = invite.clone();
            async move {
                match invite {
                    Some(c) => api::get::<serde_json::Value>(&format!("/api/invites/{}", api::enc(&c))).await.ok().and_then(|v| v.get("valid").and_then(|x| x.as_bool())).unwrap_or(false),
                    None => false,
                }
            }
        }
    });
    let invited = invite_ok().unwrap_or(false);
    let mut register = use_signal(|| setup || invite.is_some());
    let can_register = setup || registration_open || invited;
    let inv = invite.clone();
    let submit = move |e: FormEvent| {
        e.prevent_default();
        let c = Credentials { username: username(), password: password(), invite: if register() { inv.clone() } else { None } };
        let path = if register() { "/api/auth/register" } else { "/api/auth/login" };
        busy.set(true);
        error.set(None);
        spawn(async move {
            match api::post::<_, serde_json::Value>(path, &c).await {
                // Unabhängig vom Formular laden: das verschwindet nach der Anmeldung (und mit ihm
                // seine Aufgaben) – sonst fehlen Abos, Einstellungen usw. bis zum Neuladen
                Ok(_) => { dioxus::core::spawn_forever(async { load_auth().await; }); }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };
    let field = "h-10 w-full rounded-md border border-input bg-input/30 px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50";
    rsx! {
        div { class: "flex min-h-dvh items-center justify-center p-4",
            form { class: "w-full max-w-sm rounded-2xl border bg-card p-6 shadow-xl", onsubmit: submit,
                div { class: "mb-6 flex items-center gap-3",
                    span { class: "flex size-10 items-center justify-center rounded-xl bg-primary text-primary-foreground",
                        icons::Play { class: "size-5 fill-current" }
                    }
                    div {
                        h1 { class: "text-lg font-semibold", "nrzTube" }
                        p { class: "text-xs text-muted-foreground", "YouTube ohne Werbung und ohne Tracking" }
                    }
                }
                if setup {
                    p { class: "mb-4 rounded-lg bg-muted p-3 text-xs text-muted-foreground", "Willkommen! Lege das erste Konto an – es wird Administrator." }
                } else if invited && register() {
                    p { class: "mb-4 flex items-center gap-2 rounded-lg bg-muted p-3 text-xs text-muted-foreground", icons::Gift { class: "size-4 shrink-0" } "Du wurdest eingeladen – lege dir ein Konto an." }
                } else if invite.is_some() && invite_ok().is_some() && !invited {
                    p { class: "mb-4 rounded-lg bg-muted p-3 text-xs text-muted-foreground", "Diese Einladung ist abgelaufen oder wurde schon benutzt." }
                }
                label { class: "mb-1 block text-xs font-medium", "Benutzername" }
                input { class: "{field} mb-3", autocomplete: "username", value: "{username}", oninput: move |e| username.set(e.value()), autofocus: true }
                label { class: "mb-1 block text-xs font-medium", "Passwort" }
                input { class: "{field} mb-4", r#type: "password", autocomplete: if register() { "new-password" } else { "current-password" }, value: "{password}", oninput: move |e| password.set(e.value()) }
                if let Some(e) = error() { p { class: "mb-3 text-sm text-destructive", "{e}" } }
                Button { class: "w-full", button_type: "submit", disabled: busy(),
                    if busy() { icons::LoaderCircle { class: "animate-spin" } }
                    if register() { "Konto anlegen" } else { "Anmelden" }
                }
                if can_register && !setup {
                    button { r#type: "button", class: "mt-4 w-full text-center text-xs text-muted-foreground hover:text-foreground",
                        onclick: move |_| register.set(!register()),
                        if register() { "Schon ein Konto? Anmelden" } else { "Neues Konto anlegen" }
                    }
                }
            }
        }
    }
}

/// Wert aus der Adresszeile (?name=wert)
fn query_param(name: &str) -> Option<String> {
    let q = crate::util::window().location().search().ok()?;
    q.trim_start_matches('?').split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == name && !v.is_empty()).then(|| urlencoding::decode(v).map(|x| x.into_owned()).unwrap_or_else(|_| v.to_string()))
    })
}
