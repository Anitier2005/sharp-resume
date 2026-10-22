import type { ReactNode } from "react";
import type { ChatAction, ChatMessage } from "@/types";
import { Check, FileIcon, Plus } from "@/components/ui/icons";

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 把数字、百分比、倍数标成重点，模拟编辑器里的高亮 */
function highlight(text: string): ReactNode[] {
  const parts = text.split(/([+＋]?\d+(?:\.\d+)?%|\d+(?:\.\d+)?\s?(?:步|倍|万|亿))/g);
  return parts.map((part, index) =>
    /^[+＋]?\d/.test(part) ? <b key={index}>{part}</b> : <span key={index}>{part}</span>,
  );
}

export function UserTurn({ message }: { message: ChatMessage }) {
  return (
    <div className="turn-user">
      {message.attachments?.map((file) => (
        <div className="attach" key={file.id} title={file.name}>
          <FileIcon size={10} />
          <span className="an">{file.name}</span>
          <span className="sz">{formatSize(file.size)}</span>
        </div>
      ))}
      {message.content && <div className="prompt">{message.content}</div>}
    </div>
  );
}

export function AssistantTurn({
  message,
  onAction,
}: {
  message: ChatMessage;
  onAction: (action: ChatAction) => void;
}) {
  return (
    <div className="turn-ai">
      {message.tool && (
        <div className="tool">
          <Check size={11} />
          {message.tool}
        </div>
      )}
      {message.content && <p>{message.content}</p>}

      {message.changes?.map((change, index) => (
        <div className="change" key={index}>
          <span className="lb">{change.label}</span>
          <span className="old">{change.before}</span>
          <span className="new">{highlight(change.after)}</span>
        </div>
      ))}

      {message.actions && message.actions.length > 0 && (
        <div className="acts">
          {message.actions.map((action) => (
            <button
              key={action.id}
              type="button"
              className={`act${action.kind === "apply" ? " primary" : ""}`}
              onClick={() => onAction(action)}
            >
              {action.kind === "apply" ? <Check size={12} /> : <Plus size={12} />}
              {action.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function TypingTurn() {
  return (
    <div className="turn-ai">
      <div className="typing">
        <span />
        <span />
        <span />
      </div>
    </div>
  );
}

export function EmptyTurn({ name, onPick }: { name: string; onPick: (text: string) => void }) {
  return (
    <div className="empty-turn">
      <p>这是 {name} 的独立会话。</p>
      <p className="dim">
        描述要改的地方，或拖入 JD、旧简历、截图。同一候选人的其他简历不会受到影响，但经历库是共用的。
      </p>
      <div className="empty-hints">
        {["按 JD 重写摘要", "把第二段改得更量化", "压缩到一页"].map((text) => (
          <button key={text} type="button" onClick={() => onPick(text)}>
            {text}
          </button>
        ))}
      </div>
    </div>
  );
}
