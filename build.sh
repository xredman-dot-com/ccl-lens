#!/usr/bin/env bash
#
# ccl-lens 打包脚本
#   ./build.sh            打 .app（x86_64 原生 release，最稳）
#   ./build.sh --dmg      同时出 .dmg（用 CI=true 跳过 Finder/AppleScript，避开常见崩溃）
#   ./build.sh --windows  交叉编译出 Windows 安装包 .exe（NSIS，用 cargo-xwin）
#   ./build.sh --debug    出未压缩的 debug 包，构建更快
#   其余参数原样透传给 `pnpm tauri build`
#
# 产物：src-tauri/target/release/bundle/{macos/*.app, dmg/*.dmg}
#       Windows: src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/*.exe
#
# 通用包（Intel+M2）需先：装 rustup → rustup target add aarch64-apple-darwin x86_64-apple-darwin
#   然后：./build.sh --target universal-apple-darwin
#
# --windows 首次使用需先：
#   rustup target add x86_64-pc-windows-msvc
#   cargo install --locked cargo-xwin

set -euo pipefail
cd "$(dirname "$0")"
shopt -s nullglob

ROOT_DIR="$(pwd -P)"
TARGET_DIR="src-tauri/target"
TARGET_MARKER="$TARGET_DIR/.ccl-lens-workspace-root"

usage() {
  cat <<'EOF'
ccl-lens 打包脚本

用法:
  ./build.sh [选项] [-- <透传给 pnpm tauri build 的参数>]

选项:
  (无)            打 .app（x86_64 原生 release，最稳）
  --dmg           同时出 .dmg（CI=true 跳过 Finder/AppleScript，避开常见崩溃）
  --windows       交叉编译出 Windows 安装包 .exe（NSIS，用 cargo-xwin）
  --debug         出未压缩的 debug 包，构建更快
  -h, --help      显示本帮助并退出

示例:
  ./build.sh
  ./build.sh --dmg
  ./build.sh --windows
  ./build.sh --target universal-apple-darwin   # 需先装 rustup 并加 aarch64 target

产物:
  src-tauri/target/release/bundle/{macos/*.app, dmg/*.dmg}
  Windows: src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/*.exe

说明:
  - 未做 Developer ID 签名，首次打开会被 Gatekeeper 拦；
    右键 → 打开，或: xattr -dr com.apple.quarantine <app>
  - 通用包(Intel+M2): rustup target add aarch64-apple-darwin x86_64-apple-darwin
    然后 ./build.sh --target universal-apple-darwin
  - --windows 首次需: rustup target add x86_64-pc-windows-msvc
                      cargo install --locked cargo-xwin
EOF
}

# 解析参数：抽出 --help / --dmg / --windows，其余透传
WANT_DMG=0
WANT_WIN=0
PASS=()
for a in "$@"; do
  case "$a" in
    -h|--help) usage; exit 0 ;;
    --dmg) WANT_DMG=1 ;;
    --windows) WANT_WIN=1 ;;
    *) PASS+=("$a") ;;
  esac
done

for bin in pnpm cargo; do
  command -v "$bin" >/dev/null 2>&1 || { echo "✗ 未找到 $bin，请先安装"; exit 1; }
done

WIN_TARGET="x86_64-pc-windows-msvc"
if [ "$WANT_WIN" -eq 1 ]; then
  command -v rustup >/dev/null 2>&1 || { echo "✗ 未找到 rustup，交叉编译 Windows 需要它"; exit 1; }
  if ! rustup target list --installed 2>/dev/null | grep -qx "$WIN_TARGET"; then
    echo "==> 安装 Rust target $WIN_TARGET"
    rustup target add "$WIN_TARGET"
  fi
  if ! cargo xwin --help >/dev/null 2>&1; then
    echo "==> 安装 cargo-xwin（提供 MSVC 工具链 + Windows SDK）"
    cargo install --locked cargo-xwin
  fi
fi

# Tauri/Cargo target 目录会缓存 build script 的 OUT_DIR 和权限清单路径。
# 如果项目目录被移动或复制，旧绝对路径会导致 "failed to read plugin permissions"。
if [ -d "$TARGET_DIR" ]; then
  if [ -f "$TARGET_MARKER" ] && [ "$(cat "$TARGET_MARKER")" != "$ROOT_DIR" ]; then
    echo "==> 检测到 target 来自其它目录，清理 Rust 构建缓存"
    rm -rf "$TARGET_DIR"
  elif [ ! -f "$TARGET_MARKER" ] && rg -q "/code/claude_helper/ccl-lens/src-tauri" "$TARGET_DIR" --hidden --no-ignore 2>/dev/null; then
    echo "==> 检测到旧 Rust 构建缓存，清理 target"
    rm -rf "$TARGET_DIR"
  fi
fi
mkdir -p "$TARGET_DIR"
printf '%s\n' "$ROOT_DIR" > "$TARGET_MARKER"

# 清理上次失败残留的临时挂载卷（否则 dmg 会反复失败）
for v in /Volumes/dmg.*; do
  [ -e "$v" ] && hdiutil detach "$v" -force >/dev/null 2>&1 || true
done

# 清掉旧的 bundle 产物，避免误启动到内嵌过期前端的 .app（曾导致 webview 资源加载 panic 崩溃）
rm -rf src-tauri/target/release/bundle src-tauri/target/debug/bundle 2>/dev/null || true

if [ ! -d node_modules ]; then
  echo "==> 安装前端依赖"
  pnpm install
fi

if [ "$WANT_WIN" -eq 1 ]; then
  echo "==> 交叉编译 Windows .exe 安装包（NSIS，cargo-xwin；首次编译较慢）"
  # 用 cargo-xwin 作为 runner，目标 MSVC triple；跨平台只能出 NSIS（MSI 需 Windows 上的 WiX）
  pnpm tauri build --runner cargo-xwin --target "$WIN_TARGET" --bundles nsis ${PASS[@]+"${PASS[@]}"}
elif [ "$WANT_DMG" -eq 1 ]; then
  echo "==> 打包 .app + .dmg（首次 release 编译较慢）"
  # CI=true 让 bundle_dmg.sh 跳过 Finder 窗口美化脚本，避免在无 GUI 会话里崩溃
  CI=true pnpm tauri build --bundles app,dmg ${PASS[@]+"${PASS[@]}"}
else
  echo "==> 打包 .app（默认；要 dmg 加 --dmg，要 Windows 加 --windows）"
  pnpm tauri build --bundles app ${PASS[@]+"${PASS[@]}"}
fi

echo
echo "==> 产物："
arts=(
  src-tauri/target/release/bundle/macos/*.app
  src-tauri/target/release/bundle/dmg/*.dmg
  src-tauri/target/debug/bundle/macos/*.app
  "src-tauri/target/$WIN_TARGET/release/bundle/nsis/"*.exe
)
if [ ${#arts[@]} -eq 0 ]; then
  echo "  （未找到产物，检查上面的构建日志）"
else
  for a in "${arts[@]}"; do echo "  $a"; done
fi
echo
if [ "$WANT_WIN" -eq 1 ]; then
  echo "提示：Windows 包未做代码签名，首次运行会弹 SmartScreen，点「更多信息 → 仍要运行」。"
else
  echo "提示：未做 Developer ID 签名，首次打开会被 Gatekeeper 拦。"
  echo "      右键 → 打开，或：xattr -dr com.apple.quarantine <app>"
fi
