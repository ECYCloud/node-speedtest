/**
 * 速度展示辅助：基于引擎写入的 rawSocketSpeed 采样序列。
 *
 * 引擎每约 0.5s 写入一格 EMA 吞吐（字节/秒）。
 * 「实时速度」用短窗平滑；「最高速度」取采样峰值，与后端 dspeedMax / 导出图同源。
 */

const SLIDING_WINDOW = 4;

/** 当前实时速度（字节/秒）：最近若干非零采样的均值。 */
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

/** 最高速度（字节/秒）：rawSocketSpeed 非零采样的峰值，与结果表 / dspeedMax 一致。 */
export function computeMaxSpeed(rawSpeeds: readonly number[] | undefined): number {
  if (!rawSpeeds || rawSpeeds.length === 0) return 0;
  let max = 0;
  for (let i = 0; i < rawSpeeds.length; i++) {
    if (rawSpeeds[i] > max) max = rawSpeeds[i];
  }
  return max;
}
