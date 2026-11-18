//! Renderer aus InnerTube-Antworten -> Typen aus `nrztube-types`.
//!
//! Statt fester Pfade läuft ein Sammler rekursiv durch die Antwort und nimmt jedes bekannte
//! Element (Video, Kanal, Playlist, Short, Beitrag) mit – so überstehen wir die häufigen
//! Umbauten der YouTube-Oberfläche (die gleiche Idee wie die Extraktoren von Invidious).

use super::json::*;
use crate::proxy::{img, video_thumb};
use nrztube_types::*;
use serde_json::Value;
use std::collections::HashSet;

pub fn channel_from_endpoint(v: &Value) -> Option<String> {
    s(v, "navigationEndpoint.browseEndpoint.browseId")
        .or_else(|| s(v, "browseEndpoint.browseId"))
        .or_else(|| s(v, "runs.0.navigationEndpoint.browseEndpoint.browseId"))
        .or_else(|| s(v, "commandRuns.0.onTap.innertubeCommand.browseEndpoint.browseId"))
}

fn verified_from_badges(v: Option<&Value>) -> bool {
    let Some(Value::Array(badges)) = v else { return false };
    badges.iter().any(|b| {
        let style = s(b, "metadataBadgeRenderer.style").unwrap_or_default();
        style.contains("VERIFIED")
    })
}

fn live_from_badges(v: &Value) -> bool {
    let mut found = false;
    for key in ["badges", "thumbnailOverlays"] {
        if let Some(Value::Array(list)) = v.get(key) {
            for b in list {
                let st = s(b, "metadataBadgeRenderer.style").unwrap_or_default();
                let ov = s(b, "thumbnailOverlayTimeStatusRenderer.style").unwrap_or_default();
                if st.contains("LIVE") || ov == "LIVE" {
                    found = true;
                }
            }
        }
    }
    found
}

pub fn video_renderer(v: &Value) -> Option<VideoItem> {
    let id = s(v, "videoId")?;
    let title = opt_t(v, "title").or_else(|| opt_t(v, "headline")).unwrap_or_default();
    let owner = v
        .get("ownerText")
        .or_else(|| v.get("longBylineText"))
        .or_else(|| v.get("shortBylineText"));
    let author = text(owner);
    let author_id = owner.and_then(channel_from_endpoint).unwrap_or_default();
    let author_thumb = best_thumb(at(v, "channelThumbnailSupportedRenderers.channelThumbnailWithLinkRenderer.thumbnail"))
        .or_else(|| best_thumb(at(v, "channelThumbnail")))
        .or_else(|| {
            best_thumb(at(v, "avatar.decoratedAvatarViewModel.avatar.avatarViewModel.image"))
        })
        .map(|u| img(&u));
    let length = opt_t(v, "lengthText").or_else(|| {
        let mut out = Vec::new();
        find_all(v, "thumbnailOverlayTimeStatusRenderer", &mut out);
        out.first().and_then(|o| opt_t(o, "text"))
    });
    let duration = length.as_deref().and_then(parse_duration);
    let views_text = opt_t(v, "viewCountText").or_else(|| opt_t(v, "shortViewCountText"));
    let views = views_text.as_deref().and_then(parse_count);
    let upcoming = v.get("upcomingEventData").is_some();
    let live = !upcoming && (live_from_badges(v) || views_text.as_deref().is_some_and(|t| t.contains("schauen") || t.contains("watching")));
    let description = at(v, "detailedMetadataSnippets.0.snippetText")
        .map(|x| text(Some(x)))
        .or_else(|| opt_t(v, "descriptionSnippet"))
        .filter(|x| !x.is_empty());
    let published_ts = s(v, "upcomingEventData.startTime").and_then(|x| x.parse().ok());
    Some(VideoItem {
        thumb: video_thumb(&id),
        id,
        title,
        original_title: None,
        author,
        author_id,
        author_thumb,
        duration,
        views,
        views_text,
        published: opt_t(v, "publishedTimeText"),
        published_ts,
        live,
        upcoming,
        short: false,
        verified: verified_from_badges(v.get("ownerBadges")),
        description,
    })
}

fn playlist_video_renderer(v: &Value) -> Option<VideoItem> {
    let mut item = video_renderer(v)?;
    if item.views_text.is_none() {
        // "Kanal • 1,2 Mio. Aufrufe • vor 3 Jahren"
        let info = t(v, "videoInfo");
        let parts: Vec<&str> = info.split('•').map(str::trim).filter(|x| !x.is_empty()).collect();
        if let Some(p) = parts.first() {
            item.views_text = Some((*p).to_string());
        }
        if let Some(p) = parts.get(1) {
            item.published = Some((*p).to_string());
        }
    }
    if item.duration.is_none() {
        item.duration = s(v, "lengthSeconds").and_then(|x| x.parse().ok());
    }
    Some(item)
}

