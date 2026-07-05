// 通用格式化（CP-8.7：模型体积显示，mockup 2.7/2.9 的「1.3 GB / 512 MB」形态）
export function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) {
    const gb = bytes / 1024 ** 3;
    return `${gb >= 10 ? Math.round(gb) : gb.toFixed(1)} GB`;
  }
  return `${Math.round(bytes / 1024 ** 2)} MB`;
}
