import raw from "../../config/models.json";
import { onSettingsRefresh, readSetting, writeSetting } from "@/lib/settings-store";

/**
 * 模型配置：来自 `config/models.json`，用静态 import 引入，
 * 所以改完刷新即生效（dev），release 里已经硬编码进 bundle，不依赖外部文件。
 */
/**
 * 模型分三类，存在同一份清单里：
 * - chat：对话与工具调用（默认）
 * - embedding：知识库向量检索（只算向量，不进对话）
 * - ocr：图片转文字（默认构建没内置 tesseract 时的兜底）
 */
/**
 * 模型按「用在哪」分类。同一个模型（比如 gpt-4o-mini）可能同时出现在几类里，
 * 每一类各管各的配置指针：换会话模型不该把压缩、分片的模型一起换掉。
 */
export type ModelKind = "chat" | "compact" | "chunk" | "embedding" | "ocr";

/** 模型选择页的五行，顺序 = 用户心里的优先级 */
export const MODEL_KINDS: { kind: ModelKind; label: string; note: string }[] = [
  { kind: "chat", label: "会话", note: "主对话：改写、读页、调工具、排版" },
  { kind: "compact", label: "压缩", note: "历史超窗口时压成摘要" },
  { kind: "chunk", label: "知识库分片", note: "把文档切成块并写摘要" },
  { kind: "embedding", label: "嵌入", note: "知识库向量化与语义召回" },
  { kind: "ocr", label: "OCR", note: "模型看不了图时把图片转文字" },
];

export interface ModelConfig {
  /** 缺省 = chat（老数据、内置对话条目都不写这个字段） */
  kind?: ModelKind;
  id: string;
  label: string;
  provider: string;
  baseUrl: string;
  /** 上下文窗口（token），用于估算「这份材料会不会超」 */
  contextWindow: number;
  maxOutput: number;
  /** 可选的思维强度档位；空数组表示这个模型没有该能力 */
  reasoning: string[];
  /**
   * 清单里的**出厂声明**，只是给用户看的说明，**不再驱动管线**。
   *
   * 能不能看图现在由 Rust 侧实测（`capability::resolve` / `probe`）决定 ——
   * 这个字段说「支持」而网关不认，图发过去只会换一个 400；说「不支持」而模型
   * 其实能看，图就永远只以元数据出现。新模型进 `config/models.json` 时照旧填它，
   * 但别拿它当判定依据。
   */
  vision: boolean;
  defaultTemperature: number;
  /** 嵌入模型的向量维度，只在 kind=embedding 时有意义（界面展示用） */
  dim?: number;
  note?: string;
}

export const MODELS: ModelConfig[] = (raw as { models: ModelConfig[] }).models;

export const modelById = (id: string): ModelConfig | undefined =>
  MODELS.find((item) => item.id === id);

/* ==========================================================================
   自定义模型

   内置清单是构建期 JSON，打包后写不回去 —— 用户自己加的模型只能另存一处。
   与 ai 设置同一处（`app.db` 的 settings 表）：只在本机，不随简历导出，
   但跟着数据目录走 —— 换台机器拷过去就能继续用。
   ========================================================================== */

const STORE_KEY = "models:v1";

/** 自定义模型只填「名字 + 模型名 + 网关」，其余按最常见档位兜底 */
const CUSTOM_PROVIDER = "自定义";
const CUSTOM_CONTEXT_WINDOW = 64_000;
const CUSTOM_MAX_OUTPUT = 8_192;
const CUSTOM_TEMPERATURE = 0.4;

/**
 * 读一次就缓存在内存里：`listModels()` 会被渲染期调用（下拉、底栏），
 * 每次都 JSON.parse 一遍存下来的 JSON 属于纯浪费。写入口只有本文件两个函数，
 * 它们会同步刷新这份缓存，所以不存在「缓存比磁盘旧」的窗口。
 */
let cache: ModelConfig[] | null = null;

