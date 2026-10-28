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
