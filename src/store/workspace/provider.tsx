import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { listen } from "@tauri-apps/api/event";
import type { Asset, Block, PageNote, Resume, Workspace } from "@/types";
import { AUTO_TEXT_SCALE_FLOOR, displayName } from "@/types";
import { EMPTY_WORKSPACE } from "@/data/empty";
import { loadWorkspace, saveWorkspace } from "@/lib/storage";
import { describeChange, diff, flatten, type Flat } from "@/lib/changes";
import { submitLayout } from "@/lib/layout";
import { originOf, TOLERANCE_MAX, usePaperMeasures } from "@/lib/measure";
import { A4_W_PX } from "@/lib/paper-size";
import {
  applyAgentActions,
  chatBridge,
  isDestructiveOp,
  layoutQuestion,
  needsConfirm,
} from "@/lib/agent-actions";
import { uid } from "@/lib/uid";
import { useToast } from "@/components/ui/toast-context";
import { Confirm } from "@/components/ui/confirm";
import { Paper } from "@/components/resume/paper";
import paperStyles from "@/components/resume/paper.module.css";
import { reducer } from "./reducer";
import type { Action } from "./types";
import { WorkspaceContext, type LayoutKind, type LayoutStatus, type WorkspaceContextValue, type SaveState } from "./context";

/**
 * 初始工作区：空白起步。
 *
 * 曾经有「DEV 就载入示例数据」和 `?seed=1` 两个入口，都是给浏览器调试模式看的；
 * 那个模式已经删掉，示例数据也一并停了 —— 需要样例就在界面里自己建。
 */
const INITIAL_WORKSPACE: Workspace = EMPTY_WORKSPACE;

/** 后端排版跑完后推回来的动作 */
interface LayoutApplied {
  personId: string;
  resumeId: string;
  pageId: string;
  actions: unknown[];
  /** 本轮工具新产出的材料（图片转化）。与动作一起到，因为它们可能互为因果 */
  assets?: Asset[];
}

/* ==========================================================================
   确认门：把「要删什么」翻译成人话
   ========================================================================== */

/**
 * 一批等用户点头的写入。
 *
 * 为什么整批挂起而不是只挂起删除那几条：这一批是模型一次决策的产物，
 * 拆开执行会出现「A 删了、B 因为 A 没了而失败」的半途状态，用户看到的
 * 也不像自己确认过的那件事。
 */
interface PendingBatch {
  ops: Action[];
  /** 删除类操作的具体对象，一条一行 */
  items: string[];
  /** 同一批里不删除的改动条数：确认后它们一起生效 */
  extra: number;
}

function findResume(state: Workspace, personId: string, resumeId: string): Resume | null {
  const person = state.people.find((item) => item.id === personId);
  return person?.resumes.find((item) => item.id === resumeId) ?? null;
}

/** 块的人话名：确认窗里写「分区「工作经历」（含 3 条）」比「block b7」有用得多 */
function describeBlock(block: Block | undefined): string {
  if (!block) return "这一块";
  if (block.kind === "section") {
    const title = block.title?.trim() || "未命名分区";
    const count = block.items?.length ?? 0;
    return count ? `分区「${title}」（含 ${count} 条）` : `分区「${title}」`;
  }
  if (block.kind === "media") {
    const caption = block.caption?.trim();
    return caption ? `图片块「${caption}」` : "一个图片块";
  }
  const count = block.fields?.length ?? 0;
  return count ? `字段组（${count} 项）` : "一组字段";
}

/**
 * 反查当前工作区，把删除类 op 说成用户认得出的东西。
 *
 * 只读 state、不猜：查不到就退回一个不带细节的说法 —— 确认窗里的描述
 * 必须比「2 项」更有信息量，但也不能编。
 */
