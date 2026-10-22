import { useEffect, useRef, useState } from "react";
import { AI_PRESETS, aiReady, testConnection, type AiSettings } from "@/lib/ai";
import { Close } from "@/components/ui/icons";

interface Props {
  settings: AiSettings;
  onClose: () => void;
  onSave: (next: AiSettings) => void;
}

export function AiSettingsSheet({ settings, onClose, onSave }: Props) {
  const [draft, setDraft] = useState<AiSettings>(settings);
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const firstRef = useRef<HTMLInputElement | null>(null);
  // onClose 每次父组件渲染都是新函数，用 ref 兜住，避免重新挂载监听、抢焦点
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    firstRef.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeRef.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const patch = (next: Partial<AiSettings>) => {
    setDraft((value) => ({ ...value, ...next }));
    setResult(null);
  };

  const runTest = async () => {
    setTesting(true);
    setResult(null);
    try {
      const text = await testConnection(draft);
      setResult({ ok: true, text: `连通 · 模型回：${text.slice(0, 40)}` });
    } catch (error) {
      setResult({ ok: false, text: error instanceof Error ? error.message : String(error) });
    } finally {
      setTesting(false);
    }
  };

  return (
    <div
      className="veil"
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <div className="sheet" role="dialog" aria-modal="true" aria-label="模型设置">
        <header className="sheet-head">
          <div className="drawer-title">
            <b>模型设置</b>
            <span>任何 OpenAI 兼容接口：DeepSeek / OpenAI / Ollama / 自建网关</span>
          </div>
          <button type="button" className="drawer-close" aria-label="关闭" onClick={onClose}>
            <Close size={11} />
          </button>
        </header>

        <div className="sheet-body">
          <div className="presets">
            {AI_PRESETS.map((preset) => (
              <button
                key={preset.label}
                type="button"
                aria-pressed={draft.baseUrl === preset.baseUrl && draft.model === preset.model}
                onClick={() => patch({ baseUrl: preset.baseUrl, model: preset.model })}
              >
                {preset.label}
              </button>
            ))}
          </div>

          <label className="fld">
            <span className="fld-k">接口地址</span>
            <input
              ref={firstRef}
              value={draft.baseUrl}
              placeholder="https://api.deepseek.com/v1"
              spellCheck={false}
              onChange={(event) => patch({ baseUrl: event.target.value })}
            />
          </label>

          <label className="fld">
            <span className="fld-k">模型</span>
            <input
              value={draft.model}
              placeholder="deepseek-chat"
              spellCheck={false}
              onChange={(event) => patch({ model: event.target.value })}
            />
          </label>

          <label className="fld">
            <span className="fld-k">API Key</span>
            <input
              type="password"
              value={draft.apiKey}
              placeholder="sk-…"
              spellCheck={false}
              onChange={(event) => patch({ apiKey: event.target.value })}
            />
          </label>

          <p className="sheet-note">
            密钥只存在这台机器的 localStorage，不写进工作区、不随简历导出。留空则按本地改写规则回答。
          </p>

          {result && (
            <p className={`sheet-result${result.ok ? " ok" : " bad"}`}>{result.text}</p>
          )}
        </div>

        <footer className="sheet-foot">
          <button type="button" className="btn-ghost" onClick={onClose}>
            取消
          </button>
          <span className="sp" />
          <button
            type="button"
            className="btn-ghost"
            disabled={testing || !draft.baseUrl.trim() || !draft.model.trim()}
            onClick={() => void runTest()}
          >
            {testing ? "测试中…" : "测试连接"}
          </button>
          <button
            type="button"
            className="btn-primary"
            disabled={!aiReady(draft)}
            onClick={() => onSave(draft)}
          >
            保存
          </button>
        </footer>
      </div>
    </div>
  );
}
