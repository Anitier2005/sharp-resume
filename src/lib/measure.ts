import type { Origin, PageMeasure, Person, ResumePage } from "@/types";

/**
 * 页面测量：类型、内容版本、状态提示、原件页数的内存暂存。
 *
 * 这里**不判定**达标与否 —— 那是 `src-tauri/src/agent/facts.rs` 的唯一职责。
 * 本文件只做两件前端才能做的事：给「我量的到底是哪一版内容」编号（只有 DOM 这边
 * 知道），以及把量到的数翻译成一句给用户看的提示。
 */

/** A4 高宽比。纸宽 → 一页高度的唯一换算，别再各处手写 297/210 */
export const A4_RATIO = 297 / 210;

/**
 * 容忍上限：与 Rust 的 `TOLERANCE_MAX` 同值。
 * 只用来决定提示语是「容忍内」还是「溢出」—— 真正的判定在 facts.rs，前端不据它拦任何东西。
 */
export const TOLERANCE_MAX = 1.02;

/** 行高兜底：paper.css 默认 `--fs:10px × --lh:1.7`；读不到 computedStyle 时用它折算行数 */
const LINE_HEIGHT_FALLBACK = 17;

/** 量不到时的四句原因，与 facts.rs 的 `Reason::text()` 同一套（提示语不能只管一半） */
const REASON_TEXT: Record<NonNullable<PageMeasure["reason"]>, string> = {
  "no-preview": "预览没打开，量不到",
  fonts: "字体还没加载完",
  timeout: "测量超时",
  empty: "这一页是空的，没有可量的内容",
};

const FNV_OFFSET = 0x811c9dc5;
const FNV_PRIME = 0x01000193;

function fnv1a(input: string): number {
  let hash = FNV_OFFSET;
  for (let index = 0; index < input.length; index += 1) {
    hash ^= input.charCodeAt(index);
    hash = Math.imul(hash, FNV_PRIME);
  }
  return hash >>> 0;
}

/**
 * 稳定序列化：对象键排序、数组保序。
 *
 * `Record`（比如 `layout.parts`）的键序不稳定 —— 同一份内容两次 JSON.stringify
 * 可能给出两个不同字符串，版本号就白算了，卡片上会出现「内容没改但提示过期」。
 */
function canon(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value) ?? "null";
  if (Array.isArray(value)) return `[${value.map(canon).join(",")}]`;
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort();
  return `{${keys.map((key) => `${JSON.stringify(key)}:${canon(record[key])}`).join(",")}}`;
}

/**
 * 内容版本：参与排版的那份内容的指纹（8 位 hex）。
 *
 * 为什么由前端算：测量对象是这里渲染出来的 DOM，只有前端知道「我量的是哪一版」。
 * Rust 侧若用 serde_json 复算同一份哈希，就得两头 key 顺序永远一致 —— 那是个
 * 必然会腐烂的约定。
 *
 * `notes` 不计入：批注绝对定位，不影响高度，它变了不该让测量作废。
 */
export function contentVersion(person: Person | null, page: ResumePage | null): string {
  if (!person || !page) return "";
  const shape = {
    kind: page.kind ?? "native",
    assetId: page.assetId ?? "",
    layout: page.layout ?? null,
    blocks: page.blocks ?? [],
    fields: (person.fields ?? []).map((field) => [field.label, field.value, field.assetId ?? ""]),
    assets: (person.assets ?? []).map((asset) => [asset.id, asset.stored]),
  };
  return fnv1a(canon(shape)).toString(16).padStart(8, "0");
}

/**
 * 状态栏徽标的 title 文案（方案 §9.2 五句）。
 *
 * 留在 `lib/` 而不是 `status-bar.tsx`：那里引了 `.module.css`，node 环境的单测
 * 进不去 —— 于是这两句最容易写错的措辞就没人守了。
 */
export function measureTip(measure: PageMeasure | null | undefined, targetPages?: number): string {
  if (!measure) return "尚未实测（预览没打开，量不到）。";
  if (measure.status === "stale") return "尚未实测（内容在上次测量之后改过，旧测量作废）。";

  const { onePage, contentPx } = measure;
  if (measure.status !== "ok" || !onePage || !contentPx) {
    return `尚未实测（${REASON_TEXT[measure.reason ?? "no-preview"]}）。`;
  }

  const pages = contentPx / onePage;
  const shown = pages.toFixed(2);
  if (pages > TOLERANCE_MAX) {
    if (!targetPages) return `实测 ${shown} 页。没有目标页数，不讲达标与否。`;
    const lines = Math.round((contentPx - onePage) / (measure.lineHeight ?? LINE_HEIGHT_FALLBACK));
    return `实测 ${shown} 页（目标 ${targetPages} 页，未达标）。超出 ${(pages - 1).toFixed(2)} 页 · 约 ${lines} 行正文。`;
  }
  // 1 页以上、容忍上限以内：算没有溢出，但要如实说是「容忍」而不是「达标」
  if (pages > 1) return `实测 ${shown} 页，在 2% 容忍内，仍算没有溢出。`;
  return targetPages
    ? `实测 ${shown} 页（目标 ${targetPages} 页，达标）。`
    : `实测 ${shown} 页。没有目标页数，不讲达标与否。`;
}

/**
 * 原件页数只在内存里活一轮：它随 `layout_submit` 的 payload 走，不落库。
 * 刷新/重启后就「这一轮没有上传原件」—— 诚实的退化，好过留一个会过期的假数。
 */
const origins = new Map<string, Origin>();

export function putOrigin(resumeId: string, origin: Origin): void {
  origins.set(resumeId, origin);
}

/**
 * 只读窥视：自动自查与 `submitLayout` 都要读同一份。
 * 做成「取走即删」的话，第二个读的人只能拿到 null，排版侧的事实块就会缺原件页数。
 */
export function originOf(resumeId: string): Origin | null {
  return origins.get(resumeId) ?? null;
}

export function clearOrigin(resumeId: string): void {
  origins.delete(resumeId);
}

export type { Origin, PageMeasure };