function readCustom(): ModelConfig[] {
  if (cache) return cache;
  cache = [];
  try {
    const text = readSetting(STORE_KEY);
    if (!text) return cache;
    const parsed: unknown = JSON.parse(text);
    if (!Array.isArray(parsed)) return cache;
    const seen = new Set<string>();
    for (const item of parsed) {
      const model = normalizeCustom(item);
      if (!model) continue;
      // 内置项优先：同 id 的自定义项永远选不到，留着只会变成删不掉的幽灵数据
      if (MODELS.some((builtin) => builtin.id === model.id)) continue;
      if (seen.has(model.id)) continue;
      seen.add(model.id);
      cache.push(model);
    }
  } catch {
    // 坏数据（手改过 / 旧版本格式）按没有自定义模型处理，不让整个设置页打不开
    cache = [];
  }
  return cache;
}

function writeCustom(list: ModelConfig[]): void {
  // 与内置同 id 的项一律不写：内置排在前，写进去也永远选不到
  const clean = list.filter((item) => !MODELS.some((builtin) => builtin.id === item.id));
  cache = clean;
  writeSetting(STORE_KEY, JSON.stringify(clean));
}

/**
 * 清洗外部数据：库里的东西可能是手改的、旧版本写的，
 * 缺字段就补缺省值而不是整条丢弃 —— 用户加过的模型不该因为一个字段消失。
 */
function normalizeCustom(input: unknown): ModelConfig | null {
  if (!input || typeof input !== "object") return null;
  const raw = input as Partial<ModelConfig>;
  const id = typeof raw.id === "string" ? raw.id.trim() : "";
  if (!id) return null;
  const label = typeof raw.label === "string" && raw.label.trim() ? raw.label.trim() : id;
  const reasoning = Array.isArray(raw.reasoning)
    ? raw.reasoning
        .filter((level): level is string => typeof level === "string" && Boolean(level.trim()))
        .map((level) => level.trim())
    : [];
  const note = typeof raw.note === "string" && raw.note.trim() ? raw.note.trim() : undefined;
  return {
    id,
    label,
    kind: raw.kind === "embedding" || raw.kind === "ocr" ? raw.kind : "chat",
    // provider 不用存：自定义模型在界面上就是「自定义」，存了反而会被手改坏
    provider: CUSTOM_PROVIDER,
    baseUrl: typeof raw.baseUrl === "string" ? raw.baseUrl.trim() : "",
    contextWindow: positive(raw.contextWindow, CUSTOM_CONTEXT_WINDOW),
    maxOutput: positive(raw.maxOutput, CUSTOM_MAX_OUTPUT),
    reasoning,
    vision: raw.vision === true,
    defaultTemperature: clampTemperature(raw.defaultTemperature, CUSTOM_TEMPERATURE),
    note,
  };
}

function positive(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) && value > 0
    ? Math.round(value)
    : fallback;
}

/** 0–2 是 OpenAI 兼容接口的通用上限，超了会被服务端直接 400 */
function clampTemperature(value: unknown, fallback: number): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.min(2, Math.max(0, value));
}

/** 老数据/内置清单没有 kind，一律当会话模型；排版那类已随 agent 一起取消，旧值退化成会话 */
export const kindOf = (model: ModelConfig): ModelKind => {
  const raw = model.kind;
  if (raw === "compact" || raw === "chunk" || raw === "embedding" || raw === "ocr") {
    return raw;
  }
  return "chat";
};

/**
 * 内置在前、自定义在后：顺序稳定，下拉不会因为加了模型就整体重排。
 * 缺省只给对话模型 —— 底栏、会话这些老调用点不该突然看到嵌入/OCR 条目。
 */
export function listModels(kind: ModelKind = "chat"): ModelConfig[] {
  return [...MODELS, ...readCustom()].filter((model) => kindOf(model) === kind);
}

/** 全部模型（不分用途）：模型选择页要跨类挑，配置页要一处看全 */
export function listAllModels(): ModelConfig[] {
  return [...MODELS, ...readCustom()];
}

/** 模型被哪些用途在用（模型配置页据此显示「会话 · 压缩」这类小字） */
export const kindLabel = (kind: ModelKind): string =>
  kind === "chat"
    ? "会话"
    : kind === "compact"
      ? "压缩"
      : kind === "chunk"
        ? "知识库分片"
        : kind === "embedding"
          ? "嵌入"
          : "OCR";

export function findModel(id: string): ModelConfig | undefined {
  return modelById(id) ?? readCustom().find((item) => item.id === id);
}

export function isCustomModel(id: string): boolean {
  return readCustom().some((item) => item.id === id);
}

