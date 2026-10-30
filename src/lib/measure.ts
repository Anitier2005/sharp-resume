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

/**
 * 量不到时的四句原因，与 facts.rs 的 `Reason::text()` 同一套（提示语不能只管一半）。
 *
 * `no-preview` 这个名字是历史包袱：离屏宿主现在常驻，量不到**不是**预览那边的问题，
 * 只剩「这一页还没量到」—— 纸面可能还在排、也可能还没挂上。别让这句话再把模型
 * 引到「预览开没开」上去。
 */
const REASON_TEXT: Record<NonNullable<PageMeasure["reason"]>, string> = {
  "no-preview": "这一页还没量到，纸面可能还在排",
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
 * 状态栏徽标的 title 文案（方案 §7.3 五句）。
 *
 * 留在 `lib/` 而不是 `status-bar.tsx`：那里引了 `.module.css`，node 环境的单测
 * 进不去 —— 于是这几句最容易写错的措辞就没人守了。
 *
 * **`targetPages` 是「这一段」的上限，不是整份**（方案路线二）。所以文案里必须
 * 出现「这一段」三个字：说成「目标 1 页」会让用户以为整份只有一张纸，而实际上
 * 三段简历每段都可以到 1 页。判定本身早在 `facts.rs::fit_of` 里做完，这里只翻译。
 */
export function measureTip(measure: PageMeasure | null | undefined, targetPages?: number): string {
  if (!measure) return "尚未实测（这一页还没量到）。";
  if (measure.status === "stale") return "尚未实测（内容在上次测量之后改过，旧测量作废）。";

  const { onePage, contentPx } = measure;
  if (measure.status !== "ok" || !onePage || !contentPx) {
    return `尚未实测（${REASON_TEXT[measure.reason ?? "no-preview"]}）。`;
  }

  const pages = contentPx / onePage;
  const shown = pages.toFixed(2);
  const bound = targetPages ? `这一段最多 ${targetPages} 页` : "";
  if (pages > TOLERANCE_MAX) {
    if (!targetPages) return `实测 ${shown} 页。没有目标页数，不讲达标与否。`;
    const lines = Math.round((contentPx - onePage) / (measure.lineHeight ?? LINE_HEIGHT_FALLBACK));
    return `实测 ${shown} 页（${bound}，未达标）。超出 ${(pages - 1).toFixed(2)} 页 · 约 ${lines} 行正文。`;
  }
  // 1 页以上、容忍上限以内：算没有溢出，但要如实说是「容忍」而不是「达标」
  if (pages > 1) {
    return targetPages
      ? `实测 ${shown} 页，在 2% 容忍内，仍算没有溢出（${bound}）。`
      : `实测 ${shown} 页，在 2% 容忍内，仍算没有溢出。`;
  }
  return targetPages
    ? `实测 ${shown} 页（${bound}，达标）。`
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
 * 一页能装多少内容、这页实际排了多少 —— 页数的整套换算只有这两行。
 *
 * 为什么必须减 `padY`：`A4_RATIO` 把纸宽换成的是**整张纸高 297mm**，而纸自己的
 * 上下内边距就是页边距（离屏宿主按打印口径给了 16mm）。那 32mm 是排不了字的，
 * 拿全高当分母等于把一页说大 12%，页数系统性偏小 —— 明明两页会说成 1.83 页。
 * 分子同理：`offsetHeight` 含上下边距，不减就是把误差从分母挪到分子。
 *
 * 抽成纯函数只为它值得一个单测：这两个数错了，模型看到的页数就跟着错，
 * 而那是它决定「压字号还是砍内容」的唯一依据。
 */
export function pageMetrics(input: {
  /** 纸的 offsetWidth */
  width: number;
  /** 纸的 offsetHeight（含上下页边距） */
  height: number;
  /** 纸的上下内边距之和，也就是上下一共多少页边距 */
  padY: number;
}): { onePage: number; contentPx: number } {
  const { width, height, padY } = input;
  return {
    onePage: width * A4_RATIO - padY,
    contentPx: height - padY,
  };
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
export interface PaperMeasures {
  hostRef: RefObject<HTMLDivElement | null>;
  person: Person | null;
  pages: ResumePage[];
}

/** 每页各自的那一份测量状态。共享一个 `last` 会让两页互相把对方判成 stale */
interface PageState {
  last: Snapshot | null;
  fontsReady: boolean;
  /** 这一页自己的「1.5s 还没量到」计时器 */
  timeout: number | null;
}

/**
 * 多页版：每一页在自己的离屏纸面上单独量，键是 pageId。
 *
 * 为什么必须分页：会话自己排版，`chat_send` 带的 `measure` 是模型唯一能知道
 * 「这一页压到几页了」的来源，而它问的可能是手上那一页、不是用户此刻看的那页；
 * 只量当前页的话，另一页永远拿不到实测 —— 模型看到的就是「这一页还没量到」。
 *
 * 为什么状态必须按页分开：两页共用一个 `last` 时，甲页量完会把乙页的
 * 上一次测量比成 stale，乙页的数字永远停在「旧测量作废」。
 *
 * `pages` 的渲染顺序与 `hostRef` 里 `[data-page]` 的顺序一致，不按索引取纸，
 * 而是按 `data-page` 反查 —— 删页、换页后索引会错位。
 */
export function usePaperMeasures(input: PaperMeasures): Record<string, PageMeasure> {
  const { hostRef, person, pages } = input;
  const [results, setResults] = useState<Record<string, PageMeasure>>({});

  const states = useRef(new Map<string, PageState>());
  /** 字体就绪是全局事实：后加的页不能因为「fonts effect 只跑一次」永远卡在 fonts */
  const fontsReady = useRef(false);
  /** 延时补读要用「最新那一份」read：闭包里的旧 read 会拿旧 content 去比，永远比不中 */
  const readRef = useRef<(pageId: string) => void>(() => undefined);

  /**
   * 每一页的内容版本。`contentVersion` 已经带着 layout 与 blocks，
   * 排版一改版本就变，测量跟着失效重测 —— 这一点保持原样。
   */
  const versions = useMemo(
    () => new Map(pages.map((page) => [page.id, contentVersion(person, page)])),
    [person, pages],
  );

  /** 内容指纹拼成一个字符串当依赖：页码/内容不变就不重装观察器 */
  const signature = pages.map((page) => `${page.id}:${versions.get(page.id)}`).join("|");

  // 依赖只认 signature：工作区其它状态变化（切面板、改批注）不该重装观察器、
  // 也不该把「1.5s 超时」的秒表反复归零。effect 里读 ref 拿最新一版。
  const pagesRef = useRef(pages);
  const versionsRef = useRef(versions);
  pagesRef.current = pages;
  versionsRef.current = versions;

  const stopTimeout = useCallback((state: PageState) => {
    if (state.timeout !== null) window.clearTimeout(state.timeout);
    state.timeout = null;
  }, []);

  /**
   * 量一遍所有页并盯住各自的纸面。
   *
   * 依赖里刻意没有「编辑 / 预览」这一档：宿主两态都在，测量不该随视图切换重算。
   */
  useLayoutEffect(() => {
    const host = hostRef.current;
    const pages = pagesRef.current;
    const versions = versionsRef.current;
    if (!host || !pages.length) return;

    const live = new Set(pages.map((page) => page.id));
    // 删掉的页连状态一起清，免得旧页的 last 一直挂着
    for (const id of [...states.current.keys()]) {
      if (!live.has(id)) states.current.delete(id);
    }

    /** 这一页自己的纸：容器里每页一个 `.wrap[data-page]`，其第一个子元素才是 `.paper` */
    const paperOf = (pageId: string): HTMLElement | null => {
      for (const child of Array.from(host.children)) {
        if (child.getAttribute("data-page") !== pageId) continue;
        return child.firstElementChild as HTMLElement | null;
      }
      return null;
    };

    const failed = (pageId: string, content: string, reason: NonNullable<PageMeasure["reason"]>) => {
      setResults((prev) => ({
        ...prev,
        [pageId]: { pageId, status: "unknown", version: okVersion, content, reason },
      }));
    };

    /**
     * 量这一页。四道拒绝的顺序有讲究：宽为 0 = 纸还没挂上；高 ≤0 = 这一页是空的；
     * 字体没就绪 = 数还会变；内容版本对不上 = 旧数不能当新数报。
     */
    const readOne = (pageId: string) => {
      const state = states.current.get(pageId);
      if (!state) return;
      const content = versions.get(pageId) ?? "";
      const paper = paperOf(pageId);
      if (!paper || !paper.offsetWidth) return failed(pageId, content, "no-preview");

      const style = getComputedStyle(paper);
      // 纸的上下内边距就是页边距（离屏宿主按打印口径给的 16mm）。
      // 一页能装的内容 = 整张纸高 − 上下页边距；那两个数只有 DOM 知道，
      // 不要再写死 265mm —— 页边距是可配的，写死就是第二个漂移源。
      const padY = (parseFloat(style.paddingTop) || 0) + (parseFloat(style.paddingBottom) || 0);
      const { onePage, contentPx } = pageMetrics({
        width: paper.offsetWidth,
        height: paper.offsetHeight,
        padY,
      });
      // 判空必须走在减完之后：只填满上下边距的纸，内容其实是 0
      if (contentPx <= 0) return failed(pageId, content, "empty");
      if (!state.fontsReady) return failed(pageId, content, "fonts");

      if (state.last && state.last.content !== content) {
        // 内容改过了（F-014）：旧测量作废，但把旧数带上，界面上才说得出「改之前是几页」
        const previous = state.last;
        state.last = null;
        setResults((prev) => ({
          ...prev,
          [pageId]: {
            pageId,
            status: "stale",
            version: okVersion,
            content,
            previous: {
              content: previous.content,
              onePage: previous.onePage,
              contentPx: previous.contentPx,
            },
          },
        }));
        state.timeout = window.setTimeout(() => readRef.current(pageId), STALE_RETRY_MS);
        return;
      }

      // 行高读实测值：把溢出量折成「多少行正文」比「超出 0.18 页」好懂
      const lineHeight = parseFloat(style.lineHeight) || LINE_HEIGHT_FALLBACK;
      okVersion += 1;
      state.last = { content, onePage, contentPx, lineHeight };
      stopTimeout(state);
      setResults((prev) => ({
        ...prev,
        [pageId]: {
          pageId,
          status: "ok",
          version: okVersion,
          content,
          onePage,
          contentPx,
          lineHeight,
        },
      }));
    };

    readRef.current = readOne;

    const observers: ResizeObserver[] = [];
    for (const page of pages) {
      const content = versions.get(page.id) ?? "";
      let state = states.current.get(page.id);
      if (!state) {
        state = {
          last: null,
          fontsReady: fontsReady.current || typeof document === "undefined" || !document.fonts,
          timeout: null,
        };
        states.current.set(page.id, state);
      }
      const pageState = state;
      stopTimeout(pageState);

      readOne(page.id);

      // 1.5s 还没量到这一版就说「超时」，不要一直挂着上一版。
      // readOne 可能刚给 stale 排了 300ms 的补读 —— 那个计时器已经占着槽位，
      // 不能再拿超时盖掉它（盖掉就永远补读不到了）。
      if (pageState.timeout === null && pageState.last?.content !== content) {
        pageState.timeout = window.setTimeout(() => {
          pageState.timeout = null;
          failed(page.id, content, "timeout");
        }, MEASURE_TIMEOUT_MS);
      }

      const paper = paperOf(page.id);
      if (!paper) continue;
      const observer = new ResizeObserver(() => readRef.current(page.id));
      observer.observe(paper);
      observers.push(observer);
    }

    return () => {
      for (const observer of observers) observer.disconnect();
      for (const state of states.current.values()) stopTimeout(state);
    };
  }, [signature, hostRef, stopTimeout]);

  // 字体只等一次。ready 之后所有页都可以给 ok —— 之前给的数都算没量到
  useEffect(() => {
    const fonts = typeof document === "undefined" ? undefined : document.fonts;
    if (!fonts) {
      fontsReady.current = true;
      for (const state of states.current.values()) state.fontsReady = true;
      return;
    }
    let alive = true;
    void fonts.ready.then(() => {
      if (!alive) return;
      fontsReady.current = true;
      for (const state of states.current.values()) state.fontsReady = true;
      for (const id of states.current.keys()) readRef.current(id);
    });
    return () => {
      alive = false;
    };
  }, []);

  return results;
}

/**
 * 原件页数只在内存里活一轮：它随 `chat_send` 的 payload 上送给模型，不落库。
 * 刷新/重启后就「这一轮没有上传原件」—— 诚实的退化，好过留一个会过期的假数。
 */
const origins = new Map<string, Origin>();

export function putOrigin(resumeId: string, origin: Origin): void {
  origins.set(resumeId, origin);
}

/**
 * 只读窥视：会话面板发 `chat_send` 时读它。
 * 做成「取走即删」的话，第二次读只能拿到 null，模型的事实块就会缺原件页数。
 */
export function originOf(resumeId: string): Origin | null {
  return origins.get(resumeId) ?? null;
}

export function clearOrigin(resumeId: string): void {
  origins.delete(resumeId);
}

export type { Origin, PageMeasure };
