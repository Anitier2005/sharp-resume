import { useEffect, useRef } from "react";
import type { Attachment } from "@/types";
import { ArrowUp, Close, FileIcon, Plus } from "@/components/ui/icons";
import styles from "./composer.module.css";
import { formatSize, toAttachment } from "@/lib/attach";

interface ComposerProps {
  model: string;
  onOpenSettings: () => void;
  onSend: (text: string, files: Attachment[]) => void;
  busy: boolean;
  /** 输入内容由会话面板持有，建议动作可以直接写入 */
  text: string;
  onTextChange: (text: string) => void;
  /** 待发送附件由外层持有，方便拖放注入 */
  files: Attachment[];
  onFilesChange: (files: Attachment[]) => void;
}

export function Composer({
  model,
  onOpenSettings,
  onSend,
  busy,
  text,
  onTextChange,
  files,
  onFilesChange,
}: ComposerProps) {
  const areaRef = useRef<HTMLTextAreaElement | null>(null);
  const fileRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    const el = areaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(160, el.scrollHeight)}px`;
  }, [text]);

  const submit = () => {
    const value = text.trim();
    if (!value && files.length === 0) return;
    if (busy) return;
    onSend(value, files);
    onTextChange("");
    onFilesChange([]);
  };

  return (
    <div className={styles.composer}>
      <div className={styles.composerIn}>
        <div className={styles.box}>
          {files.length > 0 && (
            <div className={styles.pendingFiles}>
              {files.map((file) => (
                <span className={styles.pending} key={file.id}>
                  <FileIcon size={10} />
                  {file.name}
                  <span className={styles.sz}>{formatSize(file.size)}</span>
                  <button
                    type="button"
                    aria-label={`移除 ${file.name}`}
                    onClick={() => onFilesChange(files.filter((item) => item.id !== file.id))}
                  >
                    <Close size={9} />
                  </button>
                </span>
              ))}
            </div>
          )}

          <textarea
            ref={areaRef}
            className={styles.input}
            rows={1}
            value={text}
            placeholder="描述你想改的地方，或拖入 JD、旧简历…"
            aria-label="消息输入"
            onChange={(event) => onTextChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                submit();
              }
            }}
          />

          <div className={styles.bar}>
            <button
              type="button"
              className={styles.plus}
              aria-label="添加文件"
              onClick={() => fileRef.current?.click()}
            >
              <Plus size={11} />
            </button>
            <button
              type="button"
              className={styles.pick}
              onClick={onOpenSettings}
              title="模型设置"
            >
              {model}
            </button>
            <button
              type="button"
              className={styles.send}
              aria-label="发送"
              disabled={busy || (!text.trim() && files.length === 0)}
              onClick={submit}
            >
              <ArrowUp size={11} />
            </button>
          </div>

          <input
            ref={fileRef}
            type="file"
            multiple
            hidden
            onChange={(event) => {
              const picked = Array.from(event.target.files ?? []).map(toAttachment);
              if (picked.length) onFilesChange([...files, ...picked]);
              event.target.value = "";
            }}
          />
        </div>
        <div className={styles.hint}>
          <kbd>Enter</kbd> 发送 · <kbd>Shift</kbd>
          <kbd>Enter</kbd> 换行
        </div>
      </div>
    </div>
  );
}
