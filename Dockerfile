# syntax=docker/dockerfile:1.10
# nrzTube – Multi-Stage-Build für amd64 und arm64: Oberfläche (Dioxus/WebAssembly) und Programm
# (Rust), dazu yt-dlp und Deno für die Video-Streams. Braucht nur Docker – kein Rust auf dem Rechner.
#   docker build -t nrztube .
#   docker run -p 8455:8455 -v nrztube-data:/data nrztube
# Beide Architekturen (Rust wird quer-kompiliert, nicht emuliert):
#   docker buildx build --platform linux/amd64,linux/arm64 -t <registry>/nrz-tube:<version> --push .
# Nur das Programm (für die Release-Assets, landet in dist/linux_<arch>/):
#   docker buildx build --platform linux/amd64,linux/arm64 --target dist --output type=local,dest=dist .

ARG DEBIAN=debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8

# Oberfläche auf Trixie: die vorgefertigten wasm-bindgen-Programme von dx brauchen glibc 2.38.
ARG RUST_UI=rust:1.98-trixie@sha256:a8a5f0a1e5fe7dfe1d352591e4a1c7dd2c08fd70475cae872cf3458ba0df0546

ARG RUST=rust:1.98-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e

# ---------------------------------------------------------------- Oberfläche
# WebAssembly ist überall gleich – einmal auf dem Build-Rechner bauen, für alle Architekturen.
FROM --platform=$BUILDPLATFORM ${RUST_UI} AS ui
RUN rustup target add wasm32-unknown-unknown
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    cargo install dioxus-cli --version 0.7.10 --locked --root /opt/dx
WORKDIR /src
# Die Oberfläche hängt an ../types (gemeinsame Typen der JSON-API)
COPY types types
COPY ui ui
# `dx bundle` räumt seinen Zwischenordner nicht auf – vorher leeren, sonst kommen alte
# WASM-Dateien aus dem Cache mit.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/ui/target,id=nrztube-ui-target \
    cd ui && rm -rf target/dx/nrztube-ui/release/web/public \
 && PATH=/opt/dx/bin:$PATH dx bundle --web --release --out-dir /tmp/ui \
 && mkdir -p /out && cp -r /tmp/ui/public /out/public && test -f /out/public/index.html

