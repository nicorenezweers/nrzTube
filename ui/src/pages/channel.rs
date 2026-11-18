//! Kanal: Kopf mit Banner und Abo-Knopf, Reiter Videos, Shorts, Live, Playlists, Beiträge, Info.

use crate::Route;
use crate::api;
use crate::components::*;
use crate::state::*;
use crate::pages::twitch::YtTwitchLinks;
use crate::ui::button::{Button, ButtonVariant};
use dioxus::prelude::*;
use nrztube_types::*;

#[derive(Clone, PartialEq, serde::Deserialize, Default)]
struct TabPage {
    items: Vec<Item>,
    posts: Vec<Post>,
    continuation: Option<String>,
}

fn tab_label(t: &str) -> &'static str {
    match t {
        "videos" => "Videos",
        "shorts" => "Shorts",
        "streams" => "Live",
        "playlists" => "Playlists",
        "posts" => "Beiträge",
        "about" => "Info",
        _ => "Videos",
    }
}

#[component]
pub fn ChannelPage(id: String, tab: String) -> Element {
    let cid = id.clone();
    let channel = use_resource(use_reactive!(|cid| async move { api::get::<Channel>(&format!("/api/channel/{}", api::enc(&cid))).await }));
    let mut q = use_signal(String::new);
    let mut search_results = use_signal(|| None::<Vec<Item>>);
    let s = SETTINGS();
    let ch = match channel() {
        None => return rsx! { Loading {} },
        Some(Err(e)) => return rsx! { ErrorBox { message: e.message } },
        Some(Ok(c)) => c,
    };
    let mut tabs: Vec<String> = ch.tabs.clone();
    tabs.retain(|t| !(s.hide_shorts && t == "shorts" || s.hide_live && t == "streams" || s.hide_posts && t == "posts" || s.hide_channel_playlists && t == "playlists"));
    tabs.push("about".into());
    let current = if tab.is_empty() { tabs.first().cloned().unwrap_or_else(|| "videos".into()) } else { tab.clone() };
    let subscribed = is_subscribed(&ch.id);
    let (sid, sname, sthumb) = (ch.id.clone(), ch.name.clone(), ch.avatar.clone());
    let real_id = ch.id.clone();
    rsx! {
        div { class: "mx-auto max-w-[1800px]",
            if let Some(b) = &ch.banner {
                div { class: "aspect-[6/1] w-full overflow-hidden bg-muted md:mx-6 md:mt-4 md:w-auto md:rounded-2xl",
                    img { class: "size-full object-cover", src: "{b}", alt: "" }
                }
            }
            div { class: "flex flex-wrap items-center gap-4 px-4 py-5 md:px-6",
                img { class: "size-20 rounded-full bg-muted object-cover md:size-28", src: "{ch.avatar}", alt: "" }
                div { class: "min-w-0 flex-1",
                    h1 { class: "flex items-center gap-2 text-2xl font-bold", "{ch.name}" if ch.verified { icons::Check { class: "size-5" } } }
                    div { class: "mt-1 text-sm text-muted-foreground",
                        {[ch.handle.clone(), if s.hide_subscriber_counts { None } else { ch.subscribers.clone() }, ch.video_count.clone()].into_iter().flatten().collect::<Vec<_>>().join(" • ")}
                    }
                    if !ch.description.is_empty() {
                        p { class: "mt-1 line-clamp-1 max-w-2xl text-sm text-muted-foreground", "{ch.description}" }
                    }
                }
                Button { variant: if subscribed { ButtonVariant::Secondary } else { ButtonVariant::Default },
                    onclick: move |_| { let (a, b, c) = (sid.clone(), sname.clone(), sthumb.clone()); spawn(async move { toggle_subscription(a, b, c).await; }); },
                    if subscribed { icons::BellOff {} "Abonniert" } else { icons::Bell {} "Abonnieren" }
                }
            }
            YtTwitchLinks { key: "{ch.id}", yt_id: ch.id.clone(), name: ch.name.clone() }
            div { class: "sticky top-0 z-10 flex items-center gap-1 overflow-x-auto border-b bg-background/95 px-4 md:px-6 shelf",
                for t in tabs.iter() {
                    Link { key: "{t}", to: Route::ChannelPage { id: id.clone(), tab: t.clone() },
                        class: if *t == current { "shrink-0 border-b-2 border-foreground px-3 py-3 text-sm font-medium" } else { "shrink-0 border-b-2 border-transparent px-3 py-3 text-sm text-muted-foreground hover:text-foreground" },
                        {tab_label(t)}
                    }
                }
                form { class: "ml-auto flex shrink-0 items-center py-2",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let query = q();
                        let cid = real_id.clone();
                        if query.trim().is_empty() { search_results.set(None); return; }
                        spawn(async move {
                            if let Ok(p) = api::get::<Page<Item>>(&format!("/api/channel/{cid}/search?q={}", api::enc(&query))).await {
                                search_results.set(Some(p.items));
                            }
                        });
                    },
                    input { class: "h-8 w-36 rounded-full border border-input bg-input/30 px-3 text-xs outline-none focus:w-52 transition-all",
                        placeholder: "Im Kanal suchen", value: "{q}", oninput: move |e| { q.set(e.value()); if e.value().is_empty() { search_results.set(None); } } }
                }
            }
            div { class: "p-4 md:p-6",
                if let Some(res) = search_results() {
                    ItemGrid { items: res }
                } else if current == "about" {
                    About { ch: ch.clone() }
                } else {
                    TabContent { key: "{ch.id}-{current}", id: ch.id.clone(), tab: current.clone() }
                }
            }
        }
    }
}

