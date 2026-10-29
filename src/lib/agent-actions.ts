import type { BlockRole, PageLayout, PageNote, PageStyle, ResumePage, Workspace } from "@/types";
import type { Action } from "@/store/workspace/types";

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
  /**
   * 换候选人：简历也要跟着落到这个人的第一份 —— 只改 activePersonId 会让
   * 右侧停在上一个人的简历上（模型以为切了，用户看到没动）。
   */
  openPerson: (personId: string) => void;
  /** 换简历：同一个人的另一份，当前页落到它的第一页 */
  openResume: (personId: string, resumeId: string) => void;
  patchUi: (patch: Partial<Workspace["ui"]>) => void;
  /** 模型要求填表时调用；由面板弹出表单 */
  ask: (questions: AskQuestion[]) => void;
  /** 排版 agent 给出的页样式（旧的三档疏密，只在读旧消息时出现） */
  pageStyle: (pageId: string, style: PageStyle) => void;
  /** 排版 agent 写下的成品层覆盖：整页手写或某几块手写 */
  pageLayout: (pageId: string, layout: PageLayout) => void;
  /** 会话 agent 要求某一页重新排版 */
  requestLayout: (pageId: string, brief: string) => void;
  /**
   * 内容写入通道。
   *
   * 后端不直接改库，它把「和用户点击一模一样」的 reducer action 装进 `mutate`
   * 信封交回来，由工作区按顺序落地 —— 校验、确认窗、撤销、持久化全部复用同一套。
   * 确认门（要不要先问用户）也在这个 hook 的实现里，不在校验层。
   */
  applyOps: (ops: Action[]) => void;
  /** 排版 agent 在纸面上留的一条批注 */
  pageNote: (pageId: string, note: PageNote) => void;
  /**
   * 排版 agent 向会话 agent 提问。
   *
   * 这是 agent 之间的往返，不进对话流：问题是内部的，答复也回投给排版信箱。
   * `blockId` 有值时说明问的是某一块，要写进提问与答复里，
   * 否则会话 agent 得自己再去读一遍页面才能对上号。
   */
  askSession: (pageId: string, question: string, blockId?: string) => void;
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

/* ==========================================================================
   内容写入（mutate 信封）
   ========================================================================== */

const isObject = (value: unknown): value is Record<string, unknown> =>
  !!value && typeof value === "object" && !Array.isArray(value);

/** 非空字符串：id 类字段的唯一判据 */
const text = (value: unknown): string =>
  typeof value === "string" ? value.trim() : "";

/**
 * 允许透传的 op（契约 1.2）。
 *
 * 这里只核对「信封层面的形状」——类型在不在名单里、必填的 id 在不在、
 * 该是数组/对象的位置对不对。字段内部的语义（比如 items 里每条长什么样）
 * 由 Rust 侧同一份契约保证：前端再抄一遍只会多一份会腐烂的真相。
 */
interface OpSpec {
  /** 必填、且必须是非空字符串的字段 */
  ids: string[];
  /**
   * 选填、但一旦出现就必须是非空字符串的字段。
   *
   * `addSection.id` / `addPage.id` / `createResume.resumeId` 是工具侧预生成的容器 id：
   * 同一轮里「先建容器、再往里写」全靠它对上，所以它不能是空串或数字。
   */
  optionalIds?: string[];
  /** 必填、且必须是数组的字段 */
  arrays?: string[];
  /** 必填、且必须是对象的字段 */
  objects?: string[];
}

const OP_SPEC: Record<string, OpSpec> = {
  personFields: { ids: ["personId"], arrays: ["fields"] },
  libraryItems: { ids: ["personId"], arrays: ["items"] },
  personAssets: { ids: ["personId"], arrays: ["assets"] },
  itemAssets: { ids: ["personId", "itemId"], arrays: ["assets"] },
  // id 是新页 id（工具预生成）；同包里的 pageId 只是冗余的「当前页」，不看它
  addPage: { ids: ["personId", "resumeId"], optionalIds: ["id"] },
  removePage: { ids: ["personId", "resumeId", "pageId"] },
  // label / title 允许是空串（用户可以先建页再起名），所以不进 ids
  pageLabel: { ids: ["personId", "resumeId", "pageId"] },
  pageHeader: { ids: ["personId", "resumeId", "pageId"], arrays: ["header"] },
  pageStyle: { ids: ["personId", "resumeId", "pageId"], objects: ["style"] },
  pageLayout: { ids: ["personId", "resumeId", "pageId"], objects: ["layout"] },
  blockRole: { ids: ["personId", "resumeId", "pageId", "blockId"] },
  addSection: {
    ids: ["personId", "resumeId", "pageId"],
    optionalIds: ["id", "afterBlockId"],
  },
  removeSection: { ids: ["personId", "resumeId", "pageId", "sectionId"] },
  blockFields: { ids: ["personId", "resumeId", "pageId", "blockId"], arrays: ["fields"] },
  removeBlock: { ids: ["personId", "resumeId", "pageId", "blockId"] },
  sectionTitle: { ids: ["personId", "resumeId", "pageId", "sectionId"] },
  sectionItems: { ids: ["personId", "resumeId", "pageId", "sectionId"], arrays: ["items"] },
  entryPatch: { ids: ["personId", "resumeId", "pageId", "entryId"], objects: ["patch"] },
  createResume: { ids: ["personId"], optionalIds: ["resumeId"] },
  renameResume: { ids: ["personId", "resumeId"] },
  deleteResume: { ids: ["personId", "resumeId"] },
  deletePerson: { ids: ["personId"] },
  // 目标页数是数字，进不了 ids/optionalIds（那两个是「非空字符串」的判据）——
  // 它的校验在 cleanOp 里单独写，因为「省略」是一个合法值（= 清除）
  resumeTargetPages: { ids: ["personId", "resumeId"] },
  // 页级 blocks 全量替换（split_page 用）
  pageBlocks: { ids: ["personId", "resumeId", "pageId"], arrays: ["blocks"] },
  // 删空页：不进确认门，判空在工具侧与 reducer 侧各一次
  removeEmptyPage: { ids: ["personId", "resumeId", "pageId"] },
  // 状态标记：key 走白名单（见 cleanOp），value 必须是布尔
  resumeNotice: { ids: ["personId", "resumeId", "key"] },
};

