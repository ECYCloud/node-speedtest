#!/usr/bin/env bash
# Stair Speedtest 真机冒烟测试(5 阶段验证)。
# 入参 env:
#   EXE     - 引擎相对路径(如 stairspeedtest/stairspeedtest.exe)
#   SUB_URL - 测试用订阅(GitHub Secret 注入,日志已 mask)
#   PY      - Python 3 命令名(默认 python3,Windows 用 python)
# 退出码: 1=进程未起 / 2=getversion 失败 / 3=订阅 0 节点 / 4=测速超时 / 5=无节点连通

set -uo pipefail

echo "::add-mask::${SUB_URL}"

PY="${PY:-python3}"
PORT=10870
BASE="http://127.0.0.1:${PORT}"
DIR=$(dirname "$EXE")
EXE_NAME=$(basename "$EXE")

cd "$DIR"
echo "[smoke] cwd=$(pwd) exe=$EXE_NAME py=$PY"

# 阶段 1: 启动进程 + 等 HTTP 就绪
echo "::group::阶段 1 - 启动 stairspeedtest /web"
nohup "./$EXE_NAME" /web > engine_stdout.log 2> engine_stderr.log &
PID=$!
echo "[smoke] pid=$PID"

cleanup() {
  if kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
    sleep 1
    kill -9 "$PID" 2>/dev/null || true
  fi
  pkill -f mihomo 2>/dev/null || true
}
trap cleanup EXIT

for i in $(seq 1 30); do
  if curl -fsS --max-time 2 "${BASE}/getversion" >/dev/null 2>&1; then
    echo "[smoke] HTTP 服务就绪 (${i}s)"
    break
  fi
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "[smoke] FAIL 阶段 1: 进程已退出"
    echo "--- stdout ---"; cat engine_stdout.log || true
    echo "--- stderr ---"; cat engine_stderr.log || true
    exit 1
  fi
  sleep 1
done
curl -fsS --max-time 2 "${BASE}/getversion" >/dev/null 2>&1 || {
  echo "[smoke] FAIL 阶段 1: HTTP 30s 未就绪"
  cat engine_stderr.log || true
  exit 1
}
echo "::endgroup::"

# 阶段 2: /getversion
echo "::group::阶段 2 - GET /getversion"
VER=$(curl -fsS --max-time 5 "${BASE}/getversion")
echo "[smoke] $VER"
echo "$VER" | grep -q '"main"' || { echo "FAIL 阶段 2"; exit 2; }
echo "::endgroup::"

# 阶段 3: 解析订阅(同步触发 mihomo 内核启动)
echo "::group::阶段 3 - POST /readsubscriptions"
"$PY" -c "import json,os,sys; sys.stdout.write(json.dumps({'url': os.environ['SUB_URL']}))" \
  | curl -fsS --max-time 90 -X POST "${BASE}/readsubscriptions" \
      -H 'Content-Type: application/json' --data-binary @- > sub_resp.json
NODES=$("$PY" -c "import json; print(len(json.load(open('sub_resp.json'))))")
echo "[smoke] 解析节点数: $NODES"
[ "$NODES" -gt 0 ] || {
  echo "FAIL 阶段 3: 0 节点"
  head -c 2000 sub_resp.json
  exit 3
}
echo "::endgroup::"

sleep 3  # mihomo outbound 注册稳态化

# 阶段 4: TCP_PING 测速并轮询 /status 等完成
echo "::group::阶段 4 - POST /start (TCP_PING)"
curl -fsS --max-time 10 -X POST "${BASE}/start" \
  -H 'Content-Type: application/json' \
  --data '{"testMode":"TCP_PING","sortMethod":"none","group":""}' \
  || { echo "FAIL 阶段 4: /start 调用失败"; exit 4; }
echo ""

DEADLINE=$(( $(date +%s) + 600 ))
LAST_DONE=-1
while [ "$(date +%s)" -lt "$DEADLINE" ]; do
  ST=$(curl -fsS --max-time 5 "${BASE}/status" 2>/dev/null || echo "?")
  if [ "$ST" = "stopped" ]; then
    echo "[smoke] 测速完成"
    break
  fi
  curl -fsS --max-time 5 "${BASE}/getresults" -o results.json 2>/dev/null || echo '{}' > results.json
  DONE=$("$PY" -c "import json
try: print(len(json.load(open('results.json')).get('results',[])))
except Exception: print(0)")
  if [ "$DONE" != "$LAST_DONE" ]; then
    echo "[smoke] 已测完: $DONE / $NODES"
    LAST_DONE=$DONE
  fi
  sleep 5
done

ST=$(curl -fsS --max-time 5 "${BASE}/status" 2>/dev/null || echo "?")
[ "$ST" = "stopped" ] || { echo "FAIL 阶段 4: 10 分钟后仍未完成 (status=$ST)"; exit 4; }
echo "::endgroup::"

# 阶段 5: 至少 1 个节点 ping > 0
echo "::group::阶段 5 - 至少 1 个节点连通"
curl -fsS --max-time 10 "${BASE}/getresults" -o results.json
"$PY" - <<'PYEOF'
import json, sys
d = json.load(open("results.json"))
r = d.get("results", [])
total = len(r)
alive = sum(1 for n in r if float(n.get("ping", 0)) > 0)
print(f"[smoke] 节点总数={total} 延迟>0={alive}")
if alive < 1:
    sys.exit(5)
PYEOF
RC=$?
if [ $RC -ne 0 ]; then
  echo "FAIL 阶段 5: 无节点连通"
  echo "--- results 片段 ---"
  head -c 3000 results.json
  echo "--- mihomo 日志 ---"
  ls logs/ 2>/dev/null || true
  tail -50 logs/mihomo*.log 2>/dev/null || true
  exit 5
fi
echo "::endgroup::"

echo ""
echo "============================="
echo "[smoke] PASS - 全 5 阶段通过"
echo "============================="