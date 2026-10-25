import { invoke } from "@tauri-apps/api/core";
import type { Workspace } from "@/types";
import { migrateWorkspace } from "@/lib/migrate";

/** 键名不随版本变：版本号存在数据里的 `version` 字段，换了键名等于把用户数据弄丢 */
const KEY = "sharp-resume:workspace:v1";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 读取工作区。桌面端写用户数据目录，浏览器回落 localStorage。 */
export async function loadWorkspace(): Promise<Workspace | null> {
  try {
    if (isTauri()) {
      const raw = await invoke<string | null>("read_state");
      if (!raw) return null;
      return migrateWorkspace(JSON.parse(raw));
    }
    const raw = localStorage.getItem(KEY);
    return raw ? migrateWorkspace(JSON.parse(raw)) : null;
  } catch (error) {
    console.warn("[storage] 读取失败，使用种子数据", error);
    return null;
  }
}

export async function saveWorkspace(workspace: Workspace): Promise<void> {
  const json = JSON.stringify(workspace);
  if (isTauri()) {
    await invoke("write_state", { json });
    return;
  }
  localStorage.setItem(KEY, json);
}

export async function clearWorkspace(): Promise<void> {
  if (isTauri()) {
    await invoke("write_state", { json: "" });
    return;
  }
  localStorage.removeItem(KEY);
}
