import { invoke } from "@tauri-apps/api/core";
import type { Person, ResumePage } from "@/types";
import { displayName, fieldText } from "@/types";
import { headerOf, sectionsOf } from "@/lib/blocks";
import { isTauri } from "@/lib/storage";

/** 一页 → Markdown：字段就是字段，标题就是用户写的标题 */
export function pageToMarkdown(person: Person, page: ResumePage): string {
  const lines: string[] = [];
  const [lead, ...rest] = headerOf(page);

  if (lead) lines.push(`# ${lead.value || lead.label}`, "");
  for (const field of rest) lines.push(fieldText(field));
  if (rest.length) lines.push("");

  for (const section of sectionsOf(page)) {
    if (section.title.trim()) lines.push(`## ${section.title}`, "");
    for (const entry of section.items) {
      const source = person.items.find((item) => item.id === entry.refId);
      const fields = entry.fields.length ? entry.fields : source?.fields ?? [];
      if (fields.length) {
        const [head, ...others] = fields;
        lines.push(`### ${head.value || head.label}`, "");
        for (const field of others) lines.push(`- ${fieldText(field)}`);
      }
      for (const bullet of entry.bullets) {
        if (bullet.trim()) lines.push(`- ${bullet}`);
      }
      lines.push("");
    }
  }

  return lines.join("\n").trimEnd() + "\n";
}

export function resumeToMarkdown(person: Person, pages: ResumePage[]): string {
  return pages.map((page) => pageToMarkdown(person, page)).join("\n\n---\n\n");
}

/** 保存文本：桌面端弹系统保存框，浏览器端走下载 */
export async function saveText(defaultName: string, content: string): Promise<string | null> {
  if (isTauri()) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({
      defaultPath: defaultName,
      filters: [{ name: "Markdown", extensions: ["md", "txt"] }],
    });
    if (!path) return null;
    await invoke("save_text_file", { path, content });
    return path;
  }

  const blob = new Blob([content], { type: "text/markdown;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = defaultName;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  return defaultName;
}

/** 保存二进制（DOCX 等）：桌面端弹系统保存框，浏览器端走下载 */
export async function saveBytes(
  defaultName: string,
  bytes: Uint8Array,
  filterName: string,
  extensions: string[],
  mime = "application/octet-stream",
): Promise<string | null> {
  if (isTauri()) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({ defaultPath: defaultName, filters: [{ name: filterName, extensions }] });
    if (!path) return null;
    await invoke("save_binary_file", { path, data: Array.from(bytes) });
    return path;
  }

  const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  const blob = new Blob([buffer as ArrayBuffer], { type: mime });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = defaultName;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  return defaultName;
}

/** 打印当前页；在系统对话框里选「另存为 PDF」即导出 */
export function printPage(): void {
  window.print();
}

export const personTitle = (person: Person) => displayName(person.fields);
