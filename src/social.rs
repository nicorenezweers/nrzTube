//! Social: TikTok, Instagram und X in nrzTube – Profile, Beiträge und ein gemischter Feed der
//! gefolgten Konten. Wie bei YouTube holt der Server alles selbst; Bilder und Videos laufen
//! über den eigenen Proxy, der Browser spricht nie mit den Plattformen.
//!
//! - **TikTok**: Profile und Videos über yt-dlp (ohne Anmeldung), Profilangaben aus der
//!   Profilseite. Videos lädt der Server einmal herunter und spielt sie aus dem Cache ab
//!   (TikToks Video-Adressen funktionieren nur mit den Cookies des Abrufs).
//! - **Instagram**: Profile nur mit hinterlegter Anmeldung (cookies.txt), einzelne Beiträge
//!   über yt-dlp bzw. die Einbettungsseite.
//! - **X**: Profile über die Einbettungs-Zeitleiste (mit Anmeldung zuverlässiger), einzelne
//!   Beiträge über die öffentliche Einbettungs-Schnittstelle.

use crate::proxy::{img, media};
use anyhow::{anyhow, Result};
use futures::StreamExt;
use nrztube_types::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::{Mutex, OnceCell, Semaphore};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
/// Profile: 20 Minuten, Fehler nur 3 Minuten merken
const TTL: Duration = Duration::from_secs(20 * 60);
const ERR_TTL: Duration = Duration::from_secs(3 * 60);
/// Heruntergeladene Videos nach 12 Stunden löschen
const VIDEO_TTL: Duration = Duration::from_secs(12 * 3600);

const IG_LOGIN: &str = "Instagram zeigt Profile nur angemeldet. Ein Admin kann unter Einstellungen → Social eine Instagram-Anmeldung (cookies.txt) hinterlegen.";
const X_LIMIT: &str = "X begrenzt Abrufe ohne Anmeldung gerade. Später erneut versuchen – oder ein Admin hinterlegt unter Einstellungen → Social eine X-Anmeldung (cookies.txt).";

type Slot = Arc<OnceCell<Result<PathBuf, String>>>;
type ProfileCache = HashMap<String, (Instant, Result<Arc<SocialProfile>, String>)>;

pub struct Social {
    http: reqwest::Client,
    ytdlp: Arc<crate::ytdlp::YtDlp>,
    data: PathBuf,
    profiles: Mutex<ProfileCache>,
    posts: Mutex<HashMap<String, (Instant, Arc<SocialPost>)>>,
    downloads: Mutex<HashMap<String, Slot>>,
    /// Gleichzeitige yt-dlp-Aufrufe für Social
    limit: Semaphore,
}

pub fn valid_platform(p: &str) -> bool {
    SOCIAL_PLATFORMS.contains(&p)
}

pub fn platform_name(p: &str) -> &'static str {
    match p {
        "tiktok" => "TikTok",
        "instagram" => "Instagram",
        "x" => "X",
        _ => "?",
    }
}

/// Handle prüfen und vereinheitlichen (ohne @, klein)
pub fn clean_handle(platform: &str, h: &str) -> Option<String> {
    let h = h.trim().trim_start_matches('@').to_lowercase();
    let (max, dot) = match platform {
        "tiktok" => (24, true),
        "instagram" => (30, true),
        "x" => (15, false),
        _ => return None,
    };
    let ok = !h.is_empty()
        && h.len() <= max
        && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || (dot && c == '.'));
    ok.then_some(h)
}

/// Beitrags-Kennung prüfen: TikTok und X Ziffern, Instagram Shortcode
pub fn valid_id(platform: &str, id: &str) -> bool {
    match platform {
        "tiktok" | "x" => (5..=25).contains(&id.len()) && id.chars().all(|c| c.is_ascii_digit()),
        "instagram" => (5..=48).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
        _ => false,
    }
}

fn cookie_domains(p: &str) -> &'static [&'static str] {
    match p {
        "tiktok" => &["tiktok.com"],
        "instagram" => &["instagram.com"],
        "x" => &["x.com", "twitter.com"],
        _ => &[],
    }
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn n(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64)).or_else(|| x.as_str().and_then(|s| s.parse().ok())))
}

fn count(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(|x| n(x, "count"))
}

fn parse_date(d: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(d)
        .or_else(|_| chrono::DateTime::parse_from_str(d, "%a %b %d %H:%M:%S %z %Y"))
        .map(|d| d.timestamp())
        .unwrap_or(0)
}

