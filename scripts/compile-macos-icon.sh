#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT_DIR="$PROJECT_ROOT/src-tauri/generated-icons"
ICON_WORK="$(mktemp -d "${TMPDIR:-/tmp}/rayburst-icon.XXXXXX")"
trap 'rm -rf "$ICON_WORK"' EXIT

# Compile through Apple's tool with an explicit stdin. Tauri's Node CLI can
# inherit a closed descriptor in its built-in actool invocation (tauri#15991).
xcrun actool "$PROJECT_ROOT/src-tauri/icons/Rayburst.icon" \
  --compile "$ICON_WORK" \
  --output-format human-readable-text \
  --notices --warnings --errors \
  --output-partial-info-plist "$ICON_WORK/partial.plist" \
  --app-icon Rayburst \
  --include-all-app-icons \
  --enable-on-demand-resources NO \
  --development-region en \
  --target-device mac \
  --minimum-deployment-target 26.0 \
  --platform macosx </dev/null

if [[ ! -s "$ICON_WORK/Assets.car" ]]; then
  echo "Apple actool did not produce Assets.car; see its diagnostics above." >&2
  exit 1
fi

mkdir -p "$OUTPUT_DIR"
cp "$ICON_WORK/Assets.car" "$OUTPUT_DIR/Assets.car"
