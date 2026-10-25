/** 颜色小工具：只服务「自定义强调色」需要的最小能力 */

/** #rgb / #rrggbb → [r,g,b]；解析失败返回 null */
function parseHex(hex: string): [number, number, number] | null {
  const value = hex.trim().replace(/^#/, "");
  const full =
    value.length === 3
      ? value
          .split("")
          .map((ch) => ch + ch)
          .join("")
      : value;
  if (!/^[0-9a-fA-F]{6}$/.test(full)) return null;
  return [
    parseInt(full.slice(0, 2), 16),
    parseInt(full.slice(2, 4), 16),
    parseInt(full.slice(4, 6), 16),
  ];
}

export function isHexColor(value: string): boolean {
  return parseHex(value) !== null;
}

/**
 * 强调色上该用白字还是深字。
 * 用 WCAG 的相对亮度近似（不做 gamma 校正的简化版即可，只为可读性兜底）。
 */
export function readableOn(hex: string): "#ffffff" | "#141416" {
  const rgb = parseHex(hex);
  if (!rgb) return "#ffffff";
  const [r, g, b] = rgb.map((channel) => channel / 255) as [number, number, number];
  const luminance = 0.299 * r + 0.587 * g + 0.114 * b;
  return luminance > 0.62 ? "#141416" : "#ffffff";
}