fn html_unescape(t: &str) -> String {
    t.replace("&quot;", "\"").replace("&#039;", "'").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

impl Social {
    pub fn new(ytdlp: Arc<crate::ytdlp::YtDlp>, data: PathBuf) -> Arc<Self> {
        let http = reqwest::Client::builder()
            .user_agent(UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Arc::new(Self {
            http,
            ytdlp,
            data,
            profiles: Mutex::new(HashMap::new()),
            posts: Mutex::new(HashMap::new()),
            downloads: Mutex::new(HashMap::new()),
            limit: Semaphore::new(3),
        })
    }

    // -------------------------------------------------------------- Anmeldung (cookies.txt)

    fn cookie_path(&self, p: &str) -> PathBuf {
        self.data.join(format!("{p}-cookies.txt"))
    }

    pub fn has_login(&self, p: &str) -> bool {
        valid_platform(p) && self.cookie_path(p).exists()
    }

    pub fn status(&self) -> Vec<SocialStatus> {
        SOCIAL_PLATFORMS
            .iter()
            .map(|p| SocialStatus { platform: p.to_string(), login: self.has_login(p), needs_login: *p != "tiktok" })
            .collect()
    }

    /// cookies.txt einer Plattform speichern (leer = löschen)
    pub async fn set_cookies(&self, p: &str, text: &str) -> Result<()> {
        if !valid_platform(p) {
            return Err(anyhow!("Unbekannte Plattform"));
        }
        let path = self.cookie_path(p);
        if text.trim().is_empty() {
            let _ = tokio::fs::remove_file(&path).await;
        } else {
            let domains = cookie_domains(p);
            let ok = text.lines().any(|l| {
                let f: Vec<&str> = l.trim_start_matches("#HttpOnly_").split('\t').collect();
                f.len() >= 7 && domains.iter().any(|d| f[0].contains(d))
            });
            if !ok {
                return Err(anyhow!(
                    "Keine {}-Cookies gefunden – bitte eine cookies.txt im Netscape-Format (z. B. mit der Erweiterung „Get cookies.txt LOCALLY“) von {} exportieren",
                    platform_name(p),
                    domains[0]
                ));
            }
            let mut body = String::from("# Netscape HTTP Cookie File\n");
            body.push_str(text.trim_start_matches("# Netscape HTTP Cookie File").trim_start());
            body.push('\n');
            tokio::fs::write(&path, body).await?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
        // Fehler dieser Plattform vergessen, damit es gleich neu versucht wird
        let prefix = format!("{p}:");
        self.profiles.lock().await.retain(|k, (_, r)| !(k.starts_with(&prefix) && r.is_err()));
        Ok(())
    }

    /// Cookie-Kopfzeile aus der cookies.txt (nur nicht abgelaufene der Plattform)
    fn cookie_header(&self, p: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.cookie_path(p)).ok()?;
        let domains = cookie_domains(p);
        let now = chrono::Utc::now().timestamp();
        let pairs: Vec<String> = text
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.trim_start_matches("#HttpOnly_").split('\t').collect();
                if f.len() < 7 || l.starts_with("# ") || !domains.iter().any(|d| f[0].contains(d)) {
                    return None;
                }
                let exp: i64 = f[4].parse().unwrap_or(0);
                (exp == 0 || exp > now).then(|| format!("{}={}", f[5], f[6].trim()))
            })
            .collect();
        (!pairs.is_empty()).then(|| pairs.join("; "))
    }

    /// yt-dlp schreibt Cookie-Dateien zurück – mit einer Kopie pro Aufruf arbeiten
    async fn cookie_copy(&self, p: &str) -> Option<PathBuf> {
        let src = self.cookie_path(p);
        if !src.exists() {
            return None;
        }
        let tmp = std::env::temp_dir().join(format!("nrztube-{p}-{}-{}.txt", std::process::id(), crate::auth::new_token()));
        tokio::fs::copy(&src, &tmp).await.ok()?;
        Some(tmp)
    }

    async fn ytdlp_json(&self, p: &str, url: &str, extra: &[&str], secs: u64) -> Result<Value> {
        let _permit = self.limit.acquire().await?;
        let bin = self.ytdlp.bin().await;
        let mut args: Vec<String> = ["-J", "--no-warnings", "--skip-download", "--no-cache-dir"].iter().map(|x| x.to_string()).collect();
        args.extend(extra.iter().map(|x| x.to_string()));
        let cookies = self.cookie_copy(p).await;
        if let Some(c) = &cookies {
            args.push("--cookies".into());
            args.push(c.to_string_lossy().to_string());
        }
        args.push(url.to_string());
        let out = tokio::time::timeout(Duration::from_secs(secs), Command::new(&bin).args(&args).kill_on_drop(true).output()).await;
        if let Some(c) = &cookies {
            let _ = tokio::fs::remove_file(c).await;
        }
        let out = out.map_err(|_| anyhow!("{}: Zeitüberschreitung", platform_name(p)))??;
        if !out.status.success() {
            return Err(anyhow!(ytdlp_error(p, &String::from_utf8_lossy(&out.stderr))));
        }
        Ok(serde_json::from_slice(&out.stdout)?)
    }

    // -------------------------------------------------------------- Öffentliche Schnittstelle

    /// Profil mit den neuesten Beiträgen (zwischengespeichert)
    pub async fn profile(&self, p: &str, h: &str, fresh: bool) -> Result<Arc<SocialProfile>> {
        let key = format!("{p}:{h}");
        if !fresh {
            if let Some((t, r)) = self.profiles.lock().await.get(&key) {
                let ttl = if r.is_ok() { TTL } else { ERR_TTL };
                if t.elapsed() < ttl {
                    return r.clone().map_err(|e| anyhow!(e));
                }
            }
        }
        let r = match p {
            "tiktok" => self.tiktok_profile(h).await,
            "instagram" => self.instagram_profile(h).await,
            "x" => self.x_profile(h).await,
            _ => Err(anyhow!("Unbekannte Plattform")),
        }
        .map(Arc::new)
        .map_err(|e| e.to_string());
        {
            let mut c = self.profiles.lock().await;
            c.retain(|_, (t, _)| t.elapsed() < TTL);
            c.insert(key, (Instant::now(), r.clone()));
        }
        r.map_err(|e| anyhow!(e))
    }

    /// Ein einzelner Beitrag
    pub async fn post(&self, p: &str, id: &str, h: Option<&str>) -> Result<Arc<SocialPost>> {
        let key = format!("{p}:{id}");
        if let Some((t, post)) = self.posts.lock().await.get(&key) {
            if t.elapsed() < TTL {
                return Ok(post.clone());
            }
        }
        let post = Arc::new(match p {
            "tiktok" => self.tiktok_post(id, h).await?,
            "instagram" => self.instagram_post(id).await?,
            "x" => self.x_post(id).await?,
            _ => return Err(anyhow!("Unbekannte Plattform")),
        });
        let mut c = self.posts.lock().await;
        c.retain(|_, (t, _)| t.elapsed() < TTL);
        c.insert(key, (Instant::now(), post.clone()));
        Ok(post)
    }

    /// Gemischter Feed aller gefolgten Konten (neueste zuerst)
    pub async fn feed(&self, follows: Vec<SocialFollow>, fresh: bool) -> SocialFeed {
        let results: Vec<(SocialFollow, Result<Arc<SocialProfile>>)> = futures::stream::iter(follows)
            .map(|f| async move {
                let r = self.profile(&f.platform, &f.handle, fresh).await;
                (f, r)
            })
            .buffer_unordered(4)
            .collect()
            .await;
        let mut feed = SocialFeed::default();
        let mut seen = std::collections::HashSet::new();
        for (f, r) in results {
            match r {
                Ok(prof) => {
                    for post in &prof.posts {
                        if seen.insert(format!("{}:{}", post.platform, post.id)) {
                            feed.posts.push(post.clone());
                        }
                    }
                }
                Err(e) => feed.errors.push(format!("{} @{}: {e}", platform_name(&f.platform), f.handle)),
            }
        }
        feed.posts.sort_by(|a, b| b.published.cmp(&a.published));
        feed.posts.truncate(240);
        feed.errors.sort();
        feed
    }

    // -------------------------------------------------------------- Videos aus dem Cache

    /// Video herunterladen (einmal pro Beitrag, auch bei mehreren gleichzeitigen Anfragen)
    pub async fn video_file(&self, p: &str, id: &str, h: Option<&str>) -> Result<PathBuf> {
        let dir = self.data.join("cache").join("social");
        let path = dir.join(format!("{p}-{id}.mp4"));
        if path.exists() {
            return Ok(path);
        }
        let key = format!("{p}:{id}");
        let slot = self.downloads.lock().await.entry(key.clone()).or_insert_with(|| Arc::new(OnceCell::new())).clone();
        let r = slot
            .get_or_init(|| async { self.download(p, id, h, &dir, &path).await.map_err(|e| e.to_string()) })
            .await
            .clone();
        self.downloads.lock().await.remove(&key);
        r.map_err(|e| anyhow!(e))
    }

    async fn download(&self, p: &str, id: &str, h: Option<&str>, dir: &std::path::Path, path: &std::path::Path) -> Result<PathBuf> {
        tokio::fs::create_dir_all(dir).await?;
        cleanup(dir).await;
        let url = match p {
            "tiktok" => format!("https://www.tiktok.com/@{}/video/{id}", h.unwrap_or("_")),
            "instagram" => format!("https://www.instagram.com/p/{id}/"),
            _ => format!("https://x.com/i/status/{id}"),
        };
        let _permit = self.limit.acquire().await?;
        let tmp = path.with_extension("part.mp4");
        let bin = self.ytdlp.bin().await;
        let mut args: Vec<String> = [
            "-q", "--no-warnings", "--no-cache-dir", "--no-part", "--playlist-items", "1",
            "-f", "b[vcodec^=h264][ext=mp4]/b[vcodec^=avc][ext=mp4]/b[ext=mp4]/bv*[ext=mp4]+ba[ext=m4a]/b",
            "--merge-output-format", "mp4", "--force-overwrites", "-o",
        ]
        .iter()
        .map(|x| x.to_string())
        .collect();
        args.push(tmp.to_string_lossy().to_string());
        let cookies = self.cookie_copy(p).await;
        if let Some(c) = &cookies {
            args.push("--cookies".into());
            args.push(c.to_string_lossy().to_string());
        }
        args.push(url);
        let out = tokio::time::timeout(Duration::from_secs(180), Command::new(&bin).args(&args).kill_on_drop(true).output()).await;
        if let Some(c) = &cookies {
            let _ = tokio::fs::remove_file(c).await;
        }
        let out = out.map_err(|_| anyhow!("Video: Zeitüberschreitung"))??;
        if !out.status.success() || !tmp.exists() {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(anyhow!(ytdlp_error(p, &String::from_utf8_lossy(&out.stderr))));
        }
        tokio::fs::rename(&tmp, path).await?;
        Ok(path.to_path_buf())
    }

    // -------------------------------------------------------------- TikTok

    async fn tiktok_profile(&self, h: &str) -> Result<SocialProfile> {
        let url = format!("https://www.tiktok.com/@{h}");
        let (info, list) = tokio::join!(
            self.tiktok_user_info(h),
            self.ytdlp_json("tiktok", &url, &["--flat-playlist", "--playlist-end", "24"], 75)
        );
        let list = list?;
        let mut prof = info.unwrap_or_else(|_| SocialProfile { platform: "tiktok".into(), handle: h.to_string(), ..Default::default() });
        prof.url = url;
        if let Some(Value::Array(entries)) = list.get("entries") {
            prof.posts = entries.iter().filter_map(|e| tiktok_entry(e, h)).collect();
        }
        if prof.name.is_empty() {
            prof.name = prof.posts.first().map(|p| p.author_name.clone()).filter(|x| !x.is_empty()).unwrap_or_else(|| h.to_string());
        }
        for post in prof.posts.iter_mut() {
            if post.author_avatar.is_empty() {
                post.author_avatar = prof.avatar.clone();
            }
            if post.author_name.is_empty() {
                post.author_name = prof.name.clone();
            }
        }
        Ok(prof)
    }

    /// Name, Bild, Bio und Zahlen aus der Profilseite (klappt nicht immer – dann ohne)
    async fn tiktok_user_info(&self, h: &str) -> Result<SocialProfile> {
        let html = self.http.get(format!("https://www.tiktok.com/@{h}")).header("accept-language", "de-DE,de;q=0.9,en;q=0.8").send().await?.text().await?;
        let start = html.find("__UNIVERSAL_DATA_FOR_REHYDRATION__").ok_or_else(|| anyhow!("keine Profildaten"))?;
        let rest = &html[start..];
        let json_start = rest.find('>').ok_or_else(|| anyhow!("keine Profildaten"))? + 1;
        let json_end = rest[json_start..].find("</script>").ok_or_else(|| anyhow!("keine Profildaten"))?;
        let v: Value = serde_json::from_str(&rest[json_start..json_start + json_end])?;
        let info = &v["__DEFAULT_SCOPE__"]["webapp.user-detail"]["userInfo"];
        let u = &info["user"];
        if !u.is_object() {
            return Err(anyhow!("keine Profildaten"));
        }
        let st = &info["stats"];
        let avatar = [s(u, "avatarLarger"), s(u, "avatarMedium"), s(u, "avatarThumb")].into_iter().find(|x| !x.is_empty()).unwrap_or_default();
        Ok(SocialProfile {
            platform: "tiktok".into(),
            handle: h.to_string(),
            name: s(u, "nickname"),
            avatar: if avatar.is_empty() { String::new() } else { img(&avatar) },
            bio: s(u, "signature"),
            followers: n(st, "followerCount"),
            following: n(st, "followingCount"),
            post_count: n(st, "videoCount"),
            verified: u.get("verified").and_then(Value::as_bool).unwrap_or(false),
            ..Default::default()
        })
    }

    async fn tiktok_post(&self, id: &str, h: Option<&str>) -> Result<SocialPost> {
        let url = format!("https://www.tiktok.com/@{}/video/{id}", h.unwrap_or("_"));
        let v = self.ytdlp_json("tiktok", &url, &["--no-playlist"], 45).await?;
        tiktok_entry(&v, h.unwrap_or("")).ok_or_else(|| anyhow!("TikTok: Beitrag nicht lesbar"))
    }

    // -------------------------------------------------------------- Instagram

    async fn instagram_profile(&self, h: &str) -> Result<SocialProfile> {
        let Some(cookie) = self.cookie_header("instagram") else { return Err(anyhow!(IG_LOGIN)) };
        let csrf = cookie.split("; ").find_map(|c| c.strip_prefix("csrftoken=")).unwrap_or("").to_string();
        let res = self
            .http
            .get(format!("https://www.instagram.com/api/v1/users/web_profile_info/?username={h}"))
            .header("x-ig-app-id", "936619743392459")
            .header("x-csrftoken", csrf)
            .header("x-requested-with", "XMLHttpRequest")
            .header("referer", format!("https://www.instagram.com/{h}/"))
            .header("accept", "*/*")
            .header("cookie", cookie)
            .send()
            .await?;
        let status = res.status().as_u16();
        let text = res.text().await?;
        match status {
            404 => return Err(anyhow!("Instagram-Konto nicht gefunden")),
            429 => return Err(anyhow!("Instagram begrenzt Abrufe gerade – bitte später erneut versuchen")),
            401 | 403 => return Err(anyhow!("Instagram lehnt die hinterlegte Anmeldung ab – bitte eine neue cookies.txt hinterlegen")),
            _ => {}
        }
        let v: Value = serde_json::from_str(&text).map_err(|_| anyhow!("Instagram lehnt die hinterlegte Anmeldung ab – bitte eine neue cookies.txt hinterlegen"))?;
        let u = &v["data"]["user"];
        if !u.is_object() {
            return Err(anyhow!("Instagram-Konto nicht gefunden"));
        }
        let pic = [s(u, "profile_pic_url_hd"), s(u, "profile_pic_url")].into_iter().find(|x| !x.is_empty()).unwrap_or_default();
        let mut prof = SocialProfile {
            platform: "instagram".into(),
            handle: h.to_string(),
            name: s(u, "full_name"),
            avatar: if pic.is_empty() { String::new() } else { img(&pic) },
            bio: s(u, "biography"),
            followers: count(u, "edge_followed_by"),
            following: count(u, "edge_follow"),
            post_count: count(u, "edge_owner_to_timeline_media"),
            verified: u.get("is_verified").and_then(Value::as_bool).unwrap_or(false),
            url: format!("https://www.instagram.com/{h}/"),
            ..Default::default()
        };
        if prof.name.is_empty() {
            prof.name = h.to_string();
        }
        if let Some(Value::Array(edges)) = u["edge_owner_to_timeline_media"].get("edges") {
            for e in edges {
                if let Some(mut post) = ig_node(&e["node"], h) {
                    post.author_name = prof.name.clone();
                    post.author_avatar = prof.avatar.clone();
                    prof.posts.push(post);
                }
            }
        }
        if u.get("is_private").and_then(Value::as_bool) == Some(true) && prof.posts.is_empty() {
            return Err(anyhow!("Dieses Instagram-Konto ist privat"));
        }
        Ok(prof)
    }

    async fn instagram_post(&self, code: &str) -> Result<SocialPost> {
        let url = format!("https://www.instagram.com/p/{code}/");
        match self.ytdlp_json("instagram", &url, &[], 45).await {
            Ok(v) => ig_ytdlp(&v, code).ok_or_else(|| anyhow!("Instagram: Beitrag nicht lesbar")),
            // Reine Bild-Beiträge kann yt-dlp nicht – dann die öffentliche Einbettungsseite
            Err(e) => self.instagram_embed(code).await.map_err(|_| e),
        }
    }

    async fn instagram_embed(&self, code: &str) -> Result<SocialPost> {
        let html = self.http.get(format!("https://www.instagram.com/p/{code}/embed/captioned/")).send().await?.text().await?;
        let image = between(&html, "class=\"EmbeddedMediaImage\"", "src=\"", "\"").map(|x| html_unescape(&x)).ok_or_else(|| anyhow!("Instagram: Beitrag nicht gefunden"))?;
        let author = between(&html, "class=\"UsernameText\"", ">", "<").unwrap_or_default();
        let caption = html
            .find("class=\"Caption\"")
            .map(|i| {
                let rest = &html[i..];
                let end = rest.find("class=\"CaptionComments\"").unwrap_or(rest.len().min(4000));
                strip_tags(&rest[..end])
            })
            .unwrap_or_default();
        let caption = caption.strip_prefix(&author).unwrap_or(&caption).trim().to_string();
        Ok(SocialPost {
            platform: "instagram".into(),
            id: code.to_string(),
            author: author.to_lowercase(),
            author_name: author,
            text: caption,
            media: vec![SocialMedia { kind: "image".into(), url: img(&image), ..Default::default() }],
            url: format!("https://www.instagram.com/p/{code}/"),
            ..Default::default()
        })
    }

    // -------------------------------------------------------------- X

    async fn x_profile(&self, h: &str) -> Result<SocialProfile> {
        let mut req = self.http.get(format!("https://syndication.twitter.com/srv/timeline-profile/screen-name/{h}?showReplies=false"));
        if let Some(c) = self.cookie_header("x") {
            req = req.header("cookie", c);
        }
        let res = req.send().await?;
        let status = res.status().as_u16();
        let html = res.text().await?;
        if status == 429 || html.trim_start().starts_with("Rate limit") {
            return Err(anyhow!(X_LIMIT));
        }
        let json = between(&html, "id=\"__NEXT_DATA__\"", ">", "</script>").ok_or_else(|| anyhow!("X: Konto nicht gefunden"))?;
        let v: Value = serde_json::from_str(&json)?;
        let entries = v["props"]["pageProps"]["timeline"]["entries"].as_array().cloned().unwrap_or_default();
        let mut prof = SocialProfile { platform: "x".into(), handle: h.to_string(), url: format!("https://x.com/{h}"), ..Default::default() };
        for e in &entries {
            let t = &e["content"]["tweet"];
            if !t.is_object() {
                continue;
            }
            let u = &t["user"];
            if prof.name.is_empty() && s(u, "screen_name").eq_ignore_ascii_case(h) {
                prof.name = s(u, "name");
                prof.avatar = x_avatar(&s(u, "profile_image_url_https"));
                prof.bio = s(u, "description");
                prof.followers = n(u, "followers_count");
                prof.following = n(u, "friends_count");
                prof.post_count = n(u, "statuses_count");
                prof.verified = u.get("verified").and_then(Value::as_bool).unwrap_or(false) || u.get("is_blue_verified").and_then(Value::as_bool).unwrap_or(false);
            }
            if let Some(post) = x_tweet(t) {
                prof.posts.push(post);
            }
        }
        if prof.posts.is_empty() {
            return Err(anyhow!("X: Keine Beiträge – das Konto ist privat, gesperrt oder existiert nicht"));
        }
        if prof.name.is_empty() {
            prof.name = h.to_string();
        }
        Ok(prof)
    }

    async fn x_post(&self, id: &str) -> Result<SocialPost> {
        let res = self
            .http
            .get(format!("https://cdn.syndication.twimg.com/tweet-result?id={id}&token={}&lang=de", x_token(id)))
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!("X: Beitrag nicht gefunden (gelöscht oder privat)"));
        }
        let v: Value = res.json().await?;
        if s(&v, "__typename") == "TweetTombstone" {
            return Err(anyhow!("X: Beitrag nicht verfügbar"));
        }
        x_tweet(&v).ok_or_else(|| anyhow!("X: Beitrag nicht lesbar"))
    }
}

