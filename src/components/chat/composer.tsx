import { useEffect, useRef, useState } from "react";
import type { Attachment } from "@/types";
import { ArrowUp, Close, FileIcon, Plus } from "@/components/ui/icons";
import { Dropdown, MenuItem } from "@/components/ui/menu";

const MODELS = ["Sonnet 4.5", "Opus 4.5", "GPT-5.2", "本地模型"];

function extOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

function toAttachment(file: File): Attachment {
  return {
    id: `f-${crypto.randomUUID().slice(0, 8)}`,
    name: file.name,
    size: file.size,
    ext: extOf(file.name),
  };
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

interface ComposerProps {
  model: string;
  onModelChange: (model: string) => void;
  onSend: (text: string, files: Attachment[]) => void;
  busy: boolean;
  /** 外部注入的草稿（点击建议动作时填入） */
  draft: string;
  onDraftConsumed: () => void;
  /** 待发送附件由外层持有，方便拖放注入 */
  files: Attachment[];
  onFilesChange: (files: Attachment[]) => void;
}

export function Composer({
  model,
  onModelChange,
  onSend,
  busy,
  draft,
  onDraftConsumed,
  files,
  onFilesChange,
}: ComposerProps) {
  const [text, setText] = useState("");
  const areaRef = useRef<HTMLTextAreaElement | null>(null);
  const fileRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!draft) return;
    setText(draft);
    onDraftConsumed();
    areaRef.current?.focus();
  }, [draft, onDraftConsumed]);

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
    setText("");
    onFilesChange([]);
  };

  return (
    <div className="composer">
      <div className="composer-in">
        <div className="box">
          {files.length > 0 && (
            <div className="pending-files">
              {files.map((file) => (
                <span className="pending" key={file.id}>
                  <FileIcon size={10} />
                  {file.name}
                  <span className="sz">{formatSize(file.size)}</span>
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
            className="input-area"
            rows={1}
            value={text}
            placeholder="描述你想改的地方，或拖入 JD、旧简历…"
            aria-label="消息输入"
            onChange={(event) => setText(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                submit();
              }
            }}
          />

          <div className="bar-row">
            <button
              type="button"
              className="plus"
              aria-label="添加文件"
              onClick={() => fileRef.current?.click()}
            >
              <Plus size={11} />
            </button>
            <Dropdown
              width={140}
              trigger={
                <button type="button" className="pick">
                  {model}
                </button>
              }
            >
              {MODELS.map((item) => (
                <MenuItem
                  key={item}
                  label={item}
                  hint={item === model ? "当前" : undefined}
                  onSelect={() => onModelChange(item)}
                />
              ))}
            </Dropdown>
            <button
              type="button"
              className="send"
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
        <div className="hint">
          <kbd>Enter</kbd> 发送 · <kbd>Shift</kbd>
          <kbd>Enter</kbd> 换行
        </div>
      </div>
    </div>
  );
}

export { toAttachment };
