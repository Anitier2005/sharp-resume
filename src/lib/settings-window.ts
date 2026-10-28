import { invoke } from "@tauri-apps/api/core";
import { emitTo } from "@tauri-apps/api/event";
import { isTauri } from "@/lib/storage";

/**
 * 设置窗口。
 *
 * 为什么用真窗口而不是再套一层浮层：设置只有几个字段，但它要一边改一边看主窗口
 * 的变化（换强调色、换字号、换当前模型）。浮层必须盖住主窗口才成立，改完还得
 * 手动关掉再看效果；独立窗口可以拖到旁边，改一下、看一眼。
 *
 * 一个设置区一个窗口，label 固定（`settings-model` / `settings-look` / `settings-knowledge`）：
 * 重复点同一个入口只会把它聚焦到前面，不会开出一排一模一样的窗。
 */
export type SettingsView = "model" | "look" | "knowledge";

/**
 * 打开（或前置）设置窗口。
 *
 * 建窗这件事交给 Rust（`open_settings`）：前端 `new WebviewWindow` 要先要一组 ACL
 * 权限，再用 `getByLabel` 去重又是另一条权限 —— 少一条就静默失败，很难查。
 * 这里只发一条命令。
 *
 * 返回 false 表示当前不是桌面环境（浏览器调试），调用方自行退回浮层。
 */
export async function openSettingsWindow(
  view: SettingsView,
  params: Record<string, string> = {},
): Promise<boolean> {
  if (!isTauri()) return false;
  try {
    await invoke("open_settings", { view, params });
    return true;
  } catch (error) {
    console.error("打开设置窗口失败", error);
    return false;
  }
}

/* ==========================================================================
   跨窗口通知
   ========================================================================== */

/** 设置窗改了模型：主窗口据此重新读 localStorage 里的当前模型 */
export type SettingsChanged = { kind: "ai" | "aux" };

/** 外观窗改了一项：主窗口立刻应用到 `.win` 上 */
export interface UiPatch {
  accent?: string;
  accentHex?: string;
  mode?: "light" | "dark";
  textScale?: number | null;
}

async function toMain(event: string, payload: unknown) {
  if (!isTauri()) return;
  try {
    await emitTo("main", event, payload);
  } catch {
    // 主窗口已经关了（例如只剩设置窗）：没有接收方不是错误
  }
}

export const notifySettingsChanged = (kind: SettingsChanged["kind"]) =>
  toMain("settings:changed", { kind } satisfies SettingsChanged);

export const notifyUiPatch = (patch: UiPatch) => toMain("ui:patch", patch);