fn shorts_lockup(v: &Value) -> Option<VideoItem> {
    let id = s(v, "onTap.innertubeCommand.reelWatchEndpoint.videoId")
        .or_else(|| s(v, "entityId").map(|e| e.trim_start_matches("shorts-shelf-item-").to_string()))?;
    let views_text = opt_t(v, "overlayMetadata.secondaryText");
    Some(VideoItem {
        thumb: video_thumb(&id),
        title: t(v, "overlayMetadata.primaryText"),
        id,
        views: views_text.as_deref().and_then(parse_count),
        views_text,
        short: true,
        ..Default::default()
    })
}

fn reel_item(v: &Value) -> Option<VideoItem> {
    let id = s(v, "videoId")?;
    let views_text = opt_t(v, "viewCountText");
    Some(VideoItem {
        thumb: video_thumb(&id),
        title: t(v, "headline"),
        id,
        views: views_text.as_deref().and_then(parse_count),
        views_text,
        short: true,
        ..Default::default()
    })
}

fn is_views(l: &str) -> bool {
    l.chars().any(|c| c.is_ascii_digit())
        && ["aufruf", "view", "zuschau", "watching", "wartend", "waiting"].iter().any(|k| l.contains(k))
}

fn is_date(l: &str) -> bool {
    ["vor ", " ago", "gestreamt", "streamed", "premiere", "geplant", "scheduled", "aktualisiert", "updated"]
        .iter()
        .any(|k| l.contains(k))
}

/// Gaming-Seite: `videoCardRenderer`
fn video_card(v: &Value) -> Option<VideoItem> {
    let mut it = video_renderer(v)?;
    if it.author.is_empty() {
        it.author = t(v, "bylineText");
        it.author_id = v.get("bylineText").and_then(channel_from_endpoint).unwrap_or_default();
    }
    let meta = t(v, "metadataText");
    for part in meta.split(['·', '•']).map(str::trim) {
        let l = part.to_lowercase();
        if is_views(&l) && it.views_text.is_none() {
            it.views_text = Some(part.to_string());
        } else if is_date(&l) && it.published.is_none() {
            it.published = Some(part.to_string());
        }
    }
    Some(it)
}

/// Neues Format (2024+): `lockupViewModel` für Videos, Playlists, Mixe.
fn lockup(v: &Value) -> Option<Item> {
    let id = s(v, "contentId")?;
    let kind = s(v, "contentType").unwrap_or_default();
    let meta = at(v, "metadata.lockupMetadataViewModel")?;
    let title = t(meta, "title");
    let mut rows: Vec<Vec<(String, Option<String>)>> = Vec::new();
    if let Some(Value::Array(list)) = at(meta, "metadata.contentMetadataViewModel.metadataRows") {
        for row in list {
            let mut parts = Vec::new();
            if let Some(Value::Array(ps)) = row.get("metadataParts") {
                for p in ps {
                    let txt = t(p, "text");
                    let ch = at(p, "text.commandRuns.0.onTap.innertubeCommand.browseEndpoint.browseId")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    if !txt.is_empty() {
                        parts.push((txt, ch));
                    }
                }
            }
            rows.push(parts);
        }
    }
    let avatar = best_thumb(at(meta, "image.decoratedAvatarViewModel.avatar.avatarViewModel.image"))
        .map(|u| img(&u));
    let avatar_channel = s(meta, "image.decoratedAvatarViewModel.rendererContext.commandContext.onTap.innertubeCommand.browseEndpoint.browseId");
    // Teile einordnen: Kanal, Aufrufe, Datum (die Reihenfolge ist je nach Seite verschieden)
    let mut author = String::new();
    let mut author_id = String::new();
    let mut views_text: Option<String> = None;
    let mut published: Option<String> = None;
    for (ri, row) in rows.iter().enumerate() {
        for (pi, (txt, ch)) in row.iter().enumerate() {
            let l = txt.to_lowercase();
            if let Some(c) = ch {
                author = txt.clone();
                author_id = c.clone();
            } else if is_views(&l) {
                views_text = Some(txt.clone());
            } else if is_date(&l) {
                published = Some(txt.clone());
            } else if ri == 0 && pi == 0 && rows.len() > 1 && author.is_empty() {
                author = txt.clone();
            }
        }
    }
    if author_id.is_empty() {
        author_id = avatar_channel.unwrap_or_default();
    }
    let mut badges = Vec::new();
    find_all(v, "thumbnailBadgeViewModel", &mut badges);
    let badge_texts: Vec<String> = badges.iter().filter_map(|b| s(b, "text")).collect();

    if kind.contains("PLAYLIST") || kind.contains("PODCAST") || kind.contains("ALBUM") {
        let count = badge_texts.iter().find(|b| b.chars().any(|c| c.is_ascii_digit())).cloned();
        let thumb = best_thumb(at(v, "contentImage.collectionThumbnailViewModel.primaryThumbnail.thumbnailViewModel.image"))
            .map(|u| img(&u))
            .unwrap_or_default();
        let first_video = s(v, "rendererContext.commandContext.onTap.innertubeCommand.watchEndpoint.videoId");
        return Some(Item::Playlist(PlaylistItem {
            id,
            title,
            author,
            author_id,
            thumb,
            video_count: count,
            first_video,
        }));
    }
    if !kind.contains("VIDEO") && !kind.is_empty() {
        return None;
    }
    let duration = badge_texts.iter().find_map(|b| parse_duration(b));
    let live = badge_texts.iter().any(|b| b.eq_ignore_ascii_case("live") || b.eq_ignore_ascii_case("live jetzt"));
    let upcoming = views_text.as_deref().is_some_and(|x| x.contains("Geplant") || x.contains("Scheduled") || x.contains("Premiere"));
    Some(Item::Video(VideoItem {
        thumb: video_thumb(&id),
        id,
        title,
        original_title: None,
        author,
        author_id,
        author_thumb: avatar,
        duration,
        views: views_text.as_deref().and_then(parse_count),
        views_text,
        published,
        published_ts: None,
        live,
        upcoming,
        short: false,
        verified: false,
        description: None,
    }))
}

