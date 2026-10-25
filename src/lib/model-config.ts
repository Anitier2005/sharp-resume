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
  defaultTemperature: number;
  note?: string;
}

export const MODELS: ModelConfig[] = (raw as { models: ModelConfig[] }).models;

export const modelById = (id: string): ModelConfig | undefined =>
  MODELS.find((item) => item.id === id);

/** 本机密钥草稿（`config/ai.local.json`，已 gitignore）：只作为开发期的默认值 */
export interface LocalAiDraft {
  baseUrl?: string;
  model?: string;
  apiKey?: string;
  temperature?: number;
  reasoning?: string;
}

const localModules = import.meta.glob<{ default: LocalAiDraft }>("../../config/ai.local.json", {
  eager: true,
});

/**
 * 只在 dev 读取。
 * 生产构建里即便这个文件存在也不使用 —— 而且 `scripts/package.mjs` 会在
 * 发现它时直接拒绝打包，避免把密钥打进发行版。
 */
export const LOCAL_AI: LocalAiDraft | undefined = import.meta.env.DEV
  ? Object.values(localModules)[0]?.default
  : undefined;
