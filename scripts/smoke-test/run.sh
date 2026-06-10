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

# Windows git bash 默认会把以 / 开头的 cli 参数转成 Windows 路径(/web 变 C:/Program Files/Git/web),
# 引擎收不到 /web 标志就退回 CLI 交互模式,然后因 stdin EOF 立即结束。
# MSYS_NO_PATHCONV=1 + MSYS2_ARG_CONV_EXCL=* 同时禁用 cygwin 与 MSYS2 两套转换。
# 这两个变量在 Linux/macOS 上不会被读到,设了也无害。
export MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL='*'

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
  # Windows git bash 没 pkill,用 taskkill 兜底
  if command -v pkill >/dev/null 2>&1; then
    pkill -f mihomo 2>/dev/null || true
  elif command -v taskkill >/dev/null 2>&1; then
    taskkill //F //IM mihomo.exe 2>/dev/null || true
  fi
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
# 超时给 240s:Windows runner 在某些时段 TLS 协商较慢,要给 mihomo 充分时间
# 拉订阅 + 同步起 mihomo 子进程并写出配置。
"$PY" -c "import json,os,sys; sys.stdout.write(json.dumps({'url': os.environ['SUB_URL']}))" \
  | curl -fsS --max-time 240 -X POST "${BASE}/readsubscriptions" \
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
# /start 必须把要测的节点列表通过 configs 字段传回,否则 targetNodes 是空、
# batchTest 立即结束。configs 元素结构与 /readsubscriptions 返回的一致。
# server_port 是 Int,但 ssrspeed_regenerate_node_list 用 stoi 解析,Int / String 都能吃。
"$PY" -c "
import json
configs = json.load(open('sub_resp.json'))
print(json.dumps({
    'testMode': 'TCP_PING',
    'sortMethod': 'none',
    'group': '',
    'configs': configs
}))" > start_body.json
curl -fsS --max-time 10 -X POST "${BASE}/start" \
  -H 'Content-Type: application/json' \
  --data-binary @start_body.json \
  || { echo "FAIL 阶段 4: /start 调用失败"; exit 4; }
echo ""
sleep 2  # 让测速线程把 start_flag 翻成 true,避免下面循环立刻看到 stopped 而误判完成

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
# 阶段 5: 引擎在该架构上能跑完整轮测速(硬性指标)+ 节点连通性(软指标)
#
# 设计:smoke test 的核心目的是验证"程序在这架构 CPU 上能正常运行",而不是
# 验证 GitHub runner 的网络出口是否能 reach 订阅的节点。因此:
#   - 已测节点数(results.length)接近 NODES → 证明 mihomo 协议栈在该架构完整工作
#     (中途崩溃/卡死会让 results 远小于 NODES)
#   - 至少 1 节点 ping>0 → 网络出口能 reach 该订阅节点(网络可达性)
#                          失败仅打 WARN,因为这取决于 runner IP 段而非架构
echo "::group::阶段 5 - 引擎完整性 + 节点连通性"
curl -fsS --max-time 10 "${BASE}/getresults" -o results.json
"$PY" - <<PYEOF
import json, sys
d = json.load(open("results.json"))
r = d.get("results", [])
total = len(r)
alive = sum(1 for n in r if float(n.get("ping", 0)) > 0)
nodes = $NODES
ratio = total / nodes if nodes else 0
print(f"[smoke] 节点总数={nodes} 已测完={total} 延迟>0={alive}")
# 引擎完整性: 必须跑完 ≥80% 节点,否则说明引擎在此架构上出问题(崩溃/卡死)
if ratio < 0.8:
    print(f"FAIL: 仅跑完 {ratio:.0%} 节点,引擎可能在此架构上有问题")
    sys.exit(5)
print(f"PASS 引擎完整性: {ratio:.0%} 节点跑完,mihomo 协议栈正常")
if alive < 1:
    print("WARN 节点连通性: 0/{} 节点 ping>0".format(total))
    print("       (可能是 runner 网段无法 reach 订阅节点,与架构无关)")
else:
    print(f"PASS 节点连通性: {alive}/{total} 节点 ping>0")
PYEOF
RC=$?
if [ $RC -ne 0 ]; then
  echo "--- results 片段 ---"
  head -c 3000 results.json
  echo ""
  echo "--- mihomo 日志 ---"
  ls logs/ 2>/dev/null || true
  tail -50 logs/mihomo*.log 2>/dev/null || true
  exit 5
fi
echo "::endgroup::"

echo ""
echo "============================="
echo "[smoke] PASS - 引擎在该架构正常工作"
echo "============================="