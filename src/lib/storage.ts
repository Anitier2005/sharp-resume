import { invoke } from "@tauri-apps/api/core";
import type { Workspace } from "@/types";
import { migrateWorkspace } from "@/lib/migrate";

/**
 * 工作区读写。真相只有一处：数据目录里的 workspace.json。
 *
 * 这里曾经有一条「不在 Tauri 里就退回 localStorage」的分支，配合浏览器调试模式。
 * 现在没有那个模式了（前端只跑在 Tauri 窗口里），存储也就只有一条路 ——
 * 两条存储路径最容易出现「这边改了那边没改」。
 */

export async function loadWorkspace(): Promise<Workspace | null> {
  try {
    const raw = await invoke<string | null>("read_state");
    if (!raw) return null;
    return migrateWorkspace(JSON.parse(raw));
  } catch (error) {
    console.warn("[storage] 读取失败，按空工作区处理", error);
    return null;
  }
}

export async function saveWorkspace(workspace: Workspace): Promise<void> {
  await invoke("write_state", { json: JSON.stringify(workspace) });
}

export async function clearWorkspace(): Promise<void> {
  await invoke("write_state", { json: "" });
}
