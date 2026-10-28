/** 颜色小工具：服务「自定义强调色」与色板组件 */

/** HSV：h 0–360，s/v 0–100（用整数，避免下拉框里出现 33.33333） */
export interface Hsv {
  h: number;
  s: number;
  v: number;
}

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

/**
 * HEX → HSV。
 *
 * 色板要能「拖着色相条转一圈再拖回来，饱和度不掉」—— 这要求内部状态是 HSV
 * 而不是 HEX：从 HEX 反推，灰色（s=0）的色相信息已经丢了，拖回来会变成红色。
 * 所以调用方必须把 h/s/v 当状态存，只在提交时转成 HEX。
 */
export function hexToHsv(hex: string): Hsv | null {
  const rgb = parseHex(hex);
  if (!rgb) return null;
  const [r, g, b] = rgb.map((channel) => channel / 255) as [number, number, number];
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const delta = max - min;

  let h = 0;
  if (delta !== 0) {
    if (max === r) h = ((g - b) / delta) % 6;
    else if (max === g) h = (b - r) / delta + 2;
    else h = (r - g) / delta + 4;
    h *= 60;
    if (h < 0) h += 360;
  }

  return {
    h: Math.round(h),
    s: Math.round((max === 0 ? 0 : delta / max) * 100),
    v: Math.round(max * 100),
  };
}

/** HSV → #rrggbb（小写，带 #） */
export function hsvToHex(h: number, s: number, v: number): string {
  const sat = Math.min(100, Math.max(0, s)) / 100;
  const val = Math.min(100, Math.max(0, v)) / 100;
  const chroma = val * sat;
  const sector = ((((h % 360) + 360) % 360) / 60);
  const second = chroma * (1 - Math.abs((sector % 2) - 1));

  let r = 0;
  let g = 0;
  let b = 0;
  if (sector < 1) [r, g, b] = [chroma, second, 0];
  else if (sector < 2) [r, g, b] = [second, chroma, 0];
  else if (sector < 3) [r, g, b] = [0, chroma, second];
  else if (sector < 4) [r, g, b] = [0, second, chroma];
  else if (sector < 5) [r, g, b] = [second, 0, chroma];
  else [r, g, b] = [chroma, 0, second];

  const base = val - chroma;
  const toHex = (channel: number) =>
    Math.round((channel + base) * 255)
      .toString(16)
      .padStart(2, "0");

  return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
}
