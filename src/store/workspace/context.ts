import { createContext } from "react";
import type { Person, Resume, ResumePage, Workspace } from "@/types";
import type { Action } from "./types";

export type SaveState = "idle" | "saving" | "saved";

export interface WorkspaceContextValue {
  state: Workspace;
  dispatch: React.Dispatch<Action>;
  person: Person | null;
  resume: Resume | null;
  page: ResumePage | null;
  activePageId: string | null;
  setActivePageId: (id: string | null) => void;
  /** 页签栏的快速检索列表：最近用过的在前，当前页一定在首位 */
  openPages: string[];
  /** 切到某页并把它顶到最前 */
  openPage: (id: string) => void;
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