pub fn channel_renderer(v: &Value) -> Option<ChannelItem> {
    let id = s(v, "channelId")?;
    let sub = opt_t(v, "subscriberCountText");
    let vc = opt_t(v, "videoCountText");
    // YouTube zeigt in der Suche den @Namen im Feld subscriberCountText und die Abos in videoCountText
    let (handle, subscribers, video_count) = match (&sub, &vc) {
        (Some(a), b) if a.starts_with('@') => (Some(a.clone()), b.clone(), None),
        _ => (None, sub.clone(), vc.clone()),
    };
    Some(ChannelItem {
        id,
        name: opt_t(v, "title").unwrap_or_default(),
        thumb: best_thumb(v.get("thumbnail")).map(|u| img(&u)).unwrap_or_default(),
        handle,
        subscribers,
        video_count,
        description: opt_t(v, "descriptionSnippet"),
        verified: verified_from_badges(v.get("ownerBadges")),
    })
}

fn playlist_renderer(v: &Value) -> Option<PlaylistItem> {
    let id = s(v, "playlistId")?;
    let owner = v.get("shortBylineText").or_else(|| v.get("longBylineText"));
    let thumb = best_thumb(at(v, "thumbnails.0"))
        .or_else(|| best_thumb(v.get("thumbnail")))
        .or_else(|| best_thumb(at(v, "thumbnailRenderer.playlistVideoThumbnailRenderer.thumbnail")))
        .map(|u| img(&u))
        .unwrap_or_default();
    Some(PlaylistItem {
        id,
        title: t(v, "title"),
        author: text(owner),
        author_id: owner.and_then(channel_from_endpoint).unwrap_or_default(),
        thumb,
        video_count: opt_t(v, "videoCountText").or_else(|| s(v, "videoCount")),
        first_video: s(v, "navigationEndpoint.watchEndpoint.videoId"),
    })
}

/// Sammelt alle bekannten Elemente und das letzte Fortsetzungs-Token.
#[derive(Default)]
pub struct Collector {
    pub items: Vec<Item>,
    pub posts: Vec<Post>,
    pub continuation: Option<String>,
    seen: HashSet<String>,
    pub skip_shelves: bool,
}

