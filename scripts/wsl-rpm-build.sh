#!/usr/bin/env bash
# WSL 内一次性跑通 npm ci + tauri build --bundles rpm,验证 RPM 打包。
# 配套 scripts/wsl-rpm-prep.sh 使用,前置假设:
#   - apt 已装 libwebkit2gtk-4.1-dev / libgtk-3-dev / libsoup-3.0-dev / libayatana-appindicator3-dev / librsvg2-dev / patchelf / rpm
#   - rustup 已装到 ~/.cargo
#   - node 22 已落到 ~/.local/node22
#   - prep 脚本已把项目同步到 ~/tauri-rpm-test/desktop
set -euo pipefail

export PATH="$HOME/.local/node22/bin:$PATH"
# shellcheck source=/dev/null
. "$HOME/.cargo/env"

cd "$HOME/tauri-rpm-test/desktop"

echo "== node $(node --version) / npm $(npm --version) / cargo $(cargo --version) =="

# Node 18 编的 native binding 在 Node 22 下要重编,直接全删重装。
rm -rf node_modules

echo "== npm ci =="
npm ci 2>&1 | tail -15

echo "== tauri build --bundles rpm =="
LOG=/tmp/tauri-rpm-build.log
if npm run tauri -- build --bundles rpm 2>&1 | tee "$LOG"; then
    echo "---BUILD OK---"
else
    echo "---BUILD FAIL---"
    exit 1
fi

echo "== locate rpm =="
RPM=$(find src-tauri/target/release/bundle/rpm -name '*.rpm' | head -n 1 || true)
if [ -z "$RPM" ]; then
    echo "no rpm produced"
    exit 1
fi
echo "rpm: $RPM ($(du -h "$RPM" | cut -f1))"

echo "== rpm -qip =="
rpm -qip "$RPM"

echo "== rpm -qpR =="
rpm -qpR "$RPM"
