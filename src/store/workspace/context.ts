import { createContext } from "react";
import type { Person, Resume, ResumePage, Workspace } from "@/types";
import type { Action } from "./types";

export type SaveState = "idle" | "saving" | "saved";

export interface WorkspaceContextValue {
  state: Workspace;
  dispatch: React.Dispatch<Action>;
  /**
   * 内容写入的唯一入口（agent 的 `mutate` 信封走这里，用户点击走 dispatch）。
   *
   * 它比 dispatch 多做三件事：把「删除类」整批挂起、弹确认窗、按原顺序落地。
   * 会话面板与排版事件都必须调它，否则同一条行为会出现两种确认策略。
   */
  applyOps: (ops: Action[]) => void;
  person: Person | null;
  resume: Resume | null;
  page: ResumePage | null;
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
  /**
   * 页排版 agent 此刻在忙哪一页、忙什么；null = 闲着。
   * 纯界面状态，不落盘：它是「正在发生的事」，不是数据。
   */
  layoutStatus: { pageId: string; text: string } | null;
  /** 实际生效的 UI 字号倍率（已合并系统设置与用户档位） */
  textScale: number;
  /** 系统「文本大小」给出的倍率，已含可读下限 */
  systemTextScale: number;
  setTextScale: (value: number | null) => void;
}

export const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);
