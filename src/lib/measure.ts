import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";
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

/* ==========================================================================
   离屏测量 hook
   ========================================================================== */

/** 每次成功测量 +1。放模块级：两个组件同时用不会互相盖掉对方的计数 */
let okVersion = 0;

/** 一次测量最多等这么久；到点还没有一次 ok 就明说量不到，不让状态栏挂着旧数 */
const MEASURE_TIMEOUT_MS = 1500;

/** 内容改短到和上一版一样高时观察器不会响，靠这一下补读 */
const STALE_RETRY_MS = 300;

/** 上一次成功测量的快照：降级成 stale 时要把它原样交出去 */
interface Snapshot {
  content: string;
  onePage: number;
  contentPx: number;
  lineHeight: number;
}

/**
 * 从离屏宿主量这一页有几页。
 *
 * 为什么不量预览那份：默认视图是编辑，预览根本不在 DOM 里；而两份各算一次必然
 * 漂移。宿主常驻，这里只认它一份。
 *
 * 字体没就绪时宁可给 `unknown("fonts")` —— 字形宽度不同会改变换行与总行数，
 * 那一刻量到的数是会变的数，而「会变的数」正是 F-007 要防的东西。
 */
export function usePageMeasure(input: {
  /** 离屏宿主的 `.wrap` 元素；它的第一个子元素才是 `.paper` */
  hostRef: RefObject<HTMLDivElement | null>;
  person: Person | null;
  page: ResumePage | null;
}): PageMeasure | null {
  const { hostRef, person, page } = input;
  const content = useMemo(() => contentVersion(person, page), [person, page]);
  const pageId = page?.id ?? "";
  const [result, setResult] = useState<PageMeasure | null>(null);

  const last = useRef<Snapshot | null>(null);
  const fontsReady = useRef(false);
  const timeout = useRef<number | null>(null);
  /** 延时补读要用「最新那一份」read：闭包里的旧 read 会拿旧 content 去比，永远比不中 */
  const readRef = useRef<() => void>(() => undefined);

  const stopTimeout = useCallback(() => {
    if (timeout.current !== null) window.clearTimeout(timeout.current);
    timeout.current = null;
  }, []);

  /**
   * 量一次。四道拒绝的顺序有讲究：宽为 0 = 纸还没挂上；高 ≤0 = 这一页是空的；
   * 字体没就绪 = 数还会变；内容版本对不上 = 旧数不能当新数报。
   */
  const read = useCallback(() => {
    const paper = hostRef.current?.firstElementChild as HTMLElement | null;
    const failed = (reason: NonNullable<PageMeasure["reason"]>) =>
      setResult({ pageId, status: "unknown", version: okVersion, content, reason });

    if (!paper || !paper.offsetWidth) return failed("no-preview");
    const contentPx = paper.offsetHeight;
    if (contentPx <= 0) return failed("empty");
    if (!fontsReady.current) return failed("fonts");

    if (last.current && last.current.content !== content) {
      // 内容改过了（F-014）：旧测量作废，但把旧数带上，界面上才说得出「改之前是几页」
      const previous = last.current;
      last.current = null;
      setResult({
        pageId,
        status: "stale",
        version: okVersion,
        content,
        previous: {
          content: previous.content,
          onePage: previous.onePage,
          contentPx: previous.contentPx,
        },
      });
      window.setTimeout(() => readRef.current(), STALE_RETRY_MS);
      return;
    }

    const onePage = paper.offsetWidth * A4_RATIO;
    // 行高读实测值：把溢出量折成「多少行正文」比「超出 0.18 页」好懂
    const lineHeight = parseFloat(getComputedStyle(paper).lineHeight) || LINE_HEIGHT_FALLBACK;
    okVersion += 1;
    last.current = { content, onePage, contentPx, lineHeight };
    stopTimeout();
    setResult({ pageId, status: "ok", version: okVersion, content, onePage, contentPx, lineHeight });
  }, [content, hostRef, pageId, stopTimeout]);

  // 延时补读要用「最新那一份」read：闭包里的旧 read 会拿旧 content 去比，永远比不中
  useEffect(() => {
    readRef.current = read;
  }, [read]);

  // 字体只等一次。ready 之后所有 read 都可以给 ok —— 之前给的数都算没量到
  useEffect(() => {
    const fonts = typeof document === "undefined" ? undefined : document.fonts;
    if (!fonts) {
      fontsReady.current = true;
      return;
    }
    let alive = true;
    void fonts.ready.then(() => {
      if (!alive) return;
      fontsReady.current = true;
      readRef.current();
    });
    return () => {
      alive = false;
    };
  }, []);

  /**
   * 量一次并盯住纸面。
   *
   * 依赖里刻意没有「编辑 / 预览」这一档：宿主两态都在，测量不该随视图切换重算。
   */
  useLayoutEffect(() => {
    read();
    const paper = hostRef.current?.firstElementChild;
    if (!paper) return;
    const observer = new ResizeObserver(() => readRef.current());
    observer.observe(paper);
    return () => observer.disconnect();
  }, [read, hostRef]);

  // 1.5s 还没量到这一版就说「超时」，不要一直挂着上一版
  useEffect(() => {
    if (!pageId || !content) return;
    stopTimeout();
    if (last.current?.content === content) return;
    timeout.current = window.setTimeout(() => {
      setResult({ pageId, status: "unknown", version: okVersion, content, reason: "timeout" });
    }, MEASURE_TIMEOUT_MS);
    return stopTimeout;
  }, [content, pageId, stopTimeout]);

  return result;
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