#[component]
fn About(ch: Channel) -> Element {
    rsx! {
        div { class: "max-w-3xl rounded-xl bg-card p-5 text-sm",
            h2 { class: "mb-2 font-semibold", "Beschreibung" }
            RichText { text: ch.description.clone() }
            div { class: "mt-4 text-xs text-muted-foreground", "Kanal-ID: {ch.id}" }
        }
    }
}

#[component]
fn TabContent(id: String, tab: String) -> Element {
    let mut items = use_signal(Vec::<Item>::new);
    let mut posts = use_signal(Vec::<Post>::new);
    let mut cont = use_signal(|| None::<String>);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);
    let (id2, tab2) = (id.clone(), tab.clone());
    use_hook(move || {
        spawn(async move {
            match api::get::<TabPage>(&format!("/api/channel/{id2}/tab/{tab2}")).await {
                Ok(p) => { items.set(p.items); posts.set(p.posts); cont.set(p.continuation); }
                Err(e) => error.set(Some(e.message)),
            }
            loading.set(false);
        });
    });
    let more = move |_| {
        let Some(c) = cont() else { return };
        let (id, tab) = (id.clone(), tab.clone());
        loading.set(true);
        spawn(async move {
            if let Ok(p) = api::get::<TabPage>(&format!("/api/channel/{id}/tab/{tab}?cont={}", api::enc(&c))).await {
                items.write().extend(p.items);
                posts.write().extend(p.posts);
                cont.set(p.continuation);
            }
            loading.set(false);
        });
    };
    rsx! {
        if let Some(e) = error() {
            ErrorBox { message: e }
        } else if items().is_empty() && posts().is_empty() && loading() {
            Loading {}
        } else if items().is_empty() && posts().is_empty() {
            EmptyState { title: "Nichts gefunden", text: "In diesem Reiter gibt es keine Inhalte.".to_string() }
        } else {
            if !posts().is_empty() {
                div { class: "mx-auto flex max-w-2xl flex-col gap-4",
                    for p in posts() { PostView { key: "{p.id}", p: p.clone() } }
                }
            }
            ItemGrid { items: items() }
            if cont().is_some() { LoadMore { loading: loading(), onclick: more } }
        }
    }
}

#[component]
fn PostView(p: Post) -> Element {
    let s = SETTINGS();
    rsx! {
        article { class: "rounded-xl border bg-card p-4",
            div { class: "mb-2 flex items-center gap-3",
                if !p.author_thumb.is_empty() && !s.hide_channel_avatars { img { class: "size-9 rounded-full object-cover", src: "{p.author_thumb}", alt: "" } }
                div {
                    div { class: "text-sm font-medium", "{p.author}" }
                    div { class: "text-xs text-muted-foreground", "{p.published}" }
                }
            }
            RichText { text: p.text.clone(), class: "text-sm" }
            if !p.images.is_empty() {
                div { class: if p.images.len() > 1 { "mt-3 grid grid-cols-2 gap-2" } else { "mt-3" },
                    for img_url in p.images.iter() {
                        img { class: "w-full rounded-lg bg-muted object-cover", src: "{img_url}", alt: "", loading: "lazy" }
                    }
                }
            }
            if let Some(v) = p.video.clone() {
                div { class: "mt-3 max-w-md", VideoCard { v } }
            }
            if !p.poll.is_empty() {
                div { class: "mt-3 flex flex-col gap-2",
                    for o in p.poll.iter() { div { class: "rounded-lg border px-3 py-2 text-sm", "{o}" } }
                    if let Some(v) = &p.poll_votes { div { class: "text-xs text-muted-foreground", "{v}" } }
                }
            }
            div { class: "mt-3 flex gap-4 text-xs text-muted-foreground",
                if !s.hide_likes && !p.likes.is_empty() { span { class: "flex items-center gap-1", icons::ThumbsUp { class: "size-3.5" } "{p.likes}" } }
                if !p.comments.is_empty() { span { class: "flex items-center gap-1", icons::MessageSquare { class: "size-3.5" } "{p.comments}" } }
            }
        }
    }
}

