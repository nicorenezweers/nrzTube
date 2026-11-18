// nrzTube – kleine Brücke zwischen der Oberfläche (Rust/WebAssembly) und dem <video>-Element:
// HLS über hls.js (Apache-2.0), Qualitätswahl, Bildschirmfoto, Bild-im-Bild, Vollbild.
// Alle Adressen zeigen auf den nrzTube-Server; der Browser spricht nie mit Google.
(function () {
  const players = {};

  function el(id) {
    return document.getElementById(id);
  }

  window.nrzAttach = function (id, src, start, autoplay, maxHeight, kind) {
    const v = el(id);
    if (!v) return false;
    window.nrzDetach(id);
    const begin = function () {
      if (start > 0) {
        try { v.currentTime = start; } catch (e) {}
      }
      if (autoplay) {
        const p = v.play();
        if (p && p.catch) p.catch(function () {});
      }
    };
    if (kind === "hls" && window.Hls && window.Hls.isSupported()) {
      const hls = new window.Hls({
        startPosition: start > 0 ? start : -1,
        capLevelToPlayerSize: false,
        maxBufferLength: 30,
        backBufferLength: 60,
      });
      players[id] = { hls: hls, maxHeight: maxHeight };
      hls.on(window.Hls.Events.MANIFEST_PARSED, function () {
        if (maxHeight > 0) {
          let best = -1, bestH = 0;
          hls.levels.forEach(function (l, i) {
            if (l.height <= maxHeight && l.height >= bestH) { best = i; bestH = l.height; }
          });
          if (best >= 0) {
            hls.autoLevelCapping = -1;
            hls.startLevel = best;
            hls.currentLevel = best;
          }
        }
        if (autoplay) {
          const p = v.play();
          if (p && p.catch) p.catch(function () {});
        }
      });
      // Audiospur: bevorzugte Sprache (window.__nrzAudioPref: "de", "en" …, "original")
      let audioChosen = false;
      hls.on(window.Hls.Events.AUDIO_TRACKS_UPDATED, function () {
        if (audioChosen) return;
        audioChosen = true;
        const i = pickAudio(hls.audioTracks || [], window.__nrzAudioPref || "de");
        if (i >= 0 && i !== hls.audioTrack) hls.audioTrack = i;
      });
      hls.on(window.Hls.Events.ERROR, function (_e, data) {
        if (!data.fatal) return;
        if (data.type === window.Hls.ErrorTypes.NETWORK_ERROR) {
          hls.startLoad();
        } else if (data.type === window.Hls.ErrorTypes.MEDIA_ERROR) {
          hls.recoverMediaError();
        } else {
          v.dispatchEvent(new CustomEvent("nrzfatal", { detail: data.details || "Fehler" }));
        }
      });
      hls.loadSource(src);
      hls.attachMedia(v);
      return true;
    }
    // Safari spielt HLS selbst; sonst eine einzelne Datei (Bild+Ton oder nur Ton)
    v.src = src;
    v.addEventListener("loadedmetadata", begin, { once: true });
    v.load();
    return true;
  };

  // Mini-Player: an der Leiste ziehen, Position merken (am Rand gehalten)
  window.nrzMini = function (id) {
    const el = document.getElementById(id);
    if (!el) return;
    const clamp = (x, y) => {
      const r = el.getBoundingClientRect();
      return [Math.max(8, Math.min(innerWidth - r.width - 8, x)), Math.max(8, Math.min(innerHeight - r.height - 8, y))];
    };
    const place = (x, y) => {
      if (!el.classList.contains("mini-player")) return;
      if (!el.style.width) el.style.width = el.getBoundingClientRect().width + "px";
      [x, y] = clamp(x, y);
      el.style.left = x + "px"; el.style.top = y + "px"; el.style.right = "auto"; el.style.bottom = "auto";
    };
    if (!el.__nrzDrag) {
      el.__nrzDrag = true;
      el.addEventListener("pointerdown", (e) => {
        if (!el.classList.contains("mini-player") || !e.target.closest(".mini-drag") || e.target.closest("button,a,input")) return;
        e.preventDefault();
        const r = el.getBoundingClientRect();
        const dx = e.clientX - r.left, dy = e.clientY - r.top;
        el.classList.add("dragging");
        const mv = (ev) => place(ev.clientX - dx, ev.clientY - dy);
        const up = () => {
          removeEventListener("pointermove", mv); removeEventListener("pointerup", up); removeEventListener("pointercancel", up);
          el.classList.remove("dragging");
          const r2 = el.getBoundingClientRect();
          try { localStorage.setItem("nrzMiniPos", JSON.stringify({ x: r2.left, y: r2.top })); } catch (e) {}
        };
        addEventListener("pointermove", mv); addEventListener("pointerup", up); addEventListener("pointercancel", up);
      });
      addEventListener("resize", () => {
        if (el.classList.contains("mini-player") && el.style.left) {
          const r = el.getBoundingClientRect(); place(r.left, r.top);
        }
      });
    }
    try {
      const p = JSON.parse(localStorage.getItem("nrzMiniPos") || "null");
      if (p) requestAnimationFrame(() => place(p.x, p.y));
    } catch (e) {}
  };

  // Grosser Player: Position des Mini-Players vergessen (nur die Darstellung)
  window.nrzMiniClear = function (id) {
    const el = document.getElementById(id);
    if (!el) return;
    for (const k of ["left", "top", "right", "bottom", "width"]) el.style[k] = "";
  };

  // Gespeicherte Position zurücksetzen (wieder unten rechts)
  window.nrzMiniReset = function (id) {
    try { localStorage.removeItem("nrzMiniPos"); } catch (e) {}
    window.nrzMiniClear(id);
  };

  window.nrzDetach = function (id) {
    const p = players[id];
    if (p && p.hls) {
      try { p.hls.destroy(); } catch (e) {}
    }
    delete players[id];
  };

  window.nrzLevels = function (id) {
    const p = players[id];
    if (!p || !p.hls) return { levels: [], current: -1, auto: true };
    return {
      levels: p.hls.levels.map(function (l, i) {
        return { index: i, height: l.height || 0, bitrate: l.bitrate || 0, fps: Math.round(l.frameRate || 0) };
      }),
      current: p.hls.currentLevel,
      auto: p.hls.autoLevelEnabled,
    };
  };

  // Originalspur erkennen: YouTube nennt Synchronfassungen "… - dubbed" bzw. markiert "original"
  function isOriginal(t) {
    const n = (t.name || "").toLowerCase();
    return n.indexOf("original") >= 0 || (n.indexOf("dubbed") < 0 && n.indexOf("synchron") < 0 && n.indexOf("auto") < 0);
  }

  function pickAudio(tracks, pref) {
    if (!tracks.length) return -1;
    pref = (pref || "de").toLowerCase();
    if (pref !== "original") {
      const want = tracks.findIndex(function (t) { return (t.lang || "").toLowerCase().split("-")[0] === pref; });
      if (want >= 0) return want;
    }
    const orig = tracks.findIndex(function (t) { return (t.name || "").toLowerCase().indexOf("original") >= 0; });
    if (orig >= 0) return orig;
    const notDubbed = tracks.findIndex(isOriginal);
    if (notDubbed >= 0) return notDubbed;
    const def = tracks.findIndex(function (t) { return t.default; });
    return def >= 0 ? def : 0;
  }

  // Audiospuren für das Menü (doppelte Einträge der zweiten Codec-Gruppe ausgelassen)
  window.nrzAudioTracks = function (id) {
    const p = players[id];
    if (!p || !p.hls) return { tracks: [], current: -1 };
    const seen = {};
    const tracks = [];
    (p.hls.audioTracks || []).forEach(function (t, i) {
      const key = (t.lang || "") + "|" + (t.name || "");
      if (seen[key]) return;
      seen[key] = true;
      tracks.push({ index: i, name: t.name || t.lang || ("Spur " + (i + 1)), lang: t.lang || "" });
    });
    // Originalspur zuerst, dann alphabetisch
    tracks.sort(function (a, b) {
      const oa = a.name.toLowerCase().indexOf("original") >= 0 ? 0 : 1;
      const ob = b.name.toLowerCase().indexOf("original") >= 0 ? 0 : 1;
      return oa - ob || a.name.localeCompare(b.name);
    });
    return { tracks: tracks, current: p.hls.audioTrack };
  };

  window.nrzSetAudio = function (id, index) {
    const p = players[id];
    if (p && p.hls) p.hls.audioTrack = index;
  };

  // Livestream: an die aktuelle Stelle springen
  window.nrzGoLive = function (id) {
    const v = el(id);
    const p = players[id];
    if (!v) return;
    let target = null;
    if (p && p.hls && p.hls.liveSyncPosition) target = p.hls.liveSyncPosition;
    else if (v.seekable && v.seekable.length) target = v.seekable.end(v.seekable.length - 1) - 3;
    if (target != null && isFinite(target)) v.currentTime = target;
    const pr = v.play();
    if (pr && pr.catch) pr.catch(function () {});
  };

  // Livechat: Breite zwischen Player und Chat ziehen (260–720 px), gemerkt
  try {
    const w = parseInt(localStorage.getItem("nrzChatWidth") || "", 10);
    if (w >= 260 && w <= 720) document.documentElement.style.setProperty("--chat-w", w + "px");
  } catch (e) {}
  window.nrzChatResize = function () {
    document.body.style.userSelect = "none";
    document.body.style.cursor = "col-resize";
    const mv = function (ev) {
      const w = Math.max(260, Math.min(720, window.innerWidth - ev.clientX));
      document.documentElement.style.setProperty("--chat-w", w + "px");
    };
    const up = function () {
      removeEventListener("pointermove", mv); removeEventListener("pointerup", up);
      document.body.style.userSelect = ""; document.body.style.cursor = "";
      const w = parseInt(getComputedStyle(document.documentElement).getPropertyValue("--chat-w"), 10);
      try { if (w) localStorage.setItem("nrzChatWidth", String(w)); } catch (e) {}
    };
    addEventListener("pointermove", mv); addEventListener("pointerup", up);
  };

  window.nrzSetLevel = function (id, index) {
    const p = players[id];
    if (p && p.hls) p.hls.currentLevel = index;
  };

  window.nrzScreenshot = function (id, name, format) {
    const v = el(id);
    if (!v || !v.videoWidth) return false;
    const c = document.createElement("canvas");
    c.width = v.videoWidth;
    c.height = v.videoHeight;
    c.getContext("2d").drawImage(v, 0, 0, c.width, c.height);
    const type = format === "jpg" ? "image/jpeg" : "image/png";
    c.toBlob(function (blob) {
      if (!blob) return;
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = name + "." + (format === "jpg" ? "jpg" : "png");
      document.body.appendChild(a);
      a.click();
      setTimeout(function () { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
    }, type, 0.95);
    return true;
  };

  window.nrzPip = function (id) {
    const v = el(id);
    if (!v) return;
    if (document.pictureInPictureElement) {
      document.exitPictureInPicture().catch(function () {});
    } else if (v.requestPictureInPicture) {
      v.requestPictureInPicture().catch(function () {});
    } else if (v.webkitSetPresentationMode) {
      v.webkitSetPresentationMode("picture-in-picture");
    }
  };

  window.nrzFullscreen = function (id) {
    const box = el(id);
    if (!box) return;
    if (document.fullscreenElement || document.webkitFullscreenElement) {
      (document.exitFullscreen || document.webkitExitFullscreen).call(document);
    } else if (box.requestFullscreen) {
      box.requestFullscreen().catch(function () {});
    } else if (box.webkitRequestFullscreen) {
      box.webkitRequestFullscreen();
    } else {
      const v = box.querySelector("video");
      if (v && v.webkitEnterFullscreen) v.webkitEnterFullscreen();
    }
  };

  window.nrzIsFullscreen = function () {
    return !!(document.fullscreenElement || document.webkitFullscreenElement);
  };

  window.nrzDownload = function (name, text, type) {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([text], { type: type || "text/plain" }));
    a.download = name;
    document.body.appendChild(a);
    a.click();
    setTimeout(function () { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
  };

  // Media Session: Titel und Tasten auf dem Sperrbildschirm / Kopfhörer
  window.nrzMediaSession = function (title, artist, art) {
    if (!("mediaSession" in navigator)) return;
    try {
      navigator.mediaSession.metadata = new MediaMetadata({ title: title, artist: artist, artwork: art ? [{ src: art, sizes: "480x360", type: "image/jpeg" }] : [] });
    } catch (e) {}
  };
})();