/** 同 id 覆盖：用户改错的模型直接重填一次即可，不必先删 */
export function saveCustomModel(model: ModelConfig): void {
  const next = normalizeCustom(model);
  if (!next) return;
  const rest = readCustom().filter((item) => item.id !== next.id);
  writeCustom([...rest, next]);
}

export function removeCustomModel(id: string): void {
  const current = readCustom();
  if (!current.some((item) => item.id === id)) return;
  writeCustom(current.filter((item) => item.id !== id));
}

/* ==========================================================================
   模型配置（profile）

   同一个模型（例如 deepseek-chat）经常要存好几套接入方式：官方直连、公司代理、
   临时借来的 key。以前这些字段直接挂在模型上，换一次就得重填一遍；拆成
   「模型 → 若干配置」之后，切换只换指针，字段各归各的。

   存在 settings 表的 `profiles:v1`，形如 `{ [modelId]: Profile[] }`。
   内置模型也要能加配置，所以这张表按 modelId 索引、与自定义模型清单分开存。
   ========================================================================== */

export interface ModelProfile {
  id: string;
  label: string;
  baseUrl: string;
  apiKey: string;
  /** 对话模型：温度 */
  temperature?: number;
  /** 对话模型：思维强度档位的取值 */
  reasoning?: string;
  /** 上下文窗口（token） */
  contextWindow?: number;
  /** 预留给压缩提示词的 token：从窗口里扣掉它，剩下的才给历史 */
  reserve?: number;
}

const PROFILE_KEY = "profiles:v1";

/** 与模型清单同理：渲染期会被反复读，缓存在内存里，写入口同步刷新 */
let profileCache: Record<string, ModelProfile[]> | null = null;

function readProfiles(): Record<string, ModelProfile[]> {
  if (profileCache) return profileCache;
  const map: Record<string, ModelProfile[]> = {};
  try {
    const text = readSetting(PROFILE_KEY);
    if (text) {
      const parsed: unknown = JSON.parse(text);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        for (const [modelId, list] of Object.entries(parsed as Record<string, unknown>)) {
          if (!Array.isArray(list)) continue;
          const clean = list
            .map((item) => normalizeProfile(item))
            .filter((item): item is ModelProfile => item !== null);
          map[modelId] = clean;
        }
      }
    }
  } catch {
    // 坏数据按「没存过」处理：宁可回到模型缺省，也不让设置页打不开
  }
  profileCache = map;
  return map;
}

function writeProfiles(map: Record<string, ModelProfile[]>): void {
  profileCache = map;
  writeSetting(PROFILE_KEY, JSON.stringify(map));
}

function normalizeProfile(input: unknown): ModelProfile | null {
  if (!input || typeof input !== "object") return null;
  const raw = input as Partial<ModelProfile>;
  const id = typeof raw.id === "string" ? raw.id.trim() : "";
  if (!id) return null;
  return {
    id,
    label: typeof raw.label === "string" && raw.label.trim() ? raw.label.trim() : id,
    baseUrl: typeof raw.baseUrl === "string" ? raw.baseUrl.trim() : "",
    apiKey: typeof raw.apiKey === "string" ? raw.apiKey : "",
    temperature:
      typeof raw.temperature === "number" ? clampTemperature(raw.temperature, 0.4) : undefined,
    reasoning: typeof raw.reasoning === "string" ? raw.reasoning : undefined,
    contextWindow:
      typeof raw.contextWindow === "number" && raw.contextWindow > 0
        ? Math.round(raw.contextWindow)
        : undefined,
    reserve:
      typeof raw.reserve === "number" && raw.reserve >= 0 ? Math.round(raw.reserve) : undefined,
  };
}

/** 模型自带的那套配置：字段全部从模型定义派生 */
export function defaultProfile(model: ModelConfig): ModelProfile {
  return {
    id: "default",
    label: "默认",
    baseUrl: model.baseUrl,
    apiKey: "",
    temperature: model.defaultTemperature,
    reasoning: model.reasoning[0],
    contextWindow: model.contextWindow,
    reserve: DEFAULT_RESERVE,
  };
}

/**
 * 某个模型下的所有配置。
 *
 * 没存过 → 派生一条「默认」；存过但是空数组 → 真的没有（用户删光了），
 * 界面据此显示「添加配置」。这两种情况必须分清，否则删光了还会冒出一条幽灵。
 */
