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
 * 把当前预览写成 PDF，路径已知时不弹框。
 *
 * 整份导出要逐页写很多个文件，一页弹一次保存框等于逼用户点十次「确定」，
 * 所以把「选路径」和「写文件」拆开：单个导出走 `savePdf`，整份导出自己算路径。
 */
export async function writePdf(path: string, marginMm: number): Promise<string> {
  await invoke("export_pdf", { input: { path, marginMm } });
  return path;
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
  return writePdf(path, marginMm);
}

/** Windows 文件名里不能出现的字符。简历标题是用户随手写的，什么都可能有 */
function safeSegment(raw: string): string {
  const cleaned = raw.replace(/[\\/:*?"<>|]/g, " ").replace(/\s+/g, " ").trim();
  return cleaned.slice(0, 80) || "简历";
}

/**
 * 整份 PDF 的落地目录：`<数据目录>/导出 PDF/<人 - 简历>/`。
 *
 * 为什么不弹目录框：整份导出的产物是「一个文件夹」，用户要的是「打开就能看到」，
 * 不是「再选一次位置」。「Word · 整份」已经这么做了（后端直接写进 `<数据目录>/导出/`），
 * 两种格式各弹一次框只会让同一件事看起来像两件事。
 *
 * 为什么不和 Word 挤同一个目录：那个目录归 `export_resume` 的 manifest 管，
 * 旁边多出几个不是它写的文件，下次整份导出会判「这里有别的东西」而拒绝执行。
 */
export async function pdfExportDir(personName: string, resumeTitle: string): Promise<string> {
  // 字段名是 `data_dir`：AppInfo 没有 rename_all，serde 按 Rust 字段原名输出。
  // 读成 dataDir 会得到 undefined，拼出 `undefined\导出 PDF\...` 这种路径 ——
  // 于是「导出成功」却什么也没写出来，比直接报错更难查。这里是唯一读它的地方，
  // 缺字段就当场报，不往下传一个坏路径。
  const info = await invoke<{ data_dir?: string }>("app_info");
  const root = info?.data_dir;
  if (!root) throw new Error("拿不到数据目录，不知道该把 PDF 放到哪");
  return `${root}\\导出 PDF\\${safeSegment(`${personName}-${resumeTitle}`)}`;
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
