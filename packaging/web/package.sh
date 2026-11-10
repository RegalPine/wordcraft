#!/usr/bin/env bash
# Build the browser version and zip it:  $DIST/wordcraft-web-<version>.zip
#
# Usage: packaging/web/package.sh [--skip-build]
#
# Needs: trunk (brew install trunk / cargo install trunk --locked) and the wasm32-unknown-unknown
# target. The zip holds a self-contained static site in wordcraft-web-<version>/ that works
# from any URL path and inside an <iframe>. Hosting notes: packaging/web/README.md.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/web"

if [[ "${1:-}" != "--skip-build" ]]; then
  command -v trunk >/dev/null || { echo "error: trunk not found (cargo install trunk --locked)" >&2; exit 1; }
  (cd "$ROOT/apps/wordcraft-web" && trunk build --release)
fi

SITE="$ROOT/dist/web"
[[ -f "$SITE/index.html" ]] || { echo "error: $SITE/index.html missing; run without --skip-build" >&2; exit 1; }
# Paths must be relative so the site works under any prefix (public_url = "./" in Trunk.toml).
if grep -Eq '(src|href)="/[^/]' "$SITE/index.html"; then
  echo "error: $SITE/index.html has root-absolute URLs; it would break when served from a sub-path" >&2
  exit 1
fi

NAME="wordcraft-web-$VERSION"
WORK="$CARGO_TARGET_DIR/web-package"
rm -rf "$WORK"
mkdir -p "$WORK/$NAME"
cp -R "$SITE/." "$WORK/$NAME/"
# CJK faces the page fetches on demand. The wasm embeds none (crates/fonts/build.rs keeps only the
# small Arabic and Georgian interface faces, and every visitor pays for what is embedded), and a
# browser has no font folders to read, so Chinese, Japanese and Korean text would be boxes. The app
# asks once per writing system and reads this list (apps/wordcraft-web/src/cjk.rs); a site without
# the folder simply has no CJK until the user adds a font, which is what it did before.
CJK_MAX_BYTES=$((48 << 20)) # the same size the web build refuses to fetch, so nothing useless ships
CJK_DIR="fonts/cjk/v$VERSION"
if [[ -n "${CRAFT_FONTS_DIR:-}" && -f "$CRAFT_FONTS_DIR/fonts/manifest.txt" ]]; then
  mkdir -p "$WORK/$NAME/$CJK_DIR"
  index="$WORK/$NAME/$CJK_DIR/index.json"
  printf '{"files":[' >"$index"
  count=0
  # One entry per writing system, its best usable face: a face for that system alone beats one
  # shared with another, and Regular beats a bold or light weight (the order is the first field, so
  # a face that turns out unusable falls through to the next candidate). The ids are the ones
  # CjkSystem::from_id reads.
  taken=""
  while IFS=$'\t' read -r _rank system family style file; do
    [[ -n "$system" ]] || continue
    case " $taken " in *" $system "*) continue ;; esac
    src="$CRAFT_FONTS_DIR/$file"
    if [[ ! -f "$src" ]]; then warn "craft-fonts lists $file, which is not there; skipped"; continue; fi
    # A name that needs escaping is a name nobody wrote here; leave it out rather than emit a
    # list the app can't read.
    if [[ "$family$style" == *\\* || "$family$style" == *\"* ]]; then warn "$family ($style) has a name this list cannot hold; skipped"; continue; fi
    rel="${file#fonts/}"
    if [[ "$rel" == /* || "$rel" == *..* ]]; then warn "craft-fonts lists an unusable path: $file; skipped"; continue; fi
    bytes=$(wc -c <"$src" | tr -d '[:space:]')
    [[ "$bytes" =~ ^[0-9]+$ ]] || { warn "$file: no size to check; skipped"; continue; }
    if ((bytes > CJK_MAX_BYTES)); then
      warn "$family ($style) is $bytes bytes, more than the web build fetches; skipped"
      continue
    fi
    dest="$WORK/$NAME/$CJK_DIR/$rel"
    mkdir -p "$(dirname "$dest")"
    cp "$src" "$dest"
    if ((count > 0)); then printf ',' >>"$index"; fi
    printf '{"system":"%s","name":"%s","style":"%s","file":"%s/%s","bytes":%s}' \
      "$system" "$family" "$style" "$CJK_DIR" "$rel" "$bytes" >>"$index"
    count=$((count + 1))
    taken="$taken $system"
  done < <(awk '
    BEGIN { want[1] = "Hans"; want[2] = "Hant"; want[3] = "Jpan"; want[4] = "Kore" }
    {
      line = $0
      gsub(/^[ \t]+|[ \t]+$/, "", line)
      if (line == "" || line ~ /^#/) next
      # The separator is a regex: the pipe is escaped, or every space would split the line.
      n = split(line, f, " \\| ")
      if (n < 4) next
      gsub(/[ \t]+/, "", f[4])
      k = split(f[4], s, ",")
      for (i = 1; i <= 4; i++) {
        hit = 0
        for (j = 1; j <= k; j++) if (s[j] == want[i]) hit = 1
        if (!hit) continue
        print (k == 1 ? 0 : 10) + (f[2] == "Regular" ? 0 : 1) "\t" tolower(want[i]) "\t" f[1] "\t" f[2] "\t" f[3]
      }
    }
  ' "$CRAFT_FONTS_DIR/fonts/manifest.txt" | sort -t "$(printf '\t')" -k2,2 -k1,1n)
  printf ']}\n' >>"$index"
  if ((count == 0)); then
    warn "CRAFT_FONTS_DIR has no CJK face the web build can ship; the site serves no fonts/cjk"
    rm -rf "$WORK/$NAME/fonts/cjk"
  else
    echo "shipped $count CJK face(s) for the page to fetch in $CJK_DIR"
  fi
fi
# Sample server configs (MIME type, caching, compression); harmless where unused.
cp "$HERE/_headers" "$HERE/.htaccess" "$WORK/$NAME/"
cp "$HERE/README.md" "$WORK/$NAME/HOSTING.md"
copy_docs "$WORK/$NAME"
rm -f "$DIST/$NAME.zip"
(cd "$WORK" && zip -qr9 "$DIST/$NAME.zip" "$NAME")
echo "wrote $DIST/$NAME.zip"
ls -lh "$DIST/$NAME.zip"
