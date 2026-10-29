import { invoke } from "@tauri-apps/api/core";

/**
 * 应用设置的存放处：`app.db` 里的 `settings` 表。
 *
 * 为什么不留在 localStorage：那里是 WebView2 的用户数据目录，**不在**
 * `SharpResumeData\` 下面。结果就是「拷走数据目录换台机器」会丢掉模型配置与密钥 ——
 * 而这些是持久状态，重算不回来（密钥用户自己也不一定记得）。
 *
 * 为什么仍然同步读：十几个组件在初始化时就要 `loadAi()`。启动先 `hydrateSettings()`
 * 把整张表灌进内存，之后读是同步的；写则即时更新内存、异步落库。
 *
 * 键名沿用旧的 localStorage 后缀（`ai:v1` 等），迁移时一一对应，不留翻译层。
 */

/** 归数据库管的键（不带旧前缀） */
export const SETTING_KEYS = ["ai:v1", "aux:v1", "slots:v1", "profiles:v1", "models:v1"] as const;

const LEGACY_PREFIX = "sharp-resume:";

const cache = new Map<string, string>();

/**
 * 别的模块在内存里也有一层缓存（`model-config` 的模型清单与配置组）。
 * 它们直接依赖这里的数据，所以重读之后必须一起失效 —— 挂在这里而不是
 * 让每个调用点自己记得清，是为了「以后再多一个监听者」时不用改任何一处。
 */
const afterRefresh = new Set<() => void>();

export function onSettingsRefresh(listener: () => void): void {
  afterRefresh.add(listener);
}

/** 启动时调一次。读失败不算致命：按默认值继续，用户改一次就会重新写进去 */
export async function hydrateSettings(): Promise<void> {
  await refreshSettings();
  await migrateFromLocalStorage();
}

/**
 * 重读整张表。别的窗口改了设置（`settings:changed`）时必须调它：
 * 每个窗口各有自己的 JS 上下文与缓存，不重读就会拿着旧值发下一条消息。
 */
export async function refreshSettings(): Promise<void> {
  try {
    const pairs = await invoke<[string, string][]>("settings_load");
    cache.clear();
    for (const [key, value] of pairs) cache.set(key, value);
    for (const listener of afterRefresh) listener();
  } catch (error) {
    console.error("[settings] 读取失败，使用默认值", error);
  }
}

/** 一次性搬迁：旧版本存在 localStorage 的值搬进库，搬完删掉 */
async function migrateFromLocalStorage(): Promise<void> {
  for (const key of SETTING_KEYS) {
    const legacy = localStorage.getItem(LEGACY_PREFIX + key);
    if (legacy === null) continue;
    if (!cache.has(key)) {
      cache.set(key, legacy);
      try {
        await invoke("settings_put", { key, value: legacy });
      } catch (error) {
        // 写不进去就留在 localStorage 里，下次启动再试 —— 不能先删再写
        console.error("[settings] 迁移失败", key, error);
        continue;
      }
    }
    localStorage.removeItem(LEGACY_PREFIX + key);
  }
  // 浏览器调试模式留下的工作区快照：那条路已经删了，留着只会让人以为还在用
  localStorage.removeItem(LEGACY_PREFIX + "workspace:v1");
}

export const readSetting = (key: string): string | null => cache.get(key) ?? null;

export function writeSetting(key: string, value: string): void {
  cache.set(key, value);
  void invoke("settings_put", { key, value }).catch((error) =>
    console.error("[settings] 写入失败", key, error),
  );
}

export function dropSetting(key: string): void {
  cache.delete(key);
  void invoke("settings_drop", { key }).catch((error) =>
    console.error("[settings] 删除失败", key, error),
  );
}
