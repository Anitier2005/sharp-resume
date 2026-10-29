import { Channel, invoke } from "@tauri-apps/api/core";
import {
  LOCAL_AI,
  MODELS,
  DEFAULT_RESERVE,
  findModel,
  listProfiles,
  type ModelConfig,
  type ModelProfile,
} from "@/lib/model-config";

/**
 * 模型接入：任何 OpenAI 兼容接口（DeepSeek / OpenAI / 本地 ollama / 自建网关）都能用。
 * 密钥只留在本机 localStorage，不写进工作区、不随简历同步。
 * 模型清单与能力参数来自 config/models.json（打包时硬编码）。
 */

export interface AiSettings {
  baseUrl: string;
  model: string;
  apiKey: string;
  temperature: number;
  /** 思维强度档位，取值来自 config/models.json 里该模型的 reasoning 列表 */
  reasoning?: string;
  /** 上下文窗口（token），缺省取模型配置 */
  contextWindow?: number;
  /** 预留给压缩提示词的 token，缺省 2000；窗口减掉它才是历史能用的部分 */
  reserve?: number;
  /** 用的是这个模型的哪一套配置（见 model-config 的 ModelProfile） */
  profileId?: string;
}

const STORE_KEY = "sharp-resume:ai:v1";

/** 默认值：优先用 testdata 里的测试配置 + 本机密钥（仅开发期），否则取模型清单第一条 */
export const DEFAULT_AI: AiSettings = {
  baseUrl: LOCAL_AI?.baseUrl ?? MODELS[0]?.baseUrl ?? "https://api.deepseek.com/v1",
  model: LOCAL_AI?.model ?? MODELS[0]?.id ?? "deepseek-chat",
  apiKey: LOCAL_AI?.apiKey ?? "",
  temperature: LOCAL_AI?.temperature ?? MODELS[0]?.defaultTemperature ?? 0.4,
  reasoning: LOCAL_AI?.reasoning ?? MODELS[0]?.reasoning[0],
  contextWindow: MODELS[0]?.contextWindow,
  reserve: DEFAULT_RESERVE,
};

/** 兼容旧版设置弹窗里的快捷预设（设置页已经改成从模型清单选，这里留作兜底） */
export const AI_PRESETS: { label: string; baseUrl: string; model: string }[] = MODELS.map(
  (item) => ({ label: item.label, baseUrl: item.baseUrl, model: item.id }),
);

export function loadAi(): AiSettings {
  try {
    const raw = localStorage.getItem(STORE_KEY);
    if (!raw) return DEFAULT_AI;
    const parsed = JSON.parse(raw) as Partial<AiSettings>;
    return { ...DEFAULT_AI, ...parsed };
  } catch {
    return DEFAULT_AI;
  }
}

export function saveAi(settings: AiSettings): void {
  localStorage.setItem(STORE_KEY, JSON.stringify(settings));
}

/**
 * 由模型清单里的一条（内置或用户自建）产出完整设置。
 *
 * apiKey 沿用上一份而不是清空：换模型通常只是换同一网关上的型号，凭据往往还是
 * 那一条；真换网关时用户在设置页再贴一次即可。少了这一步，每次换模型都要重新
 * 输入密钥 —— 而密钥是打码显示的，重输一遍成本很高。
 */
export function aiFromModel(model: ModelConfig, prev: AiSettings = loadAi()): AiSettings {
  return {
    // 自定义模型允许留空 baseUrl（沿用当前网关），内置项一定有值
    baseUrl: model.baseUrl || prev.baseUrl,
    model: model.id,
    apiKey: prev.apiKey,
    temperature: model.defaultTemperature,
    reasoning: model.reasoning[0],
    contextWindow: model.contextWindow,
    // 压缩预留跟模型无关，是用户对「历史压到多细」的选择，换模型不该重置
    reserve: prev.reserve ?? DEFAULT_RESERVE,
  };
}

/* ==========================================================================
   辅助模型：嵌入（知识库向量检索）与 OCR（图片转文字）
   ========================================================================== */

