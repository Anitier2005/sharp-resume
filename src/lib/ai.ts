import { Channel, invoke } from "@tauri-apps/api/core";
import { LOCAL_AI, MODELS, type ModelConfig } from "@/lib/model-config";

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

function endpoint(baseUrl: string): string {
  const base = baseUrl.trim().replace(/\/+$/, "");
  if (/\/chat\/completions$/.test(base)) return base;
  return `${base}/chat/completions`;
}

function headers(apiKey: string): Record<string, string> {
  const value: Record<string, string> = { "Content-Type": "application/json" };
  if (apiKey.trim()) value.Authorization = `Bearer ${apiKey.trim()}`;
  return value;
}

async function explain(response: Response): Promise<string> {
  const text = await response.text().catch(() => "");
  let detail = text.slice(0, 300);
  try {
    const json = JSON.parse(text) as { error?: { message?: string } };
    if (json.error?.message) detail = json.error.message;
  } catch {
    /* 原样返回 */
  }
  return `HTTP ${response.status}${detail ? ` · ${detail}` : ""}`;
}

/**
 * fetch 在跨域被拒时只会抛一个 TypeError：Failed to fetch —— 看不出是网断了、
 * 地址写错了、还是被 CORS 拦了。这里补上判断，把「现在到底跑在哪种模式」说清楚。
 */
async function send(url: string, init: RequestInit): Promise<Response> {
  try {
    return await fetch(url, init);
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") throw error;
    const reason = error instanceof Error ? error.message : String(error);
    const host = (() => {
      try {
        return new URL(url).host;
      } catch {
        return url;
      }
    })();
    if (inTauri()) {
      throw new Error(`连不上 ${host}（${reason}）`);
    }
    throw new Error(
      `浏览器调试模式下连不上 ${host}：请求被跨域策略拦截。` +
        `界面调试可以用浏览器，但真正对话要用桌面窗口（走 Rust 通讯层，不经 CORS）。` +
        `原始信息：${reason}`,
    );
  }
}

/** 浏览器里的实现（`npm run dev` 调试用；远端接口大多没有 CORS 头，可能失败） */
async function streamViaFetch(options: {
  settings: AiSettings;
  messages: ChatMsg[];
  signal?: AbortSignal;
  onDelta: (delta: string) => void;
}): Promise<string> {
  const { settings, messages, signal, onDelta } = options;
  const response = await send(endpoint(settings.baseUrl), {
    method: "POST",
    signal,
    headers: headers(settings.apiKey),
    body: JSON.stringify({
      model: settings.model,
      messages,
      stream: true,
      temperature: settings.temperature,
    }),
  });

  if (!response.ok) throw new Error(await explain(response));
  if (!response.body) throw new Error("接口没有返回流式内容");

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let full = "";

  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";
    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed.startsWith("data:")) continue;
      const payload = trimmed.slice(5).trim();
      if (!payload || payload === "[DONE]") continue;
      try {
        const json = JSON.parse(payload) as {
          choices?: { delta?: { content?: string } }[];
        };
        const delta = json.choices?.[0]?.delta?.content ?? "";
        if (delta) {
          full += delta;
          onDelta(delta);
        }
      } catch {
        // 分片被切开时解析失败是正常的，等下一轮
      }
    }
  }

  return full;
}

/** 连通性测试：发一条最短的请求 */
export async function testConnection(settings: AiSettings): Promise<string> {
  const response = await send(endpoint(settings.baseUrl), {
    method: "POST",
    headers: headers(settings.apiKey),
    body: JSON.stringify({
      model: settings.model,
      messages: [{ role: "user", content: "回复两个字：可用" }],
      max_tokens: 8,
      temperature: 0,
      stream: false,
    }),
  });
  if (!response.ok) throw new Error(await explain(response));
  const json = (await response.json()) as {
    choices?: { message?: { content?: string } }[];
  };
  return json.choices?.[0]?.message?.content ?? "已连通";
}

/**
 * 连通性自检：非流式发一句最短请求，把回复或错误原样返回。
 * 设置页用它验证 key / baseUrl / model 三件事是否对得上。
 */
export async function testAi(settings: AiSettings): Promise<string> {
  if (inTauri()) {
    return invoke<string>("chat_once", { req: wire(settings, [{ role: "user", content: "ping" }]) });
  }
  const response = await send(endpoint(settings.baseUrl), {
    method: "POST",
    headers: headers(settings.apiKey),
    body: JSON.stringify({
      model: settings.model,
      messages: [
        { role: "system", content: "You are a connectivity probe. Reply with OK only." },
        { role: "user", content: "ping" },
      ],
      stream: false,
      temperature: 0,
    }),
  });
  if (!response.ok) throw new Error(await explain(response));
  const json = (await response.json()) as {
    choices?: { message?: { content?: string } }[];
    usage?: { total_tokens?: number };
  };
  const text = json.choices?.[0]?.message?.content?.trim() ?? "(空回复)";
  const tokens = json.usage?.total_tokens;
  return tokens ? `${text} · ${tokens} tokens` : text;
}

/* ==========================================================================
   通讯层调度：桌面端走 Rust（不经过 WebView，因此没有 CORS、key 不进 localStorage
   之外的 JS 内存），浏览器端保留 fetch 以便 `npm run dev` 里调界面。
   ========================================================================== */

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

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

/**
 * 流式对话统一入口。
 * 桌面端用 Tauri Channel 接收 Rust 解好的 SSE 增量；浏览器端退回 fetch。
 */
export async function streamChat(options: {
  settings: AiSettings;
  messages: ChatMsg[];
  signal?: AbortSignal;
  onDelta: (delta: string) => void;
  /** 思维链增量（部分模型才有） */
  onReasoning?: (delta: string) => void;
}): Promise<string> {
  const { settings, messages, signal, onDelta, onReasoning } = options;

  if (!inTauri()) return streamViaFetch(options);

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
