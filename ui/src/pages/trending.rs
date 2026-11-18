//! Trends (Themen-Kanäle von YouTube) und Beliebt (meistgesehene neue Videos der Abos aller
//! Konten dieser Instanz).

use crate::Route;
use crate::api;
use crate::components::*;
use crate::state::*;
use dioxus::prelude::*;
use nrztube_types::*;

#[component]
pub fn Trending(cat: String) -> Element {
    let c = if cat.is_empty() { "now".to_string() } else { cat.clone() };
    let c2 = c.clone();
    let data = use_resource(use_reactive!(|c2| async move { api::get::<Vec<VideoItem>>(&format!("/api/trending?cat={c2}")).await }));
    if SETTINGS().hide_trending {
        return rsx! { EmptyState { title: "Trends ausgeblendet", text: "In den Einstellungen unter „Ablenkungsfrei“.".to_string() } };
    }
    rsx! {
        div { class: "mx-auto max-w-[1800px] px-3 py-4 md:p-6",
            PageTitle { title: "Trends" }
            div { class: "shelf -mx-1 mb-5 flex gap-2 overflow-x-auto px-1",
                for (key, label) in TRENDING_CATEGORIES.iter() {
                    Link { key: "{key}", to: Route::Trending { cat: key.to_string() },
                        class: if *key == c { "shrink-0 rounded-full bg-primary px-3.5 py-1.5 text-sm font-medium text-primary-foreground" } else { "shrink-0 rounded-full bg-secondary px-3.5 py-1.5 text-sm hover:bg-accent" },
                        "{label}"
                    }
                }
            }
            match data() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message } },
                Some(Ok(list)) if list.is_empty() => rsx! { EmptyState { title: "Keine Videos", text: "Für diese Kategorie und Region gibt es gerade nichts.".to_string() } },
                Some(Ok(list)) => rsx! { VideoGrid { videos: list } },
            }
        }
    }
}