/**
 * 这两类不是「当前对话用哪个模型」，而是「某个功能用哪个模型」。
 * 所以不和 AiSettings 混在一起：换对话模型不该悄悄把知识库的嵌入模型也换掉。
 */
export interface AuxModelChoice {
  model: string;
  /** 空 = 沿用对话模型那把网关 */
  baseUrl: string;
  /** 空 = 沿用对话模型那把密钥 */
  apiKey: string;
  /** 用的是这个模型的哪一套配置 */
  profileId?: string;
}

export interface AuxSettings {
  embedding: AuxModelChoice | null;
  ocr: AuxModelChoice | null;
}

const AUX_KEY = "sharp-resume:aux:v1";
const EMPTY_AUX: AuxSettings = { embedding: null, ocr: null };

function pickChoice(value: unknown): AuxModelChoice | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Partial<AuxModelChoice>;
  const model = typeof raw.model === "string" ? raw.model.trim() : "";
  if (!model) return null;
  return {
    model,
    baseUrl: typeof raw.baseUrl === "string" ? raw.baseUrl.trim() : "",
    apiKey: typeof raw.apiKey === "string" ? raw.apiKey : "",
  };
}

export function loadAux(): AuxSettings {
  try {
    const raw = localStorage.getItem(AUX_KEY);
    if (!raw) return EMPTY_AUX;
    const parsed = JSON.parse(raw) as Partial<AuxSettings>;
    return { embedding: pickChoice(parsed.embedding), ocr: pickChoice(parsed.ocr) };
  } catch {
    return EMPTY_AUX;
  }
}

export function saveAux(patch: Partial<AuxSettings>): void {
  const next = { ...loadAux(), ...patch };
  localStorage.setItem(AUX_KEY, JSON.stringify(next));
}

/* ==========================================================================
   其余三类用途：压缩 / 排版 / 知识库分片
   ========================================================================== */

/**
 * 这三类同样不是「当前对话用哪个模型」，而是「某件事用哪个模型」。
 * 存在独立键里，缺省 = 沿用会话模型（用户不配也能用）。
 */
export type SlotKind = "compact" | "layout" | "chunk";

export interface SlotSettings {
  compact: AuxModelChoice | null;
  layout: AuxModelChoice | null;
  chunk: AuxModelChoice | null;
}

const SLOTS_KEY = "sharp-resume:slots:v1";
const EMPTY_SLOTS: SlotSettings = { compact: null, layout: null, chunk: null };

export function loadSlots(): SlotSettings {
  try {
    const raw = localStorage.getItem(SLOTS_KEY);
    if (!raw) return EMPTY_SLOTS;
    const parsed = JSON.parse(raw) as Partial<SlotSettings>;
    return {
      compact: pickChoice(parsed.compact),
      layout: pickChoice(parsed.layout),
      chunk: pickChoice(parsed.chunk),
    };
  } catch {
    return EMPTY_SLOTS;
  }
}

export function saveSlot(kind: SlotKind, choice: AuxModelChoice | null): void {
  const next = { ...loadSlots(), [kind]: choice };
  localStorage.setItem(SLOTS_KEY, JSON.stringify(next));
}

/**
 * 把「模型 + 配置」解析成一次真实请求需要的参数。
 *
 * 规则：配置里存的 baseUrl / apiKey 优先；为空就沿用会话模型那把网关与密钥 ——
 * 同一家网关的多个用途通常共用一把 key，不该逼用户填四遍。
 */
export function slotAi(kind: SlotKind): AiSettings {
  const base = loadAi();
  const slot = loadSlots()[kind];
  if (!slot || !slot.model.trim()) return base;
  const spec = findModel(slot.model);
  return {
    ...base,
    baseUrl: slot.baseUrl.trim() || base.baseUrl,
    apiKey: slot.apiKey.trim() || base.apiKey,
    model: slot.model,
    profileId: slot.profileId,
    temperature: spec?.defaultTemperature ?? base.temperature,
    reasoning: spec?.reasoning[0] ?? base.reasoning,
    contextWindow: spec?.contextWindow ?? base.contextWindow,
  };
}

