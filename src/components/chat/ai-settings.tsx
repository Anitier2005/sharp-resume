import { useState } from "react";
import { AI_PRESETS, aiReady, testConnection, type AiSettings } from "@/lib/ai";
import { Modal } from "@/components/ui/modal";
import { cx } from "@/lib/cx";
import styles from "./ai-settings.module.css";
import shared from "@/components/ui/shared.module.css";

interface Props {
  settings: AiSettings;
  onClose: () => void;
  onSave: (next: AiSettings) => void;
}

export function AiSettingsSheet({ settings, onClose, onSave }: Props) {
  const [draft, setDraft] = useState<AiSettings>(settings);
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

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
    <Modal
      title="模型设置"
      subtitle="任何 OpenAI 兼容接口：DeepSeek / OpenAI / Ollama / 自建网关"
      // 底部已有「取消」，所以不给右上角 X —— 两个否定出口会让人犹豫点哪个
      onCancel={onClose}
      bodyClassName={styles.body}
      footer={
        <>
          <button type="button" className={shared.btnGhost} onClick={onClose}>
            取消
          </button>
          <span className={styles.sp} />
          <button
            type="button"
            className={shared.btnGhost}
            disabled={testing || !draft.baseUrl.trim() || !draft.model.trim()}
            onClick={() => void runTest()}
          >
            {testing ? "测试中…" : "测试连接"}
          </button>
          <button
            type="button"
            className={shared.btnPrimary}
            disabled={!aiReady(draft)}
            onClick={() => onSave(draft)}
          >
            保存
          </button>
        </>
      }
    >
      <div className={styles.presets}>
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

      <label className={shared.fld}>
        <span className={shared.fldK}>接口地址</span>
        <input
          // 打开就停在第一个可改的字段上，用户不用先 Tab 一次
          data-autofocus
          value={draft.baseUrl}
          placeholder="https://api.deepseek.com/v1"
          spellCheck={false}
          onChange={(event) => patch({ baseUrl: event.target.value })}
        />
      </label>

      <label className={shared.fld}>
        <span className={shared.fldK}>模型</span>
        <input
          value={draft.model}
          placeholder="deepseek-chat"
          spellCheck={false}
          onChange={(event) => patch({ model: event.target.value })}
        />
      </label>

      <label className={shared.fld}>
        <span className={shared.fldK}>API Key</span>
        <input
          type="password"
          value={draft.apiKey}
          placeholder="sk-…"
          spellCheck={false}
          onChange={(event) => patch({ apiKey: event.target.value })}
        />
      </label>

      <p className={styles.note}>
        密钥只存在这台机器的 localStorage，不写进工作区、不随简历导出。留空则按本地改写规则回答。
      </p>

      {result && (
        <p className={cx(styles.result, result.ok ? styles.resultOk : styles.resultBad)}>{result.text}</p>
      )}
    </Modal>
  );
}
