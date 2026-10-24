import { useContext } from "react";
import { WorkspaceContext, type WorkspaceContextValue } from "./context";

/**
 * 取当前工作区。
 *
 * 单独成文件而不是和 Provider 放一起：组件与 hook 混在一个模块会让
 * react-refresh 无法保持状态（只能整页刷新），拆开后就不需要 disable 规则。
 */
export function useWorkspace(): WorkspaceContextValue {
  const ctx = useContext(WorkspaceContext);
  if (!ctx) throw new Error("useWorkspace 必须在 WorkspaceProvider 内使用");
  return ctx;
}
