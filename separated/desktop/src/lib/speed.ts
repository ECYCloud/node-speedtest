/**
 * 速度计算 — 前端"实时速度"与"最高速度"严格同源同尺度。
 *
 * 数据源:后端 nodeInfo.rawSpeed[20](本文件统称 rawSpeeds)— 每格是测试期间
 * 0.5s 内字节差 ×2 折算的字节/秒瞬时速度，从 index 0 开始按时间顺序写入,
 * 未采集到的位置为 0。后端原样序列化到 JSON 字段 rawSocketSpeed。
 *
 * 设计原则:不依赖后端的 dspeed/dspeedMax 字段做"实时速度"展示，而是前端
 * 直接基于 rawSpeeds 算两个值，保证:
 *   1. 同源:两个值都从同一数组同一套窗口算法得出
 *   2. 同尺度:都用 2s(4 × 0.5s)滑动窗口均值，平掉单格抖动
 *   3. 数学保证 maxSpeed >= curSpeed
 *
 * 这样前端 UI 上"我看到过的实时速度峰值"严格等于"最高速度",不再受 polling
 * 频率影响 — 因为后端把全量 0.5s 采样保留在数组里，前端拿到数组就拿到了
 * 全部历史，自己算就够。
 */

/** 滑动窗口:4 格 × 0.5s = 2s。与后端 src/multithread_test.cpp::perform_test
    里的 kPeakWindow 同值，保证前后端窗口尺度一致。 */
const SLIDING_WINDOW = 4;

/** 当前实时速度(字节/秒):取最近 SLIDING_WINDOW 格非零采样的均值。
    全为零(刚启动 / 已停止 / 节点未测速)时返回 0,前端按"--"占位展示。 */
export function computeCurSpeed(rawSpeeds: readonly number[] | undefined): number {
  if (!rawSpeeds || rawSpeeds.length === 0) return 0;
  let sum = 0;
  let count = 0;
  for (let i = rawSpeeds.length - 1; i >= 0 && count < SLIDING_WINDOW; i--) {
    if (rawSpeeds[i] > 0) {
      sum += rawSpeeds[i];
      count++;
    }
  }
  return count > 0 ? sum / count : 0;
}

/** 历史最高速度(字节/秒):遍历 rawSpeeds,对每个采集到非零数据的位置
    算其覆盖的 SLIDING_WINDOW 滑动窗口均值，取所有窗口均值的最大值。
    与 computeCurSpeed 同源同窗口，数学上严格 maxSpeed >= curSpeed,且不论
    polling 频率多少都和后端基于同份数组算出的最大值相等。 */
export function computeMaxSpeed(rawSpeeds: readonly number[] | undefined): number {
  if (!rawSpeeds || rawSpeeds.length === 0) return 0;
  let max = 0;
  for (let i = 0; i < rawSpeeds.length; i++) {
    // 仅对已采集到非零数据的位置计算窗口，避免被尾部连续 0 拖出"窗口均值低"的假窗口
    if (rawSpeeds[i] <= 0) continue;
    const begin = Math.max(0, i - (SLIDING_WINDOW - 1));
    let sum = 0;
    let count = 0;
    for (let k = begin; k <= i; k++) {
      // 同样跳过 0(异常采样位),只统计有效格，与 computeCurSpeed 行为一致
      if (rawSpeeds[k] > 0) {
        sum += rawSpeeds[k];
        count++;
      }
    }
    if (count > 0) {
      const avg = sum / count;
      if (avg > max) max = avg;
    }
  }
  return max;
}
