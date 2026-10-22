import { invoke } from "@tauri-apps/api/core";
import type { Workspace } from "@/types";

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
      return JSON.parse(raw) as Workspace;
    }
    const raw = localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as Workspace) : null;
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
