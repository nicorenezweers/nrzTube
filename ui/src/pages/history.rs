//! Verlauf: durchsuchbar, mit Fortschritt, einzeln oder ganz löschen.

use crate::api;
use crate::components::*;
use crate::state::*;
use crate::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::util;
use dioxus::prelude::*;
use nrztube_types::*;

#[component]
pub fn HistoryPage() -> Element {
    let mut q = use_signal(String::new);
    let mut entries = use_signal(Vec::<HistoryEntry>::new);
    let mut more = use_signal(|| false);
    let mut loading = use_signal(|| true);
    let mut confirm = use_signal(|| false);
    let mut load = move |reset: bool| {
        let offset = if reset { 0 } else { entries.peek().len() };
        let query = q.peek().clone();
        loading.set(true);
        spawn(async move {
            if let Ok(list) = api::get::<Vec<HistoryEntry>>(&format!("/api/history?q={}&offset={offset}", api::enc(&query))).await {
                more.set(list.len() == 60);
                if reset { entries.set(list); } else { entries.write().extend(list); }
            }
            loading.set(false);
        });
    };
    use_hook(move || load(true));
    let ids: Vec<String> = entries().iter().map(|e| e.video.id.clone()).collect();
    use_effect(use_reactive!(|ids| load_dearrow(ids)));
    let save = SETTINGS().save_history;
    let mut last_day = String::new();
    rsx! {
        div { class: "mx-auto max-w-5xl p-4 md:p-6",
            PageTitle { title: "Verlauf",
                input { class: "h-8 w-56 rounded-md border border-input bg-input/30 px-3 text-sm outline-none", placeholder: "Verlauf durchsuchen", value: "{q}",
                    oninput: move |e| { q.set(e.value()); load(true); } }
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, onclick: move |_| confirm.set(true), icons::Trash2 {} "Alles löschen" }
            }
            if !save {
                p { class: "mb-4 rounded-lg bg-muted p-3 text-xs text-muted-foreground", "Der Verlauf ist in den Einstellungen ausgeschaltet – neue Videos werden nicht gespeichert." }
            }
            if confirm() {
                Modal { title: "Verlauf löschen?", onclose: move |_| confirm.set(false),
                    p { class: "text-sm text-muted-foreground", "Alle Einträge und gespeicherten Positionen werden entfernt." }
                    div { class: "mt-4 flex justify-end gap-2",
                        Button { variant: ButtonVariant::Ghost, onclick: move |_| confirm.set(false), "Abbrechen" }
                        Button { variant: ButtonVariant::Destructive, onclick: move |_| {
                            spawn(async move { let _ = api::delete::<serde_json::Value>("/api/history").await; entries.set(vec![]); confirm.set(false); toast("Verlauf gelöscht"); });
                        }, "Löschen" }
                    }
                }
            }
            if entries().is_empty() && !loading() {
                EmptyState { title: "Noch nichts angesehen" }
            }
            div { class: "flex flex-col gap-3",
                for e in entries() {
                    {
                        let day = day_label(e.watched);
                        let header = if day != last_day { last_day = day.clone(); Some(day) } else { None };
                        let vid = e.video.id.clone();
                        rsx! {
                            if let Some(h) = header { h2 { class: "mt-4 text-sm font-semibold text-muted-foreground", "{h}" } }
                            div { class: "flex items-start gap-2",
                                div { class: "min-w-0 flex-1", VideoCard { v: e.video.clone(), progress: Some(e.progress), compact: true } }
                                Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconXs, title: "Aus dem Verlauf entfernen",
                                    onclick: move |_| { let v = vid.clone(); spawn(async move { let _ = api::delete::<serde_json::Value>(&format!("/api/history/{v}")).await; entries.write().retain(|x| x.video.id != v); }); },
                                    icons::X { class: "size-3.5" }
                                }
                            }
                        }
                    }
                }
            }
            if loading() { Loading {} } else if more() { LoadMore { loading: false, onclick: move |_| load(false) } }
        }
    }
}

fn day_label(ts: i64) -> String {
    let d = (util::now() - ts) / 86400;
    match d {
        0 => "Heute".into(),
        1 => "Gestern".into(),
        2..=6 => format!("Vor {d} Tagen"),
        _ => {
            let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ts as f64 * 1000.0));
            format!("{:02}.{:02}.{}", date.get_date(), date.get_month() + 1, date.get_full_year())
        }
    }
}