/** 会话模型某一条配置对应的完整接入参数（模型 id 取配置所属的模型） */
export function choiceOf(modelId: string, profile: ModelProfile): AuxModelChoice {
  const spec = findModel(modelId);
  return {
    model: modelId,
    profileId: profile.id,
    baseUrl: profile.baseUrl.trim() || spec?.baseUrl || "",
    apiKey: profile.apiKey,
  };
}

/** 某个模型当前的默认配置（用于首次选模型时给一个合理的初值） */
export function defaultChoiceOf(modelId: string): AuxModelChoice | null {
  const spec = findModel(modelId);
  if (!spec) return null;
  const profile = listProfiles(spec)[0];
  if (!profile) return { model: modelId, baseUrl: spec.baseUrl, apiKey: "" };
  return choiceOf(modelId, profile);
}

export const aiReady = (settings: AiSettings): boolean =>
  Boolean(settings.baseUrl.trim() && settings.model.trim() && settings.apiKey.trim());

/**
 * 多模态消息段（OpenAI 兼容格式）。
 * 只有「模型有视觉」时才会构造数组 content；纯文本对话保持字符串 ——
 * 两种形状都按协议原样透传，不做二次包装。
 */
export type ContentPart =
  | { type: "text"; text: string }
  | { type: "image_url"; image_url: { url: string } };

export interface ChatMsg {
  role: "system" | "user" | "assistant";
  content: string | ContentPart[];
}

/**
 * 连通性测试：走 Rust 发一句最短请求，把回复原样带回来。
 *
 * 以前这里还有一条 fetch 通道（浏览器调试模式用，会被 CORS 拦）。
 * 那个模式已经删掉了：请求一律由 Rust 发出，不经 WebView，也就没有跨域问题。
 */
export async function testConnection(settings: AiSettings): Promise<string> {
  return invoke<string>("chat_once", { req: wire(settings, [{ role: "user", content: "ping" }]) });
}

/* ==========================================================================
   通讯层：全部走 Rust（不经过 WebView，因此没有 CORS）

   注意别把这句话读成「密钥不经过前端」：密钥存在 localStorage，由 `loadAi()`
   读进内存再随请求交给 Rust。想真正做到「密钥只在受信侧」，得把它搬进
   Rust 侧的配置文件 —— 那是独立的一件事，见 README 的待办。
   ========================================================================== */

/** 前端设置 → Rust 侧 AiRequest（camelCase，和 net.rs 的 serde 对齐） */
function wire(settings: AiSettings, messages: ChatMsg[]) {
  return {
    baseUrl: settings.baseUrl,
    apiKey: settings.apiKey,
    model: settings.model,
    messages: messages.map((item) => ({ role: item.role, content: item.content })),
    temperature: settings.temperature,
    reasoning: settings.reasoning ?? null,
    maxTokens: null,
  };
}

interface StreamChunk {
  kind: "delta" | "reasoning" | "done" | "error";
  delta?: string;
  reasoning?: string;
  message?: string;
}

/** 流式对话统一入口：Tauri Channel 接收 Rust 解好的 SSE 增量 */
export async function streamChat(options: {
  settings: AiSettings;
  messages: ChatMsg[];
  signal?: AbortSignal;
  onDelta: (delta: string) => void;
  /** 思维链增量（部分模型才有） */
  onReasoning?: (delta: string) => void;
}): Promise<string> {
  const { settings, messages, signal, onDelta, onReasoning } = options;

  const channel = new Channel<StreamChunk>();
  let full = "";
  channel.onmessage = (chunk) => {
    if (chunk.kind === "delta" && chunk.delta) {
      full += chunk.delta;
      onDelta(chunk.delta);
    } else if (chunk.kind === "reasoning" && chunk.reasoning) {
      onReasoning?.(chunk.reasoning);
    }
  };

  void signal; // 暂不支持中断：invoke 一旦发出无法取消，后续用 cancel 命令补
  await invoke("chat_stream", { req: wire(settings, messages), onEvent: channel });
  return full;
}
