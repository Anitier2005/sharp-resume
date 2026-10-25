import { Channel, invoke } from "@tauri-apps/api/core";
import { LOCAL_AI, MODELS } from "@/lib/model-config";

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

/** 默认值：优先用 `config/ai.local.json`（仅开发期），否则取模型清单第一条 */
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

export const aiReady = (settings: AiSettings): boolean =>
  Boolean(settings.baseUrl.trim() && settings.model.trim() && settings.apiKey.trim());

export interface ChatMsg {
  role: "system" | "user" | "assistant";
  content: string;
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

/** fetch 在跨域被拒时只会抛 TypeError，这里换成能看懂的提示 */
async function send(url: string, init: RequestInit): Promise<Response> {
  try {
    return await fetch(url, init);
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") throw error;
    const reason = error instanceof Error ? error.message : String(error);
    throw new Error(`网络不可达，或该接口不允许跨域调用（${reason}）`);
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