function describeDeletions(state: Workspace, ops: Action[]): string[] {
  const out: string[] = [];
  for (const op of ops) {
    if (!isDestructiveOp(op)) continue;
    switch (op.type) {
      case "removePage": {
        const resume = findResume(state, op.personId, op.resumeId);
        const index = resume?.pages.findIndex((page) => page.id === op.pageId) ?? -1;
        if (index < 0) {
          out.push("一页（已经找不到了）");
          break;
        }
        const label = resume?.pages[index]?.label?.trim();
        out.push(`第 ${index + 1} 页${label ? `「${label}」` : "（未命名页）"}`);
        break;
      }
      case "deleteResume": {
        const resume = findResume(state, op.personId, op.resumeId);
        const pages = resume?.pages.length ?? 0;
        out.push(`简历「${resume?.title || "未命名简历"}」和它的 ${pages} 页`);
        break;
      }
      case "deletePerson": {
        const person = state.people.find((item) => item.id === op.personId);
        out.push(
          person
            ? `候选人「${displayName(person.fields)}」的全部简历与经历`
            : "一位候选人（已经找不到了）",
        );
        break;
      }
      case "removeBlock":
      case "removeSection": {
        // removeSection 删的是整个分区（连同条目），描述要和删一个块区分开
        const targetId = op.type === "removeBlock" ? op.blockId : op.sectionId;
        const page = findResume(state, op.personId, op.resumeId)?.pages.find(
          (item) => item.id === op.pageId,
        );
        const block = page?.blocks?.find((item) => item.id === targetId);
        out.push(op.type === "removeSection" ? `整个${describeBlock(block)}` : describeBlock(block));
        break;
      }
      default:
        break;
    }
  }
  return out;
}


/* ==========================================================================
   自动自查（F-010）
   ========================================================================== */

/** 同一页同一件事最多自动投几轮，再压不动就该人决定了 */
const AUTO_ROUNDS_MAX = 3;

/** 一帧里的多次测量只投一次：排版跑一轮是模型调用，不能每帧都花一次 */
const AUTO_DEBOUNCE_MS = 300;

/**
 * 离屏实测的最多页数：每页是一张真 A4（793.7px 宽）的完整渲染，
 * 页数再多会拖慢编辑。超出的页实测为空（诚实地说量不到），不悄悄少。
 */
