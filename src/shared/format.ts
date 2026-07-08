// 通用格式化（CP-8.7：模型体积显示，05 §5.1 的「1.3 GB / 1.0 GB」形态）
export function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) {
    const gb = bytes / 1024 ** 3;
    return `${gb >= 10 ? Math.round(gb) : gb.toFixed(1)} GB`;
  }
  return `${Math.round(bytes / 1024 ** 2)} MB`;
}
