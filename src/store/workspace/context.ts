import { createContext } from "react";
import type { PageMeasure, Person, Resume, ResumePage, Workspace } from "@/types";
import type { Action } from "./types";

export type SaveState = "idle" | "saving" | "saved";

export interface WorkspaceContextValue {
  state: Workspace;
  dispatch: React.Dispatch<Action>;
  /**
   * 内容写入的唯一入口（agent 的 `mutate` 信封走这里，用户点击走 dispatch）。
   *
   * 它比 dispatch 多做三件事：把「删除类」整批挂起、弹确认窗、按原顺序落地。
   * 会话面板的 agent 指令必须调它，否则同一条行为会出现两种确认策略。
   */
  applyOps: (ops: Action[]) => void;
  person: Person | null;
  resume: Resume | null;
  page: ResumePage | null;
  /**
   * 当前页的实测结果，来自常驻离屏宿主 —— 页面页数的**唯一真值源**。
   * 不落库：它是「这一版 DOM 此刻多高」，刷新即失效。
   */
  measure: PageMeasure | null;
  /**
   * 任意一页的实测结果（键是 pageId）。离屏宿主每页一份：
   * 会话的 `chat_send` 可能问的是手上那一页，不是用户此刻看的那页。
   * 没量到 / 超出量测上限的页返回 null —— 不拿别的页凑数。
   */
  measureOf: (pageId: string) => PageMeasure | null;
  activePageId: string | null;
  setActivePageId: (id: string | null) => void;
  /** 页签栏的快速检索列表：最近用过的在前，当前页一定在首位 */
  openPages: string[];
  /** 切到某页并把它顶到最前 */
  openPage: (id: string) => void;
  /** 换候选人：同时把他的第一份简历设为当前简历 */
  openPerson: (personId: string) => void;
  /** 换简历（同一个人的另一份）：当前页落到它的第一页 */
  openResume: (personId: string, resumeId: string) => void;
  /** 关标签：只从快速检索里移除，不删页 */
  closePage: (id: string) => void;
  expanded: Set<string>;
  toggleExpanded: (personId: string) => void;
  saveState: SaveState;
  ready: boolean;
  /** 实际生效的 UI 字号倍率（已合并系统设置与用户档位） */
  textScale: number;
  /** 系统「文本大小」给出的倍率，已含可读下限 */
  systemTextScale: number;
  setTextScale: (value: number | null) => void;
}

export const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);
