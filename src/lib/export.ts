import { invoke } from "@tauri-apps/api/core";
import type { Person, ResumePage, Workspace } from "@/types";
import { displayName, fieldText } from "@/types";
import { headerOf, sectionsOf } from "@/lib/blocks";
import { saveWorkspace } from "@/lib/storage";

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

/** 保存文本：弹系统保存框，取消返回 null */
export async function saveText(defaultName: string, content: string): Promise<string | null> {
  const { save } = await import("@tauri-apps/plugin-dialog");
  const path = await save({
    defaultPath: defaultName,
    filters: [{ name: "Markdown", extensions: ["md", "txt"] }],
  });
  if (!path) return null;
  await invoke("save_text_file", { path, content });
  return path;
}

/** 保存二进制（DOCX 等）：弹系统保存框，取消返回 null */
export async function saveBytes(
  defaultName: string,
  bytes: Uint8Array,
  filterName: string,
  extensions: string[],
): Promise<string | null> {
  const { save } = await import("@tauri-apps/plugin-dialog");
  const path = await save({ defaultPath: defaultName, filters: [{ name: filterName, extensions }] });
  if (!path) return null;
  await invoke("save_binary_file", { path, data: Array.from(bytes) });
  return path;
}

/** 打印当前页；在系统对话框里选「另存为 PDF」即导出 */
export function printPage(): void {
  window.print();
}

/**
 * 把当前预览导出成 PDF 文件。
 *
 * 与 `printPage` 的区别不是「多一步」而是「谁是权威」：
 *   - `printPage` 把选择权交给系统打印对话框 —— 用户可能是去打纸，也可能是去存 PDF，
 *     应用这边两条都不知道结果；
 *   - 这里是「点一下 → 得到一个文件」，走 WebView2 的打印管线（`PrintToPdf`），
 *     排版引擎与屏幕上那页完全同一个，所以成品就是所见的那一份。
 *
 * `marginMm` 由调用方给，因为纸面有两种：分页后的真 A4 自带 16mm 边距（这时传 0），
 * 单张预览靠 `@page { margin: 16mm }`（这时传 16）。
 * 传 0 表示「不要第二层边距」，不表示「不知道」。
 */
export async function savePdf(defaultName: string, marginMm: number): Promise<string | null> {
  const { save } = await import("@tauri-apps/plugin-dialog");
  const path = await save({
    defaultPath: defaultName,
    filters: [{ name: "PDF", extensions: ["pdf"] }],
  });
  if (!path) return null;
  await invoke("export_pdf", { input: { path, marginMm } });
  return path;
}

export const personTitle = (person: Person) => displayName(person.fields);

/* ---------- 整份导出成一个文件夹 ---------- */

export interface ExportOutcome {
  dirName: string;
  /** 这次写进去的文件名（含页旁挂的图片） */
  files: string[];
  /** 上次导出、这次已经不存在的页，被清掉的文件名 */
  removed: string[];
  /** 同名但内容被用户改过：没覆盖，另起了名字 */
  conflicts: string[];
}

/**
 * 把一份简历导出成一个文件夹：每页一个文件 + manifest + 页里的图片旁挂。
 *
 * **导出读的是库里那份工作区**（后端负责写文件），所以先把内存里的改动落盘，
 * 否则刚改的那一版不会被导出去。
 */
export async function exportResumeFolder(
  workspace: Workspace,
  personId: string,
  resumeId: string,
): Promise<ExportOutcome> {
  await saveWorkspace(workspace);
  return invoke<ExportOutcome>("export_resume", { input: { personId, resumeId } });
}