// ------------------------------------------------------------------ Umwandlung

fn ytdlp_error(p: &str, stderr: &str) -> String {
    let msg = stderr.lines().filter(|l| l.contains("ERROR")).last().unwrap_or(stderr.trim()).replace("ERROR: ", "");
    // "[tiktok:user] name: Meldung" → "Meldung"
    let msg = match msg.strip_prefix('[').and_then(|r| r.split_once("] ")) {
        Some((_, rest)) => rest.split_once(": ").map(|(_, m)| m.to_string()).unwrap_or(rest.to_string()),
        None => msg,
    };
    let lower = msg.to_lowercase();
    if lower.contains("login") || lower.contains("cookies") || lower.contains("rate-limit") {
        return match p {
            "instagram" => IG_LOGIN.to_string(),
            "x" => X_LIMIT.to_string(),
            _ => format!("{}: Anmeldung nötig – ein Admin kann unter Einstellungen → Social eine cookies.txt hinterlegen", platform_name(p)),
        };
    }
    let msg: String = msg.split(" please report this issue").next().unwrap_or("").trim().trim_end_matches(';').chars().take(240).collect();
    format!("{}: {}", platform_name(p), if msg.is_empty() { "Abruf fehlgeschlagen" } else { &msg })
}

fn tiktok_entry(v: &Value, h: &str) -> Option<SocialPost> {
    let id = s(v, "id");
    if !valid_id("tiktok", &id) {
        return None;
    }
    let author = Some(s(v, "uploader")).filter(|x| clean_handle("tiktok", x).is_some()).unwrap_or_else(|| h.to_string()).to_lowercase();
    let thumbs = v.get("thumbnails").and_then(Value::as_array).cloned().unwrap_or_default();
    let thumb = ["originCover", "cover", "dynamicCover"]
        .iter()
        .find_map(|k| thumbs.iter().find(|t| t.get("id").and_then(Value::as_str) == Some(k)).map(|t| s(t, "url")))
        .or_else(|| Some(s(v, "thumbnail")).filter(|x| !x.is_empty()))
        .or_else(|| thumbs.first().map(|t| s(t, "url")))
        .unwrap_or_default();
    let text = Some(s(v, "description")).filter(|x| !x.trim().is_empty()).unwrap_or_else(|| s(v, "title"));
    let width = n(v, "width").unwrap_or(0) as u32;
    let height = n(v, "height").unwrap_or(0) as u32;
    Some(SocialPost {
        platform: "tiktok".into(),
        id: id.clone(),
        author: author.clone(),
        author_name: s(v, "channel"),
        author_avatar: String::new(),
        text: text.trim().to_string(),
        published: v.get("timestamp").and_then(Value::as_i64).unwrap_or(0),
        media: vec![SocialMedia {
            kind: "video".into(),
            url: format!("/api/social/play/tiktok/{id}?h={author}"),
            thumb: if thumb.is_empty() { String::new() } else { img(&thumb) },
            width: if width == 0 { 9 } else { width },
            height: if height == 0 { 16 } else { height },
        }],
        likes: n(v, "like_count"),
        comments: n(v, "comment_count"),
        views: n(v, "view_count"),
        reposts: n(v, "repost_count"),
        duration: v.get("duration").and_then(Value::as_f64),
        repost_by: None,
        url: format!("https://www.tiktok.com/@{author}/video/{id}"),
    })
}

