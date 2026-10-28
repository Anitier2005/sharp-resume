import raw from "../../config/models.json";

/**
 * 模型配置：来自 `config/models.json`，用静态 import 引入，
 * 所以改完刷新即生效（dev），release 里已经硬编码进 bundle，不依赖外部文件。
 */
export interface ModelConfig {
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
   * 模型本身能不能看图。注意写的是「模型的客观能力」，与用户有没有 key 无关 ——
   * 附件管线靠它决定图片是 base64 直传（vision）还是走 OCR 转文字。
   * 新模型进 config/models.json 时必须同时给这个字段，缺了按不能看图处理。
   */
  vision: boolean;
  defaultTemperature: number;
  note?: string;
}

export const MODELS: ModelConfig[] = (raw as { models: ModelConfig[] }).models;

export const modelById = (id: string): ModelConfig | undefined =>
  MODELS.find((item) => item.id === id);

/* ==========================================================================
   自定义模型

   内置清单是构建期 JSON，打包后写不回去 —— 用户自己加的模型只能另存一处。
   选 localStorage 与 ai 设置同域同策略：只在本机，不随工作区同步（密钥、网关
   地址这类东西跟着简历走反而是负担）。
   ========================================================================== */

const STORE_KEY = "sharp-resume:models:v1";

/** 自定义模型只填「名字 + 模型名 + 网关」，其余按最常见档位兜底 */
const CUSTOM_PROVIDER = "自定义";
const CUSTOM_CONTEXT_WINDOW = 64_000;
const CUSTOM_MAX_OUTPUT = 8_192;
const CUSTOM_TEMPERATURE = 0.4;

/**
 * 读一次就缓存在内存里：`listModels()` 会被渲染期调用（下拉、底栏），
 * 每次都 JSON.parse 一遍 localStorage 属于纯浪费。写入口只有本文件两个函数，
 * 它们会同步刷新这份缓存，所以不存在「缓存比磁盘旧」的窗口。
 */
let cache: ModelConfig[] | null = null;

function readCustom(): ModelConfig[] {
  if (cache) return cache;
  cache = [];
  try {
    const text = localStorage.getItem(STORE_KEY);
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
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(clean));
  } catch {
    // 隐私模式或配额满：本次会话里仍按内存中的清单生效
  }
}

/**
 * 清洗外部数据：localStorage 里的东西可能是手改的、旧版本写的，
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

/** 内置在前、自定义在后：顺序稳定，下拉不会因为加了模型就整体重排 */
export function listModels(): ModelConfig[] {
  return [...MODELS, ...readCustom()];
}

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
