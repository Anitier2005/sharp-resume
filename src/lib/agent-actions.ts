import type { PageLayout, PageStyle, Workspace } from "@/types";

/**
 * 执行会话 agent 下发的界面指令。
 *
 * 为什么指令要回传前端而不是后端直接改状态：
 *   权威数据在 Rust，但「当前看到哪一页、界面长什么样」是渲染态，
 *   只有 React 知道怎么落（reducer、页面栈、滚动位置）。
 *   于是后端只产出「要做什么」，前端负责「怎么做」。
 *
 * 这里的白名单不是多余的：即使后端已经校验过，也不该让一条脏指令
 * 把任意字段塞进 ui（比如往 ui 里写 activeResumeId 就能把用户切走）。
 */

export interface AskQuestion {
  id: string;
  label: string;
  kind: "text" | "textarea" | "select" | "multi" | "confirm" | "number" | "date";
  hint?: string;
  required?: boolean;
  options?: string[];
}

export interface AgentActionHooks {
  openPage: (pageId: string) => void;
  patchUi: (patch: Partial<Workspace["ui"]>) => void;
  /** 模型要求填表时调用；由面板弹出表单 */
  ask: (questions: AskQuestion[]) => void;
  /** 排版 agent 给出的页样式（旧的三档疏密，只在读旧消息时出现） */
  pageStyle: (pageId: string, style: PageStyle) => void;
  /** 排版 agent 写下的成品层覆盖：整页手写或某几块手写 */
  pageLayout: (pageId: string, layout: PageLayout) => void;
  /** 会话 agent 要求某一页重新排版 */
  requestLayout: (pageId: string, brief: string) => void;
}

const VIEWS = new Set(["edit", "preview"]);
const UI_KEYS = [
  "accent",
  "accentHex",
  "mode",
  "textScale",
  "sideW",
  "docW",
  "showSide",
  "showDoc",
] as const;

const ASK_KINDS = new Set([
  "text",
  "textarea",
  "select",
  "multi",
  "confirm",
  "number",
  "date",
]);

function cleanQuestions(input: unknown): AskQuestion[] {
  if (!Array.isArray(input)) return [];
  const out: AskQuestion[] = [];
  for (const raw of input) {
    if (!raw || typeof raw !== "object") continue;
    const item = raw as Record<string, unknown>;
    const label = typeof item.label === "string" ? item.label.trim() : "";
    const kind = typeof item.kind === "string" ? item.kind : "";
    if (!label || !ASK_KINDS.has(kind)) continue;
    const id = typeof item.id === "string" && item.id.trim() ? item.id.trim() : `q${out.length + 1}`;
    const question: AskQuestion = { id, label, kind: kind as AskQuestion["kind"] };
    if (typeof item.hint === "string" && item.hint.trim()) question.hint = item.hint.trim();
    if (item.required === true) question.required = true;
    if (Array.isArray(item.options)) {
      const options = item.options.filter(
        (value): value is string => typeof value === "string" && !!value.trim(),
      );
      if (options.length) question.options = options;
    }
    if ((question.kind === "select" || question.kind === "multi") && !question.options?.length) {
      continue;
    }
    out.push(question);
  }
  return out.slice(0, 8);
}

/**
 * 校验成品层覆盖。
 *
 * 注意 `{}` 是合法值：那是「reset 之后没有任何覆盖」，不是「没给」。
 * 把它当成无效会让「撤销所有手写」这条指令失效 —— 用户看到的就是点了没反应。
 */
function cleanLayout(input: unknown): PageLayout | null {
  if (!input || typeof input !== "object" || Array.isArray(input)) return null;
  const source = input as Record<string, unknown>;
  const layout: PageLayout = {};

  if (typeof source.preset === "string" && source.preset.trim()) {
    layout.preset = source.preset.trim();
  }
  if (typeof source.html === "string" && source.html.trim()) {
    layout.html = source.html;
  }
  if (source.parts && typeof source.parts === "object" && !Array.isArray(source.parts)) {
    const parts: Record<string, string> = {};
    for (const [blockId, value] of Object.entries(source.parts as Record<string, unknown>)) {
      if (typeof value === "string" && value.trim()) parts[blockId] = value;
    }
    if (Object.keys(parts).length) layout.parts = parts;
  }

  return layout;
}

function cleanPatch(input: unknown): Partial<Workspace["ui"]> {
  if (!input || typeof input !== "object") return {};
  const source = input as Record<string, unknown>;
  const patch: Record<string, unknown> = {};
  for (const key of UI_KEYS) {
    if (key in source && source[key] !== undefined) patch[key] = source[key];
  }
  return patch as Partial<Workspace["ui"]>;
}

/** 返回实际应用的条数，调用方据此决定要不要提示用户 */
export function applyAgentActions(actions: unknown, hooks: AgentActionHooks): number {
  if (!Array.isArray(actions)) return 0;
  let applied = 0;

  for (const raw of actions) {
    if (!raw || typeof raw !== "object") continue;
    const action = raw as Record<string, unknown>;

    switch (action.type) {
      case "openPage": {
        const pageId = typeof action.pageId === "string" ? action.pageId.trim() : "";
        if (!pageId) break;
        hooks.openPage(pageId);
        applied += 1;
        break;
      }
      case "setView": {
        const view = typeof action.view === "string" ? action.view : "";
        if (!VIEWS.has(view)) break;
        hooks.patchUi({ view: view as "edit" | "preview" });
        applied += 1;
        break;
      }
      case "setOpenPages": {
        const list = Array.isArray(action.pageIds)
          ? action.pageIds.filter((item): item is string => typeof item === "string" && !!item.trim())
          : [];
        if (!list.length) break;
        hooks.patchUi({ openPages: list });
        applied += 1;
        break;
      }
      case "patchUi": {
        const patch = cleanPatch(action.patch);
        if (!Object.keys(patch).length) break;
        hooks.patchUi(patch);
        applied += 1;
        break;
      }
      case "patchPageStyle": {
        const pageId = typeof action.pageId === "string" ? action.pageId.trim() : "";
        const style = action.style;
        if (!pageId || !style || typeof style !== "object") break;
        hooks.pageStyle(pageId, style as PageStyle);
        applied += 1;
        break;
      }
      case "patchPageLayout": {
        const pageId = typeof action.pageId === "string" ? action.pageId.trim() : "";
        const layout = cleanLayout(action.layout);
        if (!pageId || !layout) break;
        hooks.pageLayout(pageId, layout);
        applied += 1;
        break;
      }
      case "requestLayout": {
        const pageId = typeof action.pageId === "string" ? action.pageId.trim() : "";
        const brief = typeof action.brief === "string" ? action.brief.trim() : "";
        if (!pageId) break;
        hooks.requestLayout(pageId, brief);
        break;
      }
      case "ask": {
        const questions = cleanQuestions(action.questions);
        if (!questions.length) break;
        hooks.ask(questions);
        break;
      }
      default:
        break;
    }
  }

  return applied;
}