const MEASURE_MAX_PAGES = 8;

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, INITIAL_WORKSPACE);
  const [ready, setReady] = useState(false);
  // 上次落盘的快照：与 state 不是同一个对象即说明有未保存的改动，由它派生保存状态
  const [savedSnapshot, setSavedSnapshot] = useState<Workspace | null>(null);
  const [activePageId, setActivePageId] = useState<string | null>(null);
  /** 排版 agent 正在忙的那一页与它手上的动作；纯界面状态，不落盘 */
  const [layoutStatus, setLayoutStatus] = useState<LayoutStatus | null>(null);
  const firstRun = useRef(true);
  const toast = useToast();
  /** 等用户点头的写入批次，FIFO：一次只显示一个确认窗，后面来的排队不丢 */
  const [batches, setBatches] = useState<PendingBatch[]>([]);
  /** 排版提问的防抖表：同一页 5 秒内只让会话 agent 跑一轮，防止两边互相叫起来 */
  const askAt = useRef(new Map<string, number>());

  // 启动时载入
  useEffect(() => {
    let alive = true;
    loadWorkspace().then((saved) => {
      if (!alive) return;
      // migrate 之后一定是最新结构，这里不再判断版本号（判断会随版本升级而失效）
      if (saved && saved.people?.length) {
        dispatch({ type: "hydrate", workspace: saved });
        // 上次看的是哪一页没有单独存，就是快速检索的第一个；
        // 它不属于当前简历时 resolvedPageId 会自己退回第一页
        setActivePageId(saved.ui.openPages?.[0] ?? null);
        setSavedSnapshot(saved);
      } else {
        setSavedSnapshot(INITIAL_WORKSPACE);
      }
      setReady(true);
    });
    return () => {
      alive = false;
    };
  }, []);

  const person = useMemo(
    () => state.people.find((item) => item.id === state.ui.activePersonId) ?? state.people[0] ?? null,
    [state.people, state.ui.activePersonId],
  );

  const resume = useMemo(() => {
    if (!person) return null;
    return person.resumes.find((item) => item.id === state.ui.activeResumeId) ?? person.resumes[0] ?? null;
  }, [person, state.ui.activeResumeId]);

  // 当前页：切简历时自动落到第一页
  const resolvedPageId = useMemo(() => {
    if (!resume) return null;
    if (activePageId && resume.pages.some((page) => page.id === activePageId)) return activePageId;
    return resume.pages[0]?.id ?? null;
  }, [resume, activePageId]);

  const page = useMemo(
    () => resume?.pages.find((item) => item.id === resolvedPageId) ?? null,
    [resume, resolvedPageId],
  );

  /**
   * 离屏测量宿主：每页一份 `.wrap > Paper`，实测只从这里取，预览那份只管显示。
   *
   * 为什么不是只量当前页：内容变更触发的那条路会遍历 `resume.pages`，
   * 非当前页也得拿得到自己的实测 —— 否则模型永远在「这一页还没量到」。
   */
  const measureHost = useRef<HTMLDivElement | null>(null);
  /** 上一次提示过的总页数：只在超过上限且页数变化时提示一次，别每次敲键都刷 console */
  const measureCapWarned = useRef(0);
  const measurePages = useMemo(() => {
    const all = resume?.pages ?? [];
    if (all.length > MEASURE_MAX_PAGES) {
      // 不悄悄少：页数超上限时明说只量了前几页，剩下那些页的实测就是 null
      if (measureCapWarned.current !== all.length) {
        measureCapWarned.current = all.length;
        console.warn(
          `[measure] 这份简历有 ${all.length} 页，离屏实测上限是 ${MEASURE_MAX_PAGES} 页，只量前 ${MEASURE_MAX_PAGES} 页；其余页的实测为空。`,
        );
      }
      return all.slice(0, MEASURE_MAX_PAGES);
    }
    measureCapWarned.current = 0;
    return all;
  }, [resume]);

  const measures = usePaperMeasures({ hostRef: measureHost, person, pages: measurePages });

  /** 当前页那一份：对外语义与旧版一致（只保证当前页，别的页用 measureOf） */
  const measure = page ? (measures[page.id] ?? null) : null;

  const measureOf = useCallback((pageId: string) => measures[pageId] ?? null, [measures]);

  /**
   * 内容变更那条路挂在 500ms 防抖里：把 measureOf 放 ref，
   * 免得每次量出一个新数就把防抖计时器重置一遍。
   */
  const measureOfRef = useRef(measureOf);
  measureOfRef.current = measureOf;

  /**
   * 页签栏用的「快速检索」列表（MRU）：
   *   1. 只留当前简历里还在的页 —— 删页、切简历后不会留下找不到的 id
   *   2. 当前页永远排第一 —— 切简历、删页都会有一个自动落点，
   *      如果它不在列表里，页签栏就会出现「一个都没选中」的样子
   *   3. 老数据没有 openPages 时把全部页当列表
   */
  const openPages = useMemo(() => {
    if (!resume) return [];
    const known = new Set(resume.pages.map((item) => item.id));
    const list: string[] = [];
    for (const id of state.ui.openPages ?? resume.pages.map((item) => item.id)) {
      if (!known.has(id) || list.includes(id)) continue;
      list.push(id);
    }
    if (resolvedPageId) {
      const at = list.indexOf(resolvedPageId);
      if (at > 0) list.splice(at, 1);
      if (at !== 0) list.unshift(resolvedPageId);
    }
    return list;
  }, [resume, state.ui.openPages, resolvedPageId]);

  // 当前页一变就把它顶到最前（点页签、新建页、切简历、删页都走这里），
  // 顺带把顺序落盘，下次启动仍是最近用过的排前面
  useEffect(() => {
    if (!resolvedPageId) return;
    const stored = state.ui.openPages;
    if (stored?.[0] === resolvedPageId) return;
    const rest = (stored ?? resume?.pages.map((item) => item.id) ?? []).filter(
      (id) => id !== resolvedPageId,
    );
    dispatch({ type: "patchUi", patch: { openPages: [resolvedPageId, ...rest] } });
  }, [resolvedPageId, resume, state.ui.openPages]);

  /** 切页 + 顶到快速检索最前 */
  const openPage = useCallback(
    (id: string) => {
      setActivePageId(id);
      const stored = state.ui.openPages ?? resume?.pages.map((item) => item.id) ?? [];
      dispatch({
        type: "patchUi",
        patch: { openPages: [id, ...stored.filter((item) => item !== id)] },
      });
    },
    [resume, state.ui.openPages],
  );

  /**
   * 关标签 = 只从快速检索里移除，页本身不动。
   * 关的是当前页时退回列表里下一个最近用过的；列表空了就退回简历第一页。
   */
  const closePage = useCallback(
    (id: string) => {
      if (!resume) return;
      // 必须基于 store 里的原始列表：`openPages` 是派生值，已经按当前简历过滤过，
      // 写回去会把别的简历攒下的检索顺序抹掉
      const stored = state.ui.openPages ?? openPages;
      const rest = stored.filter((item) => item !== id);
      dispatch({ type: "patchUi", patch: { openPages: rest } });
      if (id !== resolvedPageId) return;
      const next =
        openPages.find((item) => item !== id) ??
        resume.pages.find((item) => item.id !== id)?.id ??
        null;
      if (next) setActivePageId(next);
    },
    [openPages, state.ui.openPages, resolvedPageId, resume],
  );

  /**
   * 换候选人：简历必须一起落。
   *
   * 只改 activePersonId 的话，`resume` 会退回「这个人的第一份」，但落盘的
   * ui.activeResumeId 还停在上一个人身上 —— 下次启动、以及任何读 store 的地方
   * （比如工具层的只读镜像）都会指向一份不属于当前人的简历。
   */
  const openPerson = useCallback(
    (personId: string) => {
      const target = state.people.find((item) => item.id === personId);
      const first = target?.resumes[0];
      if (!target || !first) return;
      dispatch({ type: "selectResume", personId: target.id, resumeId: first.id });
      // 视图里的当前页也得跟着走，否则 resolvedPageId 要先退回旧页再回退
      setActivePageId(first.pages[0]?.id ?? null);
    },
    [state.people],
  );

  /** 换简历：当前页落到这份简历的第一页（别把上一份的页 id 带过来） */
  const openResume = useCallback(
    (personId: string, resumeId: string) => {
      const target = state.people.find((item) => item.id === personId);
      const next = target?.resumes.find((item) => item.id === resumeId);
      if (!target || !next) return;
      dispatch({ type: "selectResume", personId: target.id, resumeId: next.id });
      setActivePageId(next.pages[0]?.id ?? null);
    },
    [state.people],
  );

  /* ==========================================================================
     内容写入通道 + 确认门
     ========================================================================== */

  /** 按数组顺序落地；不做合并 —— 顺序即语义（契约 1.1） */
  const runOps = useCallback(
    (ops: Action[]) => {
      for (const op of ops) dispatch(op);
      // addPage{open} 的「视图」那一半：reducer 已把新页顶到 openPages[0]，
      // 真正的当前页在 provider 的视图态里，得单独跟过去
      for (const op of ops) {
        if (op.type === "addPage" && op.open && op.id) openPage(op.id);
      }
    },
    [openPage],
  );

  /**
   * agent 的写入入口。用户点击走 dispatch，agent 必须走这里：
   * 它多一层确认门，行为才和「用户自己点」一致。
   */
  const applyOps = useCallback(
    (ops: Action[]) => {
      if (!ops.length) return;
      // 需要 open 的 addPage 在这里就把 id 定下来：确认通过后才 dispatch，
      // 而 reducer 里生成的 id 外面拿不到，`open` 就没法落点
      const prepared: Action[] = ops.map((op) =>
        op.type === "addPage" && op.open && !op.id ? { ...op, id: uid("pg") } : op,
      );
      if (!needsConfirm(prepared)) {
        runOps(prepared);
        return;
      }
      const items = describeDeletions(state, prepared);
      // 整批挂起：确认后按原顺序一次执行，取消则一条都不落地
      setBatches((list) => [
        ...list,
        { ops: prepared, items, extra: prepared.length - items.length },
      ]);
    },
    [state, runOps],
  );

  const confirmBatch = () => {
    const head = batches[0];
    if (!head) return;
    runOps(head.ops);
    setBatches((list) => list.slice(1));
  };

  /**
   * 取消：整批丢弃，并在会话里留一句话。
   *
   * 为什么要留话：模型并不知道用户点了「取消」（工具的返回文案是「已提交，待确认」），
   * 下一次它读到这段历史才知道那些改动没生效，否则会以为已经写进去了又去改一遍。
   */
  const cancelBatch = () => {
    const head = batches[0];
    if (!head) return;
    setBatches((list) => list.slice(1));
    // 整批都是删除时说「删除第 2 页…」，混着其他修改就不加动词，免得读起来像只删了它
    const verb = head.items.length === head.ops.length ? "删除" : "";
    const summary = head.items.length
      ? `${verb}${head.items[0]} 等 ${head.ops.length} 项改动`
      : `${head.ops.length} 项改动`;
    const notice = `（已取消：${summary}）`;
    const bridge = chatBridge();
    if (bridge) {
      bridge.notice(notice);
      return;
    }
    // 面板没挂上（极端情况）时退回工作区会话，至少让用户看得见
    if (person) {
      dispatch({
        type: "appendMessage",
        personId: person.id,
        message: { id: uid("m"), role: "user", system: true, content: notice, time: Date.now() },
      });
    }
  };

  /** 排版 agent 留的批注：按页落进数据，纸面自己负责贴到块上 */
  const appendNote = useCallback(
    (personId: string, resumeId: string, pageId: string, note: PageNote) => {
      dispatch({ type: "pageNote", personId, resumeId, pageId, note });
    },
    [],
  );

  /**
   * 排版 agent 向会话 agent 提问。
   *
   * 前端负责三件事：落成一条会话消息、让会话 agent 自动跑一轮、防重
   * （同一页 5 秒内只触发一次 —— 两个 agent 互相呼叫很容易打成死循环，
   * 契约 4 只限制后端单轮次数，这里的节流是最后一道闸）。
   */
  const askSession = useCallback(
    (pageId: string, question: string, blockId?: string) => {
      const now = Date.now();
      if (now - (askAt.current.get(pageId) ?? 0) < 5000) return;
      askAt.current.set(pageId, now);

      const bridge = chatBridge();
      if (!bridge) {
        console.warn("[layout] 会话面板还没准备好，这次提问被跳过", question);
        return;
      }
      const index = resume?.pages.findIndex((item) => item.id === pageId) ?? -1;
      bridge.turn(pageId, layoutQuestion(index + 1, question, blockId));
    },
    [resume],
  );

  /**
   * 监听器里要用「最新一版」的这些函数，但它们随 state 每次变化。
   * 放进 ref 而不是进依赖：否则每敲一个字就重订阅一遍 Tauri 事件。
   */
  const handlers = useRef({ applyOps, appendNote, askSession, openPerson, openResume });
  useEffect(() => {
    handlers.current = { applyOps, appendNote, askSession, openPerson, openResume };
  });

  // 自动保存：改动后 500ms 落盘。保存状态由「快照是否还是当前 state」派生，不在 effect 里同步 setState
  useEffect(() => {
    if (!ready) return;
    if (firstRun.current) {
      firstRun.current = false;
      return;
    }
    const timer = window.setTimeout(() => {
      saveWorkspace(state)
        .then(() => setSavedSnapshot(state))
        .catch((error) => {
          console.error("[storage] 保存失败", error);
        });
    }, 500);
    return () => window.clearTimeout(timer);
  }, [state, ready]);

  /**
   * 这一页的内容一变就告诉排版 agent。
   *
   * 为什么挂在保存节拍上（500ms 防抖）：敲字过程中每个中间态都报一遍没有意义，
   * 排版看的是这一页最终的形状，不是打字打到一半的样子。
   *
   * 为什么只报内容、不报样式：排版 agent 改完样式会触发一次 state 变化，
   * 要是连样式一起报，它就会被自己的改动叫醒，一圈一圈转下去。
   */
  const layoutBaseline = useRef(new Map<string, Flat>());

  useEffect(() => {
    if (!ready || !person || !resume) return;
    const timer = window.setTimeout(() => {
      const alive = new Set(resume.pages.map((item) => item.id));
      // 删掉的页连基线一起清，免得 id 被复用时误判成「改了一大堆」
      for (const id of [...layoutBaseline.current.keys()]) {
        if (!alive.has(id)) layoutBaseline.current.delete(id);
      }

      for (const target of resume.pages) {
        const flat = flatten(person, target);
        // 人物级字段不属于这一页：改个名字不该惊动每一页的重排
        for (const key of Object.keys(flat)) {
          if (key.startsWith("person:")) delete flat[key];
        }
        const before = layoutBaseline.current.get(target.id);
        layoutBaseline.current.set(target.id, flat);
        // 第一次见到这一页（刚打开工作区、刚切到这份简历）只记基线，不触发
        if (!before) continue;

        const deltas = diff(before, flat);
        if (!deltas.length) continue;
        void submitLayout({
          person,
          resume,
          page: target,
          state,
          kind: "change",
          text: describeChange(deltas),
          // 内容一变，这一页上一次的实测就作废了；但「量不到」也要如实上送，
          // 不带的话 Rust 侧拿到 None，只能报「还没量到」—— 现在至少是这一页自己的数
          measure: measureOfRef.current(target.id),
        });
      }
    }, 500);
    return () => window.clearTimeout(timer);
  }, [state, ready, person, resume]);

  /**
   * 排版结果：后端跑完推过来，在这里落进 store。
   *
   * 放在 provider 而不是右侧面板：排版是后台跑的，用户很可能已经切到别的页，
   * 面板卸载了也得收得到，否则这一轮就白跑了。
   */
  useEffect(() => {
    let alive = true;
    const stops: Array<() => void> = [];
    const keep = (unlisten: () => void) => {
      if (alive) stops.push(unlisten);
      else unlisten();
    };

    void listen<LayoutApplied>("layout:applied", (event) => {
      if (!alive) return;
      const { personId, resumeId, actions, assets } = event.payload;
      // 材料先落地：动作里可能马上引用它（模型常在同一轮里转完图就写进 HTML）
      if (Array.isArray(assets) && assets.length) {
        dispatch({ type: "addAssets", personId, assets });
      }
      if (!Array.isArray(actions) || !actions.length) return;
      applyAgentActions(actions, {
        // 排版事件也走这一条分发器：新增指令只需要在这里补一次，
        // 不必为「会话来的」和「排版来的」各维护一套分支
        openPage: (pageId) => setActivePageId(pageId),
        openPerson: (personId) => handlers.current.openPerson(personId),
        openResume: (personId, resumeId) => handlers.current.openResume(personId, resumeId),
        patchUi: (patch) => dispatch({ type: "patchUi", patch }),
        ask: () => {},
        pageStyle: (pageId, style) =>
          dispatch({ type: "pageStyle", personId, resumeId, pageId, style }),
        pageLayout: (pageId, layout) =>
          dispatch({ type: "pageLayout", personId, resumeId, pageId, layout }),
        requestLayout: () => {},
        // 写入走同一条确认门（排版 agent 不该写内容，但信封形状是共用的）
        applyOps: (ops) => handlers.current.applyOps(ops),
        pageNote: (pageId, note) =>
          handlers.current.appendNote(personId, resumeId, pageId, note),
        askSession: (pageId, question, blockId) =>
          handlers.current.askSession(pageId, question, blockId),
      });
    }).then(keep);

    void listen<{ pageId: string }>("layout:started", (event) => {
      if (!alive) return;
      const { pageId } = event.payload;
      // 同一页已经在显示就别换对象：每轮都换会让状态栏闪一下。
      // 换页了才重开秒表 —— 秒表量的是「这一页这一轮跑了多久」。
      setLayoutStatus((prev) =>
        prev?.pageId === pageId
          ? prev
          : { pageId, text: "正在看这一页的版式", since: Date.now(), step: 0, kind: "start" },
      );
    }).then(keep);

    void listen<{
      pageId: string;
      kind?: LayoutKind;
      tool?: string;
      detail?: string;
      thoughtChars?: number;
    }>("layout:progress", (event) => {
      if (!alive) return;
      const { pageId, kind, detail, thoughtChars } = event.payload;
      setLayoutStatus((prev) => {
        // 正常情况下 started 先到；万一没收到，就当场补一个起点，别让状态栏空着
        const base: LayoutStatus =
          prev?.pageId === pageId
            ? prev
            : {
                pageId,
                text: "正在看这一页的版式",
                since: Date.now(),
                step: 0,
                kind: "start",
              };
        // 思考只给字数：后端每 60 字发一次，读得出「它还在想」就够，不必粘原文
        const text =
          kind === "thinking" ? `思考中 · 已想 ${thoughtChars ?? 0} 字` : (detail ?? base.text);
        return { pageId, text, since: base.since, step: base.step + 1, kind: kind ?? base.kind };
      });
    }).then(keep);

    void listen<{ pageId: string }>("layout:idle", (event) => {
      if (!alive) return;
      // 只清自己那一页：别页刚开始跑，不该被这里顺手抹掉
      setLayoutStatus((prev) => (prev?.pageId === event.payload.pageId ? null : prev));
    }).then(keep);

    void listen<{ pageId: string; error: string }>("layout:failed", (event) => {
      if (!alive) return;
      toast(`排版没跑成：${event.payload.error}`);
    }).then(keep);

    return () => {
      alive = false;
      for (const stop of stops) stop();
    };
  }, [dispatch, toast]);

  /** 自动自查已投了几轮：pageId → 轮数。只放 ref：它属于这一场编辑，不是数据 */
  const autoRounds = useRef(new Map<string, number>());

  /**
   * 溢出且有目标页数 → 让排版 agent 再压一轮，最多 3 轮；还不收敛就留一条批注让人决定。
   *
   * 全在前端做：Rust 侧没有 DOM，那边做定时轮询只会产出假数据。
   * 量不到时不投 —— 拿一个假数去指挥排版，比不投更糟。
   */
  useEffect(() => {
    if (!person || !resume || !page || !measure) return;
    if (measure.status !== "ok" || !measure.onePage || !measure.contentPx) return;
    // 排版正在跑这一页：不叠加，等它跑完自然会再看一眼
    if (layoutStatus?.pageId === page.id) return;
    const target = resume.targetPages;
    if (!target) return;

    const pages = measure.contentPx / measure.onePage;
    if (pages <= TOLERANCE_MAX) {
      // 收敛了就清零：下次再溢出从第 1 轮重新数
      autoRounds.current.delete(page.id);
      return;
    }

    const round = autoRounds.current.get(page.id) ?? 0;
    if (round >= AUTO_ROUNDS_MAX) {
      if (round === AUTO_ROUNDS_MAX) {
        autoRounds.current.set(page.id, AUTO_ROUNDS_MAX + 1);
        dispatch({
          type: "pageNote",
          personId: person.id,
          resumeId: resume.id,
          pageId: page.id,
          note: {
            id: `n-auto-${page.id}-4`,
            text: `自动自查已投 ${AUTO_ROUNDS_MAX} 轮，仍实测 ${pages.toFixed(2)} 页（目标 ${target} 页）。差 ${(pages - 1).toFixed(2)} 页，需要人决定：减内容还是放宽页数。`,
            at: Math.floor(Date.now() / 1000),
          },
        });
      }
      return;
    }

    const timer = window.setTimeout(() => {
      // 计数在真正投出去的那一刻加：被防抖取消掉的那几次不算一轮
      autoRounds.current.set(page.id, round + 1);
      void submitLayout({
        person,
        resume,
        page,
        state,
        kind: "request",
        // 实测值一起上送：不带的话排版 agent 看到的仍是「尚未实测」，
        // 自动压版就成了闭眼开方子。取的是这一页自己那份，不拿当前页凑数
        measure: measureOf(page.id),
        origin: originOf(resume.id),
        text: `自动自查：这一页实测 ${pages.toFixed(2)} 页，超出 ${(pages - 1).toFixed(2)} 页，目标是 ${target} 页。请继续压到目标页数内；压不动就说清差多少，不要谎报。`,
      });
    }, AUTO_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [measure, person, resume, page, state, layoutStatus, dispatch]);

  // 系统文本缩放：Windows「辅助功能 → 文本大小」不会自动传给网页，主动问一次。
  // 问不到时用可读下限兜底（老系统没这个设置）。
  const [systemTextScale, setSystemTextScale] = useState(AUTO_TEXT_SCALE_FLOOR);
  useEffect(() => {
    let alive = true;
    import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke<number>("text_scale"))
      .then((value) => {
        if (alive && Number.isFinite(value) && value > 0) setSystemTextScale(value);
      })
      .catch(() => {
        /* 读不到就用下限，不影响可用性 */
      });
    return () => {
      alive = false;
    };
  }, []);

  // 用户选过档位就听用户的，否则自动跟随系统（下限已在 Rust 侧保证）。
  const textScale = state.ui.textScale ?? systemTextScale;

  // 落到根字号上：所有 rem 字号随之缩放。
  // 用 layout effect 而不是 effect：在浏览器绘制前写入变量，避免档位切换时闪一帧旧字号。
  useLayoutEffect(() => {
    document.documentElement.style.setProperty("--text-scale", String(textScale));
  }, [textScale]);

  const saveState: SaveState = !ready ? "idle" : savedSnapshot === state ? "saved" : "saving";

  const expanded = useMemo(() => new Set(state.ui.expanded ?? []), [state.ui.expanded]);

  const toggleExpanded = useCallback(
    (personId: string) => dispatch({ type: "togglePerson", personId }),
    [],
  );

  const value: WorkspaceContextValue = {
    state,
    dispatch,
    applyOps,
    person,
    resume,
    page,
    measure,
    measureOf,
    activePageId: resolvedPageId,
    setActivePageId,
    openPages,
    openPage,
    openPerson,
    openResume,
    closePage,
    expanded,
    toggleExpanded,
    saveState,
    ready,
    layoutStatus,
    textScale,
    systemTextScale,
    setTextScale: (value: number | null) =>
      dispatch({ type: "setTextScale", textScale: value }),
  };

  // 确认窗挂在 provider 上而不是某个面板里：触发它的可能是会话 agent
  // （在会话列）也可能是排版 agent（在简历列），挂在哪一列都会在另一列失效
  const pending = batches[0] ?? null;

  return (
    <WorkspaceContext.Provider value={value}>
      {children}

      {/* 编辑态没有纸面 DOM（面板只在预览分支挂 .paper），而默认视图就是编辑。
          实测只能从这份离屏实例取；预览那份只管显示，不许另起一套公式。
          每页一份：内容变更的循环会遍历所有页，只留当前页的话别的页永远量不到。
          宽度必须固定：纸是 1:1 的 A4（210mm = 793.7px），跟着面板变就把
          字号与分页一起量歪了 —— 以前这里写 374，等于在缩略纸上量真 A4 的内容，
          页数会大出约一倍。data-print="hide"：打印只留纸面元素，不藏这份就会
          多打出同样几页。 */}
      {person && measurePages.length > 0 && (
        <div
          aria-hidden
          data-measure="host"
          data-print="hide"
          style={{
            position: "absolute",
            left: -9999,
            top: 0,
            width: A4_W_PX,
            visibility: "hidden",
            pointerEvents: "none",
            zIndex: -1,
          }}
        >
          {measurePages.map((target) => (
            <div key={target.id} className={paperStyles.wrap} data-page={target.id}>
              <Paper person={person} page={target} />
            </div>
          ))}
        </div>
      )}

      <Confirm
        open={!!pending}
        title={
          pending && pending.items.length === 1
            ? "这一步删掉就找不回来了"
            : `这批改动会删掉 ${pending?.items.length ?? 0} 项内容`
        }
        description={[
          "这是会话里提出的改动，和你在界面上点一样，要先确认。",
          pending && pending.extra > 0
            ? `同一批里另外 ${pending.extra} 项修改会在确认后一起生效。`
            : "",
        ]
          .filter(Boolean)
          .join("")}
        items={pending?.items}
        confirmLabel={pending && pending.items.length ? "删除并继续" : "继续"}
        onConfirm={confirmBatch}
        onCancel={cancelBatch}
      />
    </WorkspaceContext.Provider>
  );
}