export function listProfiles(model: ModelConfig): ModelProfile[] {
  const stored = readProfiles()[model.id];
  return stored ?? [defaultProfile(model)];
}

export function saveProfile(modelId: string, profile: ModelProfile): void {
  const clean = normalizeProfile(profile);
  if (!clean) return;
  const map = { ...readProfiles() };
  const list = map[modelId] ?? [];
  const next = list.some((item) => item.id === clean.id)
    ? list.map((item) => (item.id === clean.id ? clean : item))
    : [...list, clean];
  map[modelId] = next;
  writeProfiles(map);
}

export function removeProfile(modelId: string, profileId: string): void {
  const map = { ...readProfiles() };
  const list = map[modelId];
  if (!list) return;
  map[modelId] = list.filter((item) => item.id !== profileId);
  writeProfiles(map);
}

/** 上下文窗口的候选档位：够用的一小组，不逼用户心算 65536 这种数 */
export const CONTEXT_PRESETS = [
  { value: 8_192, label: "8K" },
  { value: 16_000, label: "16K" },
  { value: 32_000, label: "32K" },
  { value: 64_000, label: "64K" },
  { value: 128_000, label: "128K" },
  { value: 200_000, label: "200K" },
  { value: 1_000_000, label: "1M" },
] as const;

/** 压缩预留的候选档位：窗口再大也就几段摘要，给太多等于白占 */
export const RESERVE_PRESETS = [
  { value: 1_000, label: "1K" },
  { value: 2_000, label: "2K" },
  { value: 4_000, label: "4K" },
  { value: 8_000, label: "8K" },
  { value: 16_000, label: "16K" },
] as const;

/** 没填时的压缩预留：够放一轮摘要提示词加被摘要的那几段 */
export const DEFAULT_RESERVE = 2_000;

/**
 * 开发期默认配置，两层合并：
 *   `testdata/ai.json`           测试环境（baseUrl / model / 参数），**可进仓库**
 *   `testdata/secrets/ai.local.json`  只放 apiKey 之类，**已被 .gitignore 排除**
 *
 * 用 glob 而不是 fs：构建期展开，新增文件自动带上；生产构建里 DEV 为 false 一律不读。
 */
export interface LocalAiDraft {
  baseUrl?: string;
  model?: string;
  apiKey?: string;
  temperature?: number;
  reasoning?: string;
}

/** 只挑认识的字段：配置文件里的 `$comment` 之类不该混进运行时设置 */
function pick(draft: LocalAiDraft | undefined): LocalAiDraft {
  if (!draft) return {};
  const out: LocalAiDraft = {};
  if (draft.baseUrl) out.baseUrl = draft.baseUrl;
  if (draft.model) out.model = draft.model;
  if (draft.apiKey) out.apiKey = draft.apiKey;
  if (typeof draft.temperature === "number") out.temperature = draft.temperature;
  if (draft.reasoning) out.reasoning = draft.reasoning;
  return out;
}

const secretModules = import.meta.glob<{ default: LocalAiDraft }>(
  "../../testdata/secrets/ai.local.json",
  { eager: true },
);
const testModules = import.meta.glob<{ default: LocalAiDraft }>(
  "../../testdata/ai.json",
  { eager: true },
);

/**
 * 只在 dev 生效。
 *
 * 写法上必须把 `import.meta.env.DEV` 直接放进三元表达式里，且**不要在模块顶层**
 * 先把 glob 结果取出来：那样两个 const 会变成有引用的顶层语句，摇树摇不掉，
 * 密钥就被打进发行版了（实测：`grep sk-  dist/` 能命中）。
 * 放在三元里时，生产构建下 DEV 为 false，整段（含 glob 引用）一起被消除。
 */
export const LOCAL_AI: LocalAiDraft | undefined = import.meta.env.DEV
  ? {
      ...pick(Object.values(testModules)[0]?.default),
      ...pick(Object.values(secretModules)[0]?.default),
    }
  : undefined;

/**
 * 设置窗改完模型清单 / 配置组之后，主窗的这两层内存缓存必须作废：
 * 它们是从 settings 表派生出来的，表变了缓存不会自己动。
 */
onSettingsRefresh(() => {
  cache = null;
  profileCache = null;
});
