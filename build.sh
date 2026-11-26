#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
用法: ./build.sh [--force] [--no-open]

构建 Apple Silicon 版 RemoteHelper.app 到 dist/。
  -f, --force    覆盖已存在的 dist/RemoteHelper.app
      --no-open  只构建，不启动应用
  -h, --help     显示此帮助
USAGE
}

force=false
open_app=true
for arg in "$@"; do
  case "${arg}" in
    -f|--force) force=true ;;
    --no-open) open_app=false ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "未知参数：${arg}" >&2
      usage >&2
      exit 2
      ;;
  esac
done

host_arch=$(uname -m)
if [[ "${host_arch}" != "arm64" ]]; then
  echo "此脚本只构建 Apple Silicon (arm64) 版；当前主机是 ${host_arch}。" >&2
  exit 1
fi

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd -- "$repo_root"
app="$repo_root/dist/RemoteHelper.app"

if [[ -e "${app}" && "$force" != true ]]; then
  echo "已存在 ${app}；请先移走旧应用，或使用 --force 覆盖。" >&2
  exit 1
fi

deployment_target=15.0
export MACOSX_DEPLOYMENT_TARGET="$deployment_target"

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repo_root/crates/app/Cargo.toml" | head -n 1)
if [[ -z "$version" ]]; then
  echo "无法从 crates/app/Cargo.toml 读取版本号。" >&2
  exit 1
fi
bundle_version="${version%%+*}"
bundle_version="${bundle_version%%-*}"

printf '构建 RemoteHelper %s（macOS %s+）...\n' "$version" "$deployment_target"

# 固定 target 目录，避免 ~/.cargo/config.toml 里的 build.target-dir 改变产物位置。
unset CARGO_BUILD_TARGET
cargo build --release -p App --target-dir "$repo_root/target"

binary="$repo_root/target/release/App"
if [[ ! -x "${binary}" ]]; then
  echo "未找到构建产物：${binary}" >&2
  echo "如果 Cargo 配置了 build.target，请取消后再运行。" >&2
  exit 1
fi
if ! file "${binary}" | grep -q 'arm64'; then
  echo "构建产物不是 arm64：$(file -b "${binary}")" >&2
  exit 1
fi

staging=""
cleanup() {
  if [[ -n "${staging:-}" ]]; then
    rm -rf "$staging"
  fi
  return 0
}
trap cleanup EXIT

dist="$repo_root/dist"
mkdir -p "$dist"
staging=$(mktemp -d "$dist/.RemoteHelper.XXXXXX")
bundle="$staging/RemoteHelper.app"
iconset="$staging/RemoteHelper.iconset"

mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" "$iconset"
cp "${binary}" "$bundle/Contents/MacOS/RemoteHelper"
chmod 755 "$bundle/Contents/MacOS/RemoteHelper"

icon_source="$repo_root/crates/app/assets/icon.png"
icon_width=$(sips -g pixelWidth "${icon_source}" | awk '/pixelWidth/ {print $2}')
if [[ -n "${icon_width:-}" && "$icon_width" -lt 1024 ]]; then
  echo "提示：${icon_source} 只有 ${icon_width}px，512x512@2x 会由放大生成；建议提供 1024px 源。" >&2
fi

for size in 16 32 128 256 512; do
  retina=$((size * 2))
  sips -z "$size" "$size" "${icon_source}" \
    --out "$iconset/icon_${size}x${size}.png" >/dev/null
  sips -z "$retina" "$retina" "${icon_source}" \
    --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/RemoteHelper.icns"

plist="$bundle/Contents/Info.plist"
cat > "$plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleExecutable</key><string>RemoteHelper</string>
  <key>CFBundleIdentifier</key><string>dev.hughsean.remotehelper</string>
  <key>CFBundleName</key><string>RemoteHelper</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>RemoteHelper.icns</string>
  <key>CFBundleShortVersionString</key><string>0.0.0</string>
  <key>CFBundleVersion</key><string>0.0.0</string>
  <key>LSMinimumSystemVersion</key><string>0.0.0</string>
  <key>NSLocalNetworkUsageDescription</key><string>RemoteHelper 需要访问本地网络，以连接局域网中的远程主机。</string>
</dict>
</plist>
PLIST

plutil -replace CFBundleShortVersionString -string "$bundle_version" "$plist"
plutil -replace CFBundleVersion -string "$bundle_version" "$plist"
plutil -replace LSMinimumSystemVersion -string "$deployment_target" "$plist"
plutil -lint "$plist"

codesign --force --sign - "$bundle"
codesign --verify --strict "$bundle"

if [[ -e "${app}" ]]; then
  rm -rf "${app}"
fi
mv "$bundle" "${app}"
rm -rf "$staging"
staging=""

if [[ "$open_app" == true ]]; then
  if ! open "${app}"; then
    echo "无法自动启动应用，请手动打开：${app}" >&2
    exit 1
  fi
  echo "已启动 ${app}"
else
  echo "已生成 ${app}"
fi
