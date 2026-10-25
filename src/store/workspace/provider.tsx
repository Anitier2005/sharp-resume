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
import type { Workspace } from "@/types";
import { AUTO_TEXT_SCALE_FLOOR } from "@/types";
import { SEED } from "@/data/seed";
import { EMPTY_WORKSPACE } from "@/data/empty";
import { loadWorkspace, saveWorkspace, isTauri } from "@/lib/storage";
import { reducer } from "./reducer";
import { WorkspaceContext, type WorkspaceContextValue, type SaveState } from "./context";

/**
 * 初始工作区：空白起步，跟正式版一致。
 *
 * 以前是「DEV 就给示例数据」，结果 dev 窗口当正式工作区用时，没有存档就会载入
 * 8 个虚构候选人和一段假的 AI 改写对话，一操作还落盘，用户会以为自己的数据
 * 被一堆「不存在的会话」占了。现在改成显式索取：
 *   http://localhost:1420/?seed=1   → 载入示例数据（调样式用）
 *   桌面 dev 窗口 / 不带参数          → 空白，跟生产一致
 *
 * vite.config 在 production 下把 @/data/seed 重定向到 seed.prod（空实现），
 * 所以发布产物里永远不含示例简历，带 ?seed=1 也只会得到空白工作区。
 */
const wantsSeed =
  import.meta.env.DEV && new URLSearchParams(window.location.search).has("seed");
const INITIAL_WORKSPACE: Workspace = wantsSeed ? SEED : EMPTY_WORKSPACE;

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, INITIAL_WORKSPACE);
  const [ready, setReady] = useState(false);
  // 上次落盘的快照：与 state 不是同一个对象即说明有未保存的改动，由它派生保存状态
  const [savedSnapshot, setSavedSnapshot] = useState<Workspace | null>(null);
  const [activePageId, setActivePageId] = useState<string | null>(null);
  const firstRun = useRef(true);

  // 启动时载入
  useEffect(() => {
    let alive = true;
    loadWorkspace().then((saved) => {
      if (!alive) return;
      if (saved && saved.version === 3 && saved.people?.length) {
        dispatch({ type: "hydrate", workspace: saved });
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
  // 非桌面端（浏览器预览）拿不到，用可读下限兜底。
  const [systemTextScale, setSystemTextScale] = useState(AUTO_TEXT_SCALE_FLOOR);
  useEffect(() => {
    if (!isTauri()) return;
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
    person,
    resume,
    page,
    activePageId: resolvedPageId,
    setActivePageId,
    expanded,
    toggleExpanded,
    saveState,
    ready,
    textScale,
    systemTextScale,
    setTextScale: (value: number | null) =>
      dispatch({ type: "setTextScale", textScale: value }),
  };

  return <WorkspaceContext.Provider value={value}>{children}</WorkspaceContext.Provider>;
}
