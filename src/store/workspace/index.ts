/**
 * 工作区状态的对外入口。
 *
 * 分层：
 *   types.ts    Action 定义（状态机契约）
 *   helpers.ts  纯工具（展开态、时间戳、列表映射、新建页）
 *   reducer.ts  纯 reducer —— 所有状态变换集中在此，可单测
 *   context.ts  Context 与值类型
 *   provider.tsx  React 胶水：载入、自动保存、字号缩放、派生当前人/简历/页
 *
 * 外部一律 `import { useWorkspace } from "@/store/workspace"`，不感知内部拆分。
 */
export { WorkspaceProvider } from "./provider";
export { useWorkspace } from "./use-workspace";
export type { SaveState, WorkspaceContextValue } from "./context";
export type { Action } from "./types";
export { reducer } from "./reducer";
export { uid } from "@/lib/uid";