impl Collector {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, item: Item) {
        let key = match &item {
            Item::Video(v) => format!("v{}", v.id),
            Item::Channel(c) => format!("c{}", c.id),
            Item::Playlist(p) => format!("p{}", p.id),
        };
        if self.seen.insert(key) {
            self.items.push(item);
        }
    }

    pub fn walk(&mut self, v: &Value) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    match k.as_str() {
                        "videoRenderer" | "gridVideoRenderer" | "compactVideoRenderer" | "videoWithContextRenderer" => {
                            if let Some(it) = video_renderer(x) {
                                self.push(Item::Video(it));
                            }
                        }
                        "videoCardRenderer" => {
                            if let Some(it) = video_card(x) {
                                self.push(Item::Video(it));
                            }
                        }
                        "playlistVideoRenderer" | "playlistPanelVideoRenderer" => {
                            if let Some(it) = playlist_video_renderer(x) {
                                self.push(Item::Video(it));
                            }
                        }
                        "shortsLockupViewModel" => {
                            if let Some(it) = shorts_lockup(x) {
                                self.push(Item::Video(it));
                            }
                        }
                        "reelItemRenderer" => {
                            if let Some(it) = reel_item(x) {
                                self.push(Item::Video(it));
                            }
                        }
                        "lockupViewModel" => {
                            if let Some(it) = lockup(x) {
                                self.push(it);
                            }
                        }
                        "channelRenderer" | "gridChannelRenderer" => {
                            if let Some(it) = channel_renderer(x) {
                                self.push(Item::Channel(it));
                            }
                        }
                        "playlistRenderer" | "gridPlaylistRenderer" | "radioRenderer" | "compactPlaylistRenderer" => {
                            if let Some(it) = playlist_renderer(x) {
                                self.push(Item::Playlist(it));
                            }
                        }
                        "backstagePostRenderer" | "postRenderer" => {
                            if let Some(p) = post_renderer(x) {
                                if self.seen.insert(format!("post{}", p.id)) {
                                    self.posts.push(p);
                                }
                            }
                        }
                        "continuationItemRenderer" => {
                            if let Some(tok) = s(x, "continuationEndpoint.continuationCommand.token")
                                .or_else(|| s(x, "button.buttonRenderer.command.continuationCommand.token"))
                            {
                                self.continuation = Some(tok);
                            }
                        }
                        // Keine Werbung, keine Hinweise, keine Menüs, kein Kopf der Seite
                        "adSlotRenderer" | "promotedSparklesWebRenderer" | "menu" | "topbar" | "header"
                        | "frameworkUpdates" | "responseContext" | "trackingParams" | "searchPyvRenderer"
                        | "engagementPanels" | "hotkeyDialog" | "playerOverlays" | "overlay" => {}
                        "reelShelfRenderer" | "richShelfRenderer" | "shelfRenderer" | "gridShelfViewModel"
                            if self.skip_shelves => {}
                        _ => self.walk(x),
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|x| self.walk(x)),
            _ => {}
        }
    }

    pub fn videos(self) -> Vec<VideoItem> {
        self.items
            .into_iter()
            .filter_map(|i| match i {
                Item::Video(v) => Some(v),
                _ => None,
            })
            .collect()
    }
}

pub fn post_renderer(v: &Value) -> Option<Post> {
    let id = s(v, "postId")?;
    let author_id = v.get("authorText").and_then(channel_from_endpoint)
        .or_else(|| s(v, "authorEndpoint.browseEndpoint.browseId"))
        .unwrap_or_default();
    let mut images = Vec::new();
    let att = v.get("backstageAttachment");
    let mut video = None;
    let mut poll = Vec::new();
    let mut poll_votes = None;
    if let Some(att) = att {
        if let Some(u) = best_thumb(at(att, "backstageImageRenderer.image")) {
            images.push(img(&u));
        }
        if let Some(Value::Array(list)) = at(att, "postMultiImageRenderer.images") {
            for i in list {
                if let Some(u) = best_thumb(at(i, "backstageImageRenderer.image")) {
                    images.push(img(&u));
                }
            }
        }
        if let Some(vr) = att.get("videoRenderer") {
            video = video_renderer(vr);
        }
        if let Some(pr) = att.get("pollRenderer") {
            if let Some(Value::Array(choices)) = pr.get("choices") {
                poll = choices.iter().map(|c| t(c, "text")).collect();
            }
            poll_votes = opt_t(pr, "totalVotes");
        }
    }
    Some(Post {
        id,
        author: t(v, "authorText"),
        author_id,
        author_thumb: best_thumb(v.get("authorThumbnail")).map(|u| img(&u)).unwrap_or_default(),
        text: t(v, "contentText"),
        published: t(v, "publishedTimeText"),
        likes: t(v, "voteCount"),
        comments: t(v, "actionButtons.commentActionButtonsRenderer.replyButton.buttonRenderer.text"),
        images,
        video,
        poll,
        poll_votes,
    })
}
