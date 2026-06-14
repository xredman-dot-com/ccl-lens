#!/usr/bin/env bash
#
# ccl-lens 打包脚本
#   ./build.sh            打 .app（x86_64 原生 release，最稳）
#   ./build.sh --dmg      同时出 .dmg（用 CI=true 跳过 Finder/AppleScript，避开常见崩溃）
#   ./build.sh --debug    出未压缩的 debug 包，构建更快
#   其余参数原样透传给 `pnpm tauri build`
#
# 产物：src-tauri/target/release/bundle/{macos/*.app, dmg/*.dmg}
#
# 通用包（Intel+M2）需先：装 rustup → rustup target add aarch64-apple-darwin x86_64-apple-darwin
#   然后：./build.sh --target universal-apple-darwin

set -euo pipefail
cd "$(dirname "$0")"
shopt -s nullglob

for bin in pnpm cargo; do
  command -v "$bin" >/dev/null 2>&1 || { echo "✗ 未找到 $bin，请先安装"; exit 1; }
done

# 解析参数：抽出 --dmg，其余透传
WANT_DMG=0
PASS=()
for a in "$@"; do
  case "$a" in
    --dmg) WANT_DMG=1 ;;
    *) PASS+=("$a") ;;
  esac
done

# 清理上次失败残留的临时挂载卷（否则 dmg 会反复失败）
for v in /Volumes/dmg.*; do
  [ -e "$v" ] && hdiutil detach "$v" -force >/dev/null 2>&1 || true
done

if [ ! -d node_modules ]; then
  echo "==> 安装前端依赖"
  pnpm install
fi

if [ "$WANT_DMG" -eq 1 ]; then
  echo "==> 打包 .app + .dmg（首次 release 编译较慢）"
  # CI=true 让 bundle_dmg.sh 跳过 Finder 窗口美化脚本，避免在无 GUI 会话里崩溃
  CI=true pnpm tauri build --bundles app,dmg ${PASS[@]+"${PASS[@]}"}
else
  echo "==> 打包 .app（默认；要 dmg 加 --dmg）"
  pnpm tauri build --bundles app ${PASS[@]+"${PASS[@]}"}
fi

echo
echo "==> 产物："
arts=(
  src-tauri/target/release/bundle/macos/*.app
  src-tauri/target/release/bundle/dmg/*.dmg
  src-tauri/target/debug/bundle/macos/*.app
)
if [ ${#arts[@]} -eq 0 ]; then
  echo "  （未找到产物，检查上面的构建日志）"
else
  for a in "${arts[@]}"; do echo "  $a"; done
fi
echo
echo "提示：未做 Developer ID 签名，首次打开会被 Gatekeeper 拦。"
echo "      右键 → 打开，或：xattr -dr com.apple.quarantine <app>"