# ---------------------------------------------------------------- Programm
FROM --platform=$BUILDPLATFORM ${RUST} AS build
ARG TARGETARCH
ARG BUILDARCH
# Querübersetzer, wenn Ziel und Build-Rechner verschieden sind (SQLite und ring sind C-Code
# und werden mit dem C-Compiler des Ziels gebaut).
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) t=x86_64-unknown-linux-gnu; p=x86_64-linux-gnu; pkg="gcc-x86-64-linux-gnu libc6-dev-amd64-cross" ;; \
      arm64) t=aarch64-unknown-linux-gnu; p=aarch64-linux-gnu; pkg="gcc-aarch64-linux-gnu libc6-dev-arm64-cross" ;; \
      *) echo "Architektur $TARGETARCH nicht unterstützt" >&2; exit 1 ;; \
    esac; \
    echo "$t" > /rust-target; \
    if [ "$TARGETARCH" != "$BUILDARCH" ]; then \
      apt-get update && apt-get install -y --no-install-recommends $pkg && rm -rf /var/lib/apt/lists/*; \
      rustup target add "$t"; \
      printf '[target.%s]\nlinker = "%s-gcc"\n' "$t" "$p" >> /usr/local/cargo/config.toml; \
      u=$(echo "$t" | tr - _); \
      printf 'CC_%s=%s-gcc\nAR_%s=%s-ar\n' "$u" "$p" "$u" "$p" > /cross-env; \
    else \
      touch /cross-env; \
    fi
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY types types
COPY src src
# Oberfläche aus der Stufe oben; build.rs bettet sie ins Programm ein (Text und WebAssembly
# gzip) – der Server braucht dafür keine Dateien mehr.
COPY --from=ui /out/public ui/dist/public
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,id=nrztube-target-$TARGETARCH \
    env $(cat /cross-env) cargo build --release --locked --target "$(cat /rust-target)" \
 && cp "target/$(cat /rust-target)/release/nrztube" /nrztube

# ---------------------------------------------------------------- Programm allein
# Für die Release-Assets: nrztube-<version>-linux-<arch>
FROM scratch AS dist
COPY --from=build /nrztube /nrztube

# ---------------------------------------------------------------- yt-dlp und Deno
# yt-dlp als eigenständiges Programm (bringt Python und die YouTube-Skripte yt-dlp-ejs schon mit)
# und Deno, die JavaScript-Laufzeit, die yt-dlp für YouTube braucht (yt-dlp sucht `deno` im PATH).
# Geladen und geprüft auf dem Build-Rechner; die Zielarchitektur bestimmt nur, welche Datei.
# Neue Version: Version und beide Prüfsummen zusammen ändern – yt-dlp: SHA2-256SUMS des Releases,
# Deno: deno-<arch>-unknown-linux-gnu.zip.sha256sum des Releases.
# nrzTube kopiert yt-dlp nach /data/tools/ und hält die Kopie dort selbst aktuell (`yt-dlp -U`);
# die Datei hier ist nur der Ausgangsstand.
FROM --platform=$BUILDPLATFORM ${DEBIAN} AS tools
ARG TARGETARCH
ARG YTDLP_VERSION=2026.10.26
ARG YTDLP_SHA256_AMD64=767ba9dce7052aa196f8b12652f433d76de128ba168966fa84b6394d5e6c6795
ARG YTDLP_SHA256_ARM64=e87e9533e89fe5889abaa21f5114dfe7756d6aa6219b83551add04d48b18e483
ARG DENO_VERSION=2.9.8
ARG DENO_SHA256_AMD64=e890975bc2549807bbcc0b584ffafda72cc7c93349dd99c80483dd784f6c4609
ARG DENO_SHA256_ARM64=d49596c20b9788cf8f5d8ddf644103d4bde56ed239753f0bdb92ba9361d51905
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl unzip \
 && rm -rf /var/lib/apt/lists/*
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) ytdlp=yt-dlp_linux;         ytdlp_sha=$YTDLP_SHA256_AMD64; deno=x86_64;  deno_sha=$DENO_SHA256_AMD64 ;; \
      arm64) ytdlp=yt-dlp_linux_aarch64; ytdlp_sha=$YTDLP_SHA256_ARM64; deno=aarch64; deno_sha=$DENO_SHA256_ARM64 ;; \
      *) echo "Architektur $TARGETARCH nicht unterstützt" >&2; exit 1 ;; \
    esac; \
    mkdir -p /dl /tools; \
    cd /dl; \
    curl -fsSL --retry 3 -o yt-dlp \
      "https://github.com/yt-dlp/yt-dlp/releases/download/$YTDLP_VERSION/$ytdlp"; \
    curl -fsSL --retry 3 -o deno.zip \
      "https://github.com/denoland/deno/releases/download/v$DENO_VERSION/deno-$deno-unknown-linux-gnu.zip"; \
    printf '%s  yt-dlp\n%s  deno.zip\n' "$ytdlp_sha" "$deno_sha" | sha256sum -c -; \
    unzip -q deno.zip deno -d /tools; \
    install -m 0755 yt-dlp /tools/yt-dlp; \
    chmod 0755 /tools/deno; \
    cd /; \
    rm -rf /dl

# ---------------------------------------------------------------- Laufzeit
FROM ${DEBIAN} AS runtime
# ca-certificates für HTTPS zu YouTube, SponsorBlock und DeArrow. Benutzer 1000 wie unter umbrelOS.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 1000 --user-group --home-dir /data --shell /usr/sbin/nologin nrztube \
 && mkdir -p /data && chown 1000:1000 /data
COPY --from=tools /tools/yt-dlp /tools/deno /usr/local/bin/
COPY --from=build /nrztube /usr/local/bin/nrztube
WORKDIR /data
ENV NRZTUBE_DATA_DIR=/data \
    NRZTUBE_ADDRESS=0.0.0.0 \
    NRZTUBE_PORT=8455 \
    NRZTUBE_YTDLP=/usr/local/bin/yt-dlp \
    DENO_DIR=/tmp/deno \
    DENO_NO_UPDATE_CHECK=1 \
    NRZTUBE_LOG=info
USER 1000:1000
VOLUME ["/data"]
EXPOSE 8455
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD ["nrztube", "healthcheck"]
STOPSIGNAL SIGTERM
CMD ["nrztube"]
