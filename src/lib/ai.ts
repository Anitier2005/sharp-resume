/**
 * 模型接入：任何 OpenAI 兼容接口（DeepSeek / OpenAI / 本地 ollama / 自建网关）都能用。
 * 密钥只留在本机 localStorage，不写进工作区、不随简历同步。
 */

export interface AiSettings {
  baseUrl: string;
  model: string;
  apiKey: string;
  temperature: number;
}

const STORE_KEY = "sharp-resume:ai:v1";

export const DEFAULT_AI: AiSettings = {
  baseUrl: "https://api.deepseek.com/v1",
  model: "deepseek-chat",
  apiKey: "",
  temperature: 0.4,
};

export const AI_PRESETS: { label: string; baseUrl: string; model: string }[] = [
  { label: "DeepSeek", baseUrl: "https://api.deepseek.com/v1", model: "deepseek-chat" },
  { label: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "gpt-4o-mini" },
  { label: "本地 Ollama", baseUrl: "http://127.0.0.1:11434/v1", model: "qwen2.5:7b" },
];

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

/** 流式对话：逐段回调，返回完整文本 */
export async function streamChat(options: {
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