fn ig_media(n: &Value, code: &str) -> SocialMedia {
    let display = s(n, "display_url");
    let width = n["dimensions"].get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
    let height = n["dimensions"].get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
    let thumb = if display.is_empty() { String::new() } else { img(&display) };
    if n.get("is_video").and_then(Value::as_bool) == Some(true) {
        let url = Some(s(n, "video_url")).filter(|x| !x.is_empty()).map(|u| media(&u)).unwrap_or_else(|| format!("/api/social/play/instagram/{code}"));
        SocialMedia { kind: "video".into(), url, thumb, width, height }
    } else {
        SocialMedia { kind: "image".into(), url: thumb.clone(), thumb, width, height }
    }
}

fn ig_node(node: &Value, h: &str) -> Option<SocialPost> {
    let code = s(node, "shortcode");
    if !valid_id("instagram", &code) {
        return None;
    }
    let text = node["edge_media_to_caption"]["edges"].get(0).map(|e| s(&e["node"], "text")).unwrap_or_default();
    let media = match node["edge_sidecar_to_children"].get("edges").and_then(Value::as_array) {
        Some(children) if !children.is_empty() => children.iter().map(|c| ig_media(&c["node"], &code)).collect(),
        _ => vec![ig_media(node, &code)],
    };
    Some(SocialPost {
        platform: "instagram".into(),
        id: code.clone(),
        author: h.to_string(),
        text,
        published: node.get("taken_at_timestamp").and_then(Value::as_i64).unwrap_or(0),
        media,
        likes: count(node, "edge_liked_by").or_else(|| count(node, "edge_media_preview_like")),
        comments: count(node, "edge_media_to_comment"),
        views: n(node, "video_view_count"),
        url: format!("https://www.instagram.com/p/{code}/"),
        ..Default::default()
    })
}

