/** 刷新按钮用:保证 loading 至少持续 minMs，避免 IPC 太快导致转圈动画根本不绘制。 */
export async function withRefreshSpin<T>(
  setSpinning: (v: boolean) => void,
  work: () => Promise<T>,
  minMs = 450
): Promise<T> {
  setSpinning(true);
  // 让出一帧，确保 animate-spin 的 class 已经绘到屏幕上再跑 IPC
  await new Promise<void>((r) => requestAnimationFrame(() => r()));
  const started = Date.now();
  try {
    return await work();
  } finally {
    const left = minMs - (Date.now() - started);
    if (left > 0) {
      await new Promise((r) => setTimeout(r, left));
    }
    setSpinning(false);
  }
}
