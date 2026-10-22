import type { ReactNode } from "react";

function renderInline(text: string, keyPrefix: string): ReactNode[] {
  return text
    .split(/(\*\*[^*]+\*\*)/g)
    .filter((part) => part.length > 0)
    .map((part, index) =>
      part.startsWith("**") && part.endsWith("**") ? (
        <strong key={`${keyPrefix}-b-${index}`} className="font-semibold text-foreground">
          {part.slice(2, -2)}
        </strong>
      ) : (
        <span key={`${keyPrefix}-t-${index}`}>{part}</span>
      ),
    );
}

/**
 * 极简 Markdown 渲染：只支持段落、无序列表与 **加粗**。
 * 真正的流式 Markdown 会在接入模型后再换成 react-markdown。
 */
export function MarkdownLite({ text }: { text: string }) {
  const blocks: ReactNode[] = [];
  const lines = text.split("\n");
  let listBuffer: string[] = [];

  const flushList = (key: string) => {
    if (listBuffer.length === 0) return;
    blocks.push(
      <ul key={key} className="my-1.5 space-y-1 pl-0.5">
        {listBuffer.map((item, i) => (
          <li key={`${key}-${i}`} className="flex gap-2">
            <span className="mt-[7px] size-1 shrink-0 rounded-full bg-muted-foreground/60" />
            <span className="min-w-0">{renderInline(item, `${key}-${i}`)}</span>
          </li>
        ))}
      </ul>,
    );
    listBuffer = [];
  };

  lines.forEach((line, index) => {
    const trimmed = line.trim();
    if (trimmed.startsWith("- ")) {
      listBuffer.push(trimmed.slice(2));
      return;
    }
    flushList(`ul-${index}`);
    if (trimmed.length === 0) {
      return;
    }
    blocks.push(
      <p key={`p-${index}`} className="mb-2 last:mb-0">
        {renderInline(trimmed, `p-${index}`)}
      </p>,
    );
  });
  flushList("ul-final");

  return <div className="leading-[1.78]">{blocks}</div>;
}