/** `resumeNotice` 的白名单：一个新 key 进来，先想清楚它会不会和内容语义打架 */
const RESUME_NOTICE_KEYS = new Set(["targetPagesNotice"]);

const BLOCK_ROLES = new Set<BlockRole>([
  "plain",
  "tight",
  "band",
  "tags",
  "kv",
  "table",
  "grid-2",
  "grid-3",
  "cols-2",
  "cols-3",
  "row",
]);

/** 批注的最小形状：没有 id 就无从去重，没有 text 就是空点 */
function cleanNote(input: unknown): PageNote | null {
  if (!isObject(input)) return null;
  const id = text(input.id);
  const body = text(input.text);
  if (!id || !body) return null;
  const at =
    typeof input.at === "number" && Number.isFinite(input.at)
      ? input.at
      : Math.floor(Date.now() / 1000);
  const note: PageNote = { id, text: body, at };
  const blockId = text(input.blockId);
  if (blockId) note.blockId = blockId;
  return note;
}

function cleanOp(raw: unknown): Action | null {
  if (!isObject(raw)) return null;
  const type = text(raw.type);
  const spec = OP_SPEC[type];
  if (!spec) return null;
  for (const key of spec.ids) {
    if (!text(raw[key])) return null;
  }
  for (const key of spec.optionalIds ?? []) {
    if (raw[key] !== undefined && !text(raw[key])) return null;
  }
  for (const key of spec.arrays ?? []) {
    if (!Array.isArray(raw[key])) return null;
  }
  for (const key of spec.objects ?? []) {
    if (!isObject(raw[key])) return null;
  }
  // role 省略 = 清掉提示；给了就必须是认识的，否则整条丢掉（别把未知字符串写进存档）
  // split 前端也不放行：render 已经把它撤了，放进来只会出现「前端过、渲染层警告」的分裂
  if (type === "blockRole" && raw.role !== undefined && !BLOCK_ROLES.has(raw.role as BlockRole)) {
    return null;
  }
  // 状态标记只认白名单 key、value 必须是布尔：这个 op 给模型写「任意键」的口子，
  // 等于让它可以往 Resume 上塞任何东西（包括 targetPages 这类有语义的键）
  if (type === "resumeNotice") {
    if (!RESUME_NOTICE_KEYS.has(raw.key as string)) return null;
    if (typeof raw.value !== "boolean") return null;
  }
  // 目标页数：省略 = 清除（合法）；给了就必须是正整数。0 / 负数 / 小数一律丢这一条 ——
  // 丢一条脏数据不该炸掉同一批里其它合法写入，所以这里返回 null 而不是抛错
  if (type === "resumeTargetPages" && raw.targetPages !== undefined) {
    const pages = raw.targetPages;
    if (typeof pages !== "number" || !Number.isInteger(pages) || pages < 1) return null;
  }
  return raw as Action;
}

/**
 * 逐条校验，坏的那条丢掉、其余照常。
 *
 * 为什么不是「一条错整批丢」：后端 `apply_patch` 已经做过整批回滚，
 * 到了这里说明它是合法的；能走到这里的脏数据多半是版本不一致（新字段、
 * 老前端），把用户真正想要的那几十条一起丢掉伤害更大。
 */
export function cleanOps(input: unknown): Action[] {
  if (!Array.isArray(input)) return [];
  const out: Action[] = [];
  for (const raw of input) {
    const op = cleanOp(raw);
    if (op) out.push(op);
    else console.warn("[agent-actions] 丢弃不合法的写入动作", raw);
  }
  return out;
}

/**
 * 删除类 op：不可撤销、且用户点 UI 也会被问的那几种。
 *
 * `removeSection` 在内：删掉一个含 5 条经历的分区比删一页还狠，
 * 只是它的入口一直没做确认（那是 UI 的漏洞，不是标准）。
 */
const DESTRUCTIVE = new Set([
  "removePage",
  "deleteResume",
  "deletePerson",
  "removeBlock",
  "removeSection",
]);

