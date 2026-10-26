import type { Workspace } from "@/types";

/**
 * 全新安装时的空白工作区：不含任何示例数据。
 *
 * 发布产物必须从这里起步 —— 用户拿到的是一套干净软件，
 * 而不是别人的简历样板。示例数据只在开发构建里注入（见 store/workspace.tsx）。
 */
export const EMPTY_WORKSPACE: Workspace = {
  version: 4,
  people: [],
  ui: {
    accent: "white",
    mode: "light",
    activePersonId: null,
    activeResumeId: null,
    expanded: [],
  },
};
