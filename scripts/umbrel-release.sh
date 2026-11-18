#!/bin/sh
# Umbrel-Paket ohne Registry und ohne Login (wie nrzMusic):
#   scripts/umbrel-release.sh 1.0.0
# baut das Programm für linux/amd64 und linux/arm64 (Stufe `dist` des Dockerfiles, mit
# eingebetteter Oberfläche), holt yt-dlp und Deno (Versionen und Prüfsummen aus dem Dockerfile)
# und legt alles als Commit auf den Zweig `umbrel` mit dem Tag `umbrel-<version>`:
#   nrztube-<arch>.gz  yt-dlp-<arch>.gz  deno-<arch>.gz  SHA256SUMS
# Das `pre-start`-Skript der App `nrz-tube` im nrzStore lädt diese Dateien auf dem Gerät von
# http://localhost:8390/nicorenezweers/nrzTube/raw/tag/umbrel-<version>/… (nrzGit).
# Gepusht wird auf den Remote $REMOTE (Standard: origin); Docker muss laufen.
set -eu
cd "$(dirname "$0")/.."

version="${1:-}"
if [ -z "$version" ]; then
  echo "Aufruf: $0 <version>   (z. B. $0 1.0.0)" >&2
  exit 1
fi
remote="${REMOTE:-origin}"
tag="umbrel-$version"
out="dist/umbrel/$version"
builder="${BUILDER:-nrztube-builder}"

arg() { sed -n "s/^ARG $1=//p" Dockerfile | head -1; }
ytdlp_version="$(arg YTDLP_VERSION)"
deno_version="$(arg DENO_VERSION)"

if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "Tag $tag gibt es schon – neue Version wählen." >&2
  exit 1
fi

rm -rf "$out"
mkdir -p "$out/bin" "$out/dl"

echo "==> Programm (linux/amd64, linux/arm64)"
if ! docker buildx inspect "$builder" >/dev/null 2>&1; then
  docker buildx create --name "$builder" --driver docker-container >/dev/null
fi
docker buildx build --builder "$builder" --platform linux/amd64,linux/arm64 --target dist \
  --output "type=local,dest=$out/bin" .

echo "==> yt-dlp $ytdlp_version, Deno $deno_version"
for arch in amd64 arm64; do
  case "$arch" in
    amd64) ytdlp=yt-dlp_linux; ytdlp_sha="$(arg YTDLP_SHA256_AMD64)"; deno=x86_64; deno_sha="$(arg DENO_SHA256_AMD64)" ;;
    arm64) ytdlp=yt-dlp_linux_aarch64; ytdlp_sha="$(arg YTDLP_SHA256_ARM64)"; deno=aarch64; deno_sha="$(arg DENO_SHA256_ARM64)" ;;
  esac
  curl -fsSL --retry 3 -o "$out/dl/yt-dlp-$arch" \
    "https://github.com/yt-dlp/yt-dlp/releases/download/$ytdlp_version/$ytdlp"
  curl -fsSL --retry 3 -o "$out/dl/deno-$arch.zip" \
    "https://github.com/denoland/deno/releases/download/v$deno_version/deno-$deno-unknown-linux-gnu.zip"
  printf '%s  %s\n%s  %s\n' "$ytdlp_sha" "$out/dl/yt-dlp-$arch" "$deno_sha" "$out/dl/deno-$arch.zip" | shasum -a 256 -c -
  unzip -q -o "$out/dl/deno-$arch.zip" deno -d "$out/dl/deno-$arch"

  gzip -9 -c "$out/bin/linux_$arch/nrztube" > "$out/nrztube-$arch.gz"
  gzip -9 -c "$out/dl/yt-dlp-$arch" > "$out/yt-dlp-$arch.gz"
  gzip -9 -c "$out/dl/deno-$arch/deno" > "$out/deno-$arch.gz"
done
files="nrztube-amd64.gz nrztube-arm64.gz yt-dlp-amd64.gz yt-dlp-arm64.gz deno-amd64.gz deno-arm64.gz"
(cd "$out" && shasum -a 256 $files > SHA256SUMS)

echo "==> Zweig umbrel, Tag $tag"
worktree="$(mktemp -d -t nrztube-umbrel.XXXXXX)"
rmdir "$worktree"
trap 'git worktree remove --force "$worktree" >/dev/null 2>&1 || true' EXIT
git fetch -q "$remote" umbrel 2>/dev/null || true
if git rev-parse -q --verify "refs/remotes/$remote/umbrel" >/dev/null; then
  git worktree add -q -B umbrel "$worktree" "$remote/umbrel"
else
  git worktree add -q --orphan -b umbrel "$worktree"
fi
rm -f "$worktree"/*.gz "$worktree"/SHA256SUMS
for f in $files SHA256SUMS; do cp "$out/$f" "$worktree/"; done
cat > "$worktree/README.md" <<EOF
# nrzTube – Umbrel-Paket

Dieser Zweig enthält nur fertige Dateien für die Umbrel-App \`nrz-tube\` (nrzStore), damit sie
ohne Container-Registry und ohne Login installiert werden kann. Der Quellcode liegt auf \`main\`.

| Datei | Inhalt |
|---|---|
| \`nrztube-<arch>.gz\` | Server mit eingebetteter Oberfläche (glibc), amd64/arm64 |
| \`yt-dlp-<arch>.gz\` | yt-dlp $ytdlp_version (eigenständig, Unlicense) |
| \`deno-<arch>.gz\` | Deno $deno_version (MIT), JavaScript für yt-dlp |
| \`SHA256SUMS\` | Prüfsummen |

Stand: nrzTube $version, gebaut aus \`$(git rev-parse --short HEAD)\` mit \`scripts/umbrel-release.sh $version\`.
EOF
git -C "$worktree" add -A
git -C "$worktree" -c user.name="$(git config user.name || echo nrz)" -c user.email="$(git config user.email || echo nrz@localhost)" \
  commit -q -m "Umbrel-Paket nrzTube $version"
git -C "$worktree" tag -a "$tag" -m "Umbrel-Paket nrzTube $version"
git push -q "$remote" umbrel "refs/tags/$tag"
echo "Fertig: $tag auf $remote (Zweig umbrel)."
cat "$out/SHA256SUMS"
