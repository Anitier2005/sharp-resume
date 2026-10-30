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
import type { Block, Resume, Workspace } from "@/types";
import { AUTO_TEXT_SCALE_FLOOR, displayName } from "@/types";
import { EMPTY_WORKSPACE } from "@/data/empty";
import { loadWorkspace, saveWorkspace } from "@/lib/storage";
import { usePaperMeasures } from "@/lib/measure";
import { A4_W_PX } from "@/lib/paper-size";
import { chatBridge, isDestructiveOp, needsConfirm } from "@/lib/agent-actions";
import { uid } from "@/lib/uid";
import { Confirm } from "@/components/ui/confirm";
import { Paper } from "@/components/resume/paper";
import paperStyles from "@/components/resume/paper.module.css";
import { reducer } from "./reducer";
import type { Action } from "./types";
import { WorkspaceContext, type WorkspaceContextValue, type SaveState } from "./context";

/**
 * 初始工作区：空白起步。
 *
 * 曾经有「DEV 就载入示例数据」和 `?seed=1` 两个入口，都是给浏览器调试模式看的；
 * 那个模式已经删掉，示例数据也一并停了 —— 需要样例就在界面里自己建。
 */
const INITIAL_WORKSPACE: Workspace = EMPTY_WORKSPACE;

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
   离屏实测上限
   ========================================================================== */

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
  const firstRun = useRef(true);
  /** 等用户点头的写入批次，FIFO：一次只显示一个确认窗，后面来的排队不丢 */
  const [batches, setBatches] = useState<PendingBatch[]>([]);

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
   * 为什么不是只量当前页：会话自己排版，`chat_send` 带的 `measure` 是它唯一
   * 能知道「这一页压到几页了」的来源；它可能问的是手上那一页而不是用户此刻
   * 看的那页，所以每一页都得有自己的实测。
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
    textScale,
    systemTextScale,
    setTextScale: (value: number | null) =>
      dispatch({ type: "setTextScale", textScale: value }),
  };

  // 确认窗挂在 provider 上而不是某个面板里：触发它的可能是会话 agent，
  // 面板卸载、换列之后这条挂起仍然得在
  const pending = batches[0] ?? null;

  return (
    <WorkspaceContext.Provider value={value}>
      {children}

      {/* 编辑态没有纸面 DOM（面板只在预览分支挂 .paper），而默认视图就是编辑。
          实测只能从这份离屏实例取；预览那份只管显示，不许另起一套公式。
          每页一份：会话的 chat_send 可能问的是任意一页，只留当前页的话别的页永远量不到。
          宽度必须固定：纸是 1:1 的 A4（210mm = 793.7px），跟着面板变就把
          字号与分页一起量歪了 —— 以前这里写 374，等于在缩略纸上量真 A4 的内容，
          页数会大出约一倍。data-print="hide"：打印只留纸面元素，不藏这份就会
          多打出同样几页。 */}
      {person && measurePages.length > 0 && (
        <div
          ref={measureHost}
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
