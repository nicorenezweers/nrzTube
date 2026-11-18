# nrzTube – Umbrel-Paket

Dieser Zweig enthält nur fertige Dateien für die Umbrel-App `nrz-tube` (nrzStore), damit sie
ohne Container-Registry und ohne Login installiert werden kann. Der Quellcode liegt auf `main`.

| Datei | Inhalt |
|---|---|
| `nrztube-<arch>.gz` | Server mit eingebetteter Oberfläche (glibc), amd64/arm64 |
| `yt-dlp-<arch>.gz` | yt-dlp 2026.10.26 (eigenständig, Unlicense) |
| `deno-<arch>.gz` | Deno 2.9.8 (MIT), JavaScript für yt-dlp |
| `SHA256SUMS` | Prüfsummen |

Stand: nrzTube 1.11.1, gebaut aus `80316fe` mit `scripts/umbrel-release.sh 1.11.1`.
