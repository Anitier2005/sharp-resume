import { createContext } from "react";
import type { PageMeasure, Person, Resume, ResumePage, Workspace } from "@/types";
import type { Action } from "./types";

export type SaveState = "idle" | "saving" | "saved";

/**
 * 排版这一轮里「正在发生的事」是哪一类。
 * `start` 是本地造的（后端只发后面三种），用来给秒表一个起点。
 */
export type LayoutKind = "start" | "tool_start" | "tool_end" | "thinking";

/**
 * 页排版 agent 此刻在忙哪一页、忙什么。纯界面状态，不落盘。
 *
 * `since` 与 `step` 是给用户看的「还活着」凭证：文案两次变化之间可能隔几十秒，
 * 秒表每秒在走、步数只在真收到进度时才涨 —— 两者都不许由界面自己编。
 */
export interface LayoutStatus {
  pageId: string;
  text: string;
  /** 这一轮开跑的时刻（ms）；后续进度只更新文案，不重置它 */
  since: number;
  /** 收到的进度条数：工具开始/结束、每 60 字思考各算一步 */
  step: number;
  kind: LayoutKind;
}

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
  /**
   * 当前页的实测结果，来自常驻离屏宿主 —— 页面页数的**唯一真值源**。
   * 不落库：它是「这一版 DOM 此刻多高」，刷新即失效。
   */
  measure: PageMeasure | null;
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
  layoutStatus: LayoutStatus | null;
  /** 实际生效的 UI 字号倍率（已合并系统设置与用户档位） */
  textScale: number;
  /** 系统「文本大小」给出的倍率，已含可读下限 */
  systemTextScale: number;
  setTextScale: (value: number | null) => void;
}

export const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);