/// Beitrag aus yt-dlp (Video oder Karussell mit Videos)
fn ig_ytdlp(v: &Value, code: &str) -> Option<SocialPost> {
    let entries: Vec<Value> = match v.get("entries").and_then(Value::as_array) {
        Some(e) if !e.is_empty() => e.clone(),
        _ => vec![v.clone()],
    };
    let first = entries.first()?;
    let handle = [s(v, "channel"), s(first, "channel"), s(v, "uploader_id"), s(first, "uploader_id")]
        .into_iter()
        .find(|x| clean_handle("instagram", x).is_some())
        .unwrap_or_default()
        .to_lowercase();
    let media = entries
        .iter()
        .map(|e| {
            let thumb = Some(s(e, "thumbnail")).filter(|x| !x.is_empty()).map(|t| img(&t)).unwrap_or_default();
            // Beste Datei mit Bild und Ton direkt über den Proxy
            let best = e.get("formats").and_then(Value::as_array).and_then(|fs| {
                fs.iter()
                    .filter(|f| s(f, "vcodec") != "none" && s(f, "acodec") != "none" && s(f, "protocol").starts_with("http"))
                    .max_by_key(|f| n(f, "height").unwrap_or(0))
                    .map(|f| s(f, "url"))
            });
            SocialMedia {
                kind: "video".into(),
                url: best.filter(|u| !u.is_empty()).map(|u| media(&u)).unwrap_or_else(|| format!("/api/social/play/instagram/{code}")),
                thumb,
                width: n(e, "width").unwrap_or(0) as u32,
                height: n(e, "height").unwrap_or(0) as u32,
            }
        })
        .collect();
    let text = [s(v, "description"), s(first, "description")].into_iter().find(|x| !x.is_empty()).unwrap_or_default();
    Some(SocialPost {
        platform: "instagram".into(),
        id: code.to_string(),
        author: handle.clone(),
        author_name: [s(v, "uploader"), s(first, "uploader")].into_iter().find(|x| !x.is_empty()).unwrap_or(handle),
        text,
        published: v.get("timestamp").or_else(|| first.get("timestamp")).and_then(Value::as_i64).unwrap_or(0),
        media,
        likes: n(v, "like_count").or_else(|| n(first, "like_count")),
        comments: n(v, "comment_count").or_else(|| n(first, "comment_count")),
        duration: first.get("duration").and_then(Value::as_f64),
        url: format!("https://www.instagram.com/p/{code}/"),
        ..Default::default()
    })
}

