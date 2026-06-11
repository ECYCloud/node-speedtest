#!/usr/bin/env bash
# WSL 内一次性准备 Tauri RPM 验证用的工作目录。仅用于本地端到端测试,不参与发布构建。
set -euo pipefail

SRC=/mnt/d/Offices/stairspeedtest-reborn-master/separated/desktop
DST=$HOME/tauri-rpm-test/desktop

mkdir -p "$HOME/tauri-rpm-test"

echo "== rsync (excluding node_modules / target / gen / dist) =="
rsync -a --delete \
  --exclude=node_modules \
  --exclude=src-tauri/target \
  --exclude=src-tauri/gen \
  --exclude=dist \
  --exclude=build.log \
  "$SRC/" "$DST/"

echo "== engine placeholders =="
cd "$DST/src-tauri"
rm -rf engine
mkdir -p engine/tools/clients engine/tools/misc
for f in stairspeedtest pref.ini config.yaml sub_link.txt \
         tools/clients/mihomo \
         tools/misc/SourceHanSansCN-Medium.otf \
         tools/misc/TwemojiFlat.ttf; do
  echo placeholder > "engine/$f"
done
chmod +x engine/stairspeedtest engine/tools/clients/mihomo
ls -la engine/

echo "== work dir size =="
du -sh "$DST"
