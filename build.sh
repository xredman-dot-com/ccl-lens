#!/usr/bin/env bash
#
# ccl-lens 打包脚本
#   ./build.sh            打 x86_64 原生 release（.app + .dmg）
#   ./build.sh --debug    出未压缩的 debug 包，构建更快
#   其余参数原样透传给 `pnpm tauri build`
#
# 产物位置：src-tauri/target/release/bundle/{macos/*.app, dmg/*.dmg}
#
# 注意：当前机器是 Intel 且无 rustup，只能出 x86_64。
# 要打 Intel+M2 通用包，需先：
#   1) 安装 rustup（curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh）
#   2) rustup target add aarch64-apple-darwin x86_64-apple-darwin
#   3) ./build.sh --target universal-apple-darwin

set -euo pipefail
cd "$(dirname "$0")"

for bin in pnpm cargo; do
  command -v "$bin" >/dev/null 2>&1 || { echo "✗ 未找到 $bin，请先安装"; exit 1; }
done

if [ ! -d node_modules ]; then
  echo "==> 安装前端依赖"
  pnpm install
fi

echo "==> 开始打包（首次 release 编译较慢，请耐心等待）"
pnpm tauri build "$@"

echo
echo "==> 产物："
shopt -s nullglob
arts=(src-tauri/target/release/bundle/macos/*.app src-tauri/target/release/bundle/dmg/*.dmg)
if [ ${#arts[@]} -eq 0 ]; then
  echo "  （未找到 release 产物，若用了 --debug 见 target/debug/bundle）"
else
  for a in "${arts[@]}"; do
    echo "  $a"
  done
fi
echo
echo "提示：未做 Developer ID 签名，首次打开会被 Gatekeeper 拦，"
echo "      右键 → 打开，或 xattr -dr com.apple.quarantine <app> 解除。"
