import type { ReactNode } from "react";
import type { ChatAction, ChatMessage } from "@/types";
import { Check, FileIcon, Plus } from "@/components/ui/icons";
import { segmentText } from "@/lib/inline-tokens";
import { Markdown } from "@/lib/markdown";
import { cx } from "@/lib/cx";
import styles from "./message.module.css";

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
  const attachments = message.attachments ?? [];
  // 有 token 的附件就长在正文里；没 token 的是旧消息，退回下面那排
  const inline = attachments.filter((file) => file.token);
  const legacy = attachments.filter((file) => !file.token);
  const segments = segmentText(message.content ?? "", inline);

  return (
    <div className={styles.turnUser}>
      {message.content && (
        <div className={styles.prompt} data-selectable>
          {segments.map((part, index) =>
            part.file ? (
              <span className={styles.inlineAsset} key={index} title={part.file.name}>
                <FileIcon size={9} />
                {part.file.name}
              </span>
            ) : (
              <span key={index}>{part.text}</span>
            ),
          )}
        </div>
      )}

      {legacy.map((file) => (
        <div className={styles.attach} key={file.id} title={file.name}>
          <FileIcon size={10} />
          <span className={styles.an}>{file.name}</span>
          <span className={styles.sz}>{formatSize(file.size)}</span>
        </div>
      ))}
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
    <div className={styles.turnAi}>
      {message.tool && (
        <div className={styles.tool}>
          <Check size={11} />
          {message.tool}
        </div>
      )}
      {message.content && <Markdown text={message.content} />}

      {message.changes?.map((change, index) => (
        <div className={styles.change} key={index}>
          <span className={styles.lb}>{change.label}</span>
          <span className={styles.old}>{change.before}</span>
          <span className={styles.new}>{highlight(change.after)}</span>
        </div>
      ))}

      {message.actions && message.actions.length > 0 && (
        <div className={styles.acts}>
          {message.actions.map((action) => (
            <button
              key={action.id}
              type="button"
              className={cx(styles.act, action.kind === "apply" && styles.actPrimary)}
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
    <div className={styles.turnAi}>
      <div className={styles.typing}>
        <span />
        <span />
        <span />
      </div>
    </div>
  );
}

export function EmptyTurn({ name, onPick }: { name: string; onPick: (text: string) => void }) {
  return (
    <div className={styles.emptyTurn}>
      <p>这是 {name} 的独立会话。</p>
      <p className={styles.dim}>
        描述要改的地方，或拖入 JD、旧简历、截图。同一候选人的其他简历不会受到影响，但经历库是共用的。
      </p>
      <div className={styles.emptyHints}>
        {["按 JD 重写摘要", "把第二段改得更量化", "压缩到一页"].map((text) => (
          <button key={text} type="button" onClick={() => onPick(text)}>
            {text}
          </button>
        ))}
      </div>
    </div>
  );
}