export function isDestructiveOp(op: Action): boolean {
  return DESTRUCTIVE.has(op.type);
}

/**
 * 确认门：这一批要不要先问用户。
 *
 * 判据只有一条：**用户点 UI 会被问的，agent 也要被问** —— 也就是这四种删除。
 * 「整批挂起」是范围问题，不在这里判：`applyOps` 拿到 true 之后会把整批
 * （包括同一批里不删除的改动）一起压住，等用户点头再按原顺序执行。
 */
export function needsConfirm(ops: Action[]): boolean {
  const removals = ops.filter(isDestructiveOp).length;
  return removals >= 1;
}

/* ==========================================================================
   排版 ↔ 会话 的桥

   排版 agent 的提问最终要让会话 agent 跑一轮，而「跑一轮」需要模型设置、
   流式通道、工具事件 —— 全都在聊天面板里。provider 拿不到那些东西，
   所以用一个极小的注册表把两边接起来：面板挂载时注册，卸载时注销。
   ========================================================================== */

export interface ChatBridge {
  /**
   * 把一条消息追加进当前会话并跑一轮。
   *
   * `silent` 表示这轮是 agent 之间的往返：消息进模型上下文但不进对话流，
   * 界面上看不到提问、答复、打字动画，答复由面板回投给排版信箱。
   */
  turn: (pageId: string, content: string, silent?: boolean, blockId?: string) => void;
  /** 只追加一条系统提示，不触发模型 */
  notice: (content: string) => void;
}

let bridge: ChatBridge | null = null;

export function registerChatBridge(next: ChatBridge): () => void {
  bridge = next;
  return () => {
    if (bridge === next) bridge = null;
  };
}

export function chatBridge(): ChatBridge | null {
  return bridge;
}

/**
 * 内部提问用的提示词，不进对话流 —— 它只喂给会话 agent，
 * 让会话 agent 知道问的是哪一页、哪一块，不必自己再读一遍页面。
 */
export function layoutQuestion(pageNo: number, question: string, blockId?: string): string {
  const head = pageNo > 0 ? `排版 · 关于第 ${pageNo} 页` : "排版";
  const where = blockId ? `${head} · 块 ${blockId}` : head;
  return `${where}：${question.trim()}`;
}

/**
 * 同一页 5 秒内只让会话 agent 跑一轮。
 *
 * 两个 agent 互相呼叫很容易打成跨轮循环，后端的单轮限次挡不住它，
 * 这里的节流是最后一道闸。表放模块级而不是各组件一份：排版事件（provider）
 * 与会话指令（面板）是两条入口，各持一份就等于两个闸门，谁也挡不住对方。
 */
const ASK_COOLDOWN_MS = 5000;
const askedAt = new Map<string, number>();

/**
 * 排版 agent 向会话 agent 提问的唯一入口。
 *
 * 只把提问交给 ChatBridge：格式化、防重、落消息都在这一条路上，
 * provider 与面板都不该再各写一份，否则两边漂移时最难查的就是「为什么问了两遍」。
 */
export function askSessionFromLayout(input: {
  /** 提问文案要带「第几页」，页序以调用方手里那份简历为准 */
  pages: ResumePage[];
  pageId: string;
  question: string;
  blockId?: string;
}): void {
  const next = chatBridge();
  if (!next) {
    console.warn("[layout] 会话面板还没准备好，这次提问被跳过", input.question);
    return;
  }
  const now = Date.now();
  if (now - (askedAt.get(input.pageId) ?? 0) < ASK_COOLDOWN_MS) return;
  askedAt.set(input.pageId, now);

  const index = input.pages.findIndex((item) => item.id === input.pageId);
  next.turn(
    input.pageId,
    layoutQuestion(index + 1, input.question, input.blockId),
    true,
    input.blockId,
  );
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
      case "openPerson": {
        const personId = text(action.personId);
        if (!personId) break;
        hooks.openPerson(personId);
        applied += 1;
        break;
      }
      case "openResume": {
        const personId = text(action.personId);
        const resumeId = text(action.resumeId);
        if (!personId || !resumeId) break;
        hooks.openResume(personId, resumeId);
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
      case "mutate": {
        // 内容写入：信封里是一串 reducer action。坏的那条丢掉，其余照常。
        // 不计入 applied：调用方据此弹「已调整界面 N 处」，而内容写入的结果
        // 要么已经出现在纸面上、要么还挂在确认窗里，报个数只会让人困惑
        const ops = cleanOps(action.ops);
        if (!ops.length) break;
        hooks.applyOps(ops);
        break;
      }
      case "pageNote": {
        const pageId = text(action.pageId);
        const note = cleanNote(action.note);
        if (!pageId || !note) break;
        hooks.pageNote(pageId, note);
        break;
      }
      case "askSession": {
        const pageId = text(action.pageId);
        const question = text(action.question);
        if (!pageId || !question) break;
        const blockId = text(action.blockId);
        hooks.askSession(pageId, question, blockId || undefined);
        break;
      }
      default:
        break;
    }
  }

  return applied;
}