fn x_avatar(u: &str) -> String {
    if u.is_empty() {
        return String::new();
    }
    img(&u.replace("_normal.", "_400x400."))
}

fn x_media(m: &Value) -> Option<SocialMedia> {
    let pic = s(m, "media_url_https");
    let (width, height) = match m.get("original_info") {
        Some(o) => (n(o, "width").unwrap_or(0) as u32, n(o, "height").unwrap_or(0) as u32),
        None => (0, 0),
    };
    match s(m, "type").as_str() {
        "photo" if !pic.is_empty() => {
            let big = img(&format!("{pic}?name=large"));
            Some(SocialMedia { kind: "image".into(), url: big, thumb: img(&format!("{pic}?name=small")), width, height })
        }
        "video" | "animated_gif" => {
            let variants = m["video_info"]["variants"].as_array()?;
            let best = variants
                .iter()
                .filter(|v| s(v, "content_type") == "video/mp4")
                .max_by_key(|v| n(v, "bitrate").unwrap_or(0))?;
            Some(SocialMedia {
                kind: "video".into(),
                url: media(&s(best, "url")),
                thumb: if pic.is_empty() { String::new() } else { img(&pic) },
                width,
                height,
            })
        }
        _ => None,
    }
}

fn x_tweet(t: &Value) -> Option<SocialPost> {
    if let Some(rt) = t.get("retweeted_status").filter(|x| x.is_object()) {
        let mut inner = x_tweet(rt)?;
        inner.repost_by = Some(s(&t["user"], "screen_name"));
        return Some(inner);
    }
    let id = s(t, "id_str");
    if !valid_id("x", &id) {
        return None;
    }
    let u = &t["user"];
    let screen = s(u, "screen_name");
    let mut text = Some(s(t, "full_text")).filter(|x| !x.is_empty()).unwrap_or_else(|| s(t, "text"));
    // Kurzlinks auflösen, Medien-Links entfernen
    if let Some(Value::Array(urls)) = t["entities"].get("urls") {
        for l in urls {
            let (short, long) = (s(l, "url"), s(l, "expanded_url"));
            if !short.is_empty() && !long.is_empty() {
                text = text.replace(&short, &long);
            }
        }
    }
    let media_list = t["extended_entities"]
        .get("media")
        .or_else(|| t.get("mediaDetails"))
        .or_else(|| t["entities"].get("media"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for m in &media_list {
        let short = s(m, "url");
        if !short.is_empty() {
            text = text.replace(&short, "");
        }
    }
    let media: Vec<SocialMedia> = media_list.iter().filter_map(x_media).collect();
    let duration = media_list
        .iter()
        .find_map(|m| m["video_info"].get("duration_millis").and_then(Value::as_f64))
        .map(|ms| ms / 1000.0);
    Some(SocialPost {
        platform: "x".into(),
        id: id.clone(),
        author: screen.to_lowercase(),
        author_name: s(u, "name"),
        author_avatar: x_avatar(&s(u, "profile_image_url_https")),
        text: html_unescape(text.trim()),
        published: parse_date(&s(t, "created_at")),
        media,
        likes: n(t, "favorite_count"),
        comments: n(t, "reply_count").or_else(|| n(t, "conversation_count")),
        views: None,
        reposts: n(t, "retweet_count"),
        duration,
        repost_by: None,
        url: format!("https://x.com/{screen}/status/{id}"),
    })
}

/// Prüfzeichen der Einbettungs-Schnittstelle: ((id / 1e15) · π) zur Basis 36, ohne Nullen und Punkt
fn x_token(id: &str) -> String {
    let x = id.parse::<f64>().unwrap_or(0.0) / 1e15 * std::f64::consts::PI;
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut ip = x.trunc() as u64;
    let mut fp = x.fract();
    let mut int = Vec::new();
    while ip > 0 {
        int.push(D[(ip % 36) as usize]);
        ip /= 36;
    }
    int.reverse();
    let mut out = String::from_utf8(int).unwrap_or_default();
    for _ in 0..10 {
        fp *= 36.0;
        out.push(D[fp.trunc() as usize % 36] as char);
        fp = fp.fract();
    }
    out.replace('0', "")
}

fn between(text: &str, anchor: &str, start: &str, end: &str) -> Option<String> {
    let i = text.find(anchor)?;
    let rest = &text[i + anchor.len()..];
    let j = rest.find(start)? + start.len();
    let k = rest[j..].find(end)?;
    Some(rest[j..j + k].to_string())
}

fn strip_tags(html: &str) -> String {
    let html = html.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n");
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Der Rest des Anfangs-Tags ("class=\"Caption\">") fällt beim ersten '>' weg
    let out = out.split_once('>').map(|(_, r)| r.to_string()).unwrap_or(out);
    html_unescape(out.trim())
}

/// Alte Videos im Cache löschen
async fn cleanup(dir: &std::path::Path) {
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else { return };
    while let Ok(Some(e)) = rd.next_entry().await {
        if let Ok(meta) = e.metadata().await {
            let old = meta.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age > VIDEO_TTL);
            if old {
                let _ = tokio::fs::remove_file(e.path()).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles() {
        assert_eq!(clean_handle("tiktok", "@Khaby.Lame").as_deref(), Some("khaby.lame"));
        assert_eq!(clean_handle("x", "a.b"), None);
        assert_eq!(clean_handle("x", "NASA").as_deref(), Some("nasa"));
        assert!(valid_id("instagram", "DQ3y7vKjK6y"));
        assert!(!valid_id("x", "12ab"));
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date("2022-10-26T18:45:58.000Z"), 1666809958);
        assert_eq!(parse_date("Wed Oct 26 18:45:58 +0000 2022"), 1666809958);
    }

    #[test]
    fn token() {
        assert!(!x_token("1585341984679469056").is_empty());
    }
}
