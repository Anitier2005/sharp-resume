import { invoke } from "@tauri-apps/api/core";
import type { Person, ResumePage } from "@/types";
import { isTauri } from "@/lib/storage";

const LABELS = {
  zh: { experience: "工作经历", project: "项目", education: "教育", skill: "技能" },
  en: { experience: "Experience", project: "Selected Projects", education: "Education", skill: "Skills" },
};

export function pageToMarkdown(person: Person, page: ResumePage): string {
  const en = page.kind === "en";
  const label = LABELS[en ? "en" : "zh"];
  const lines: string[] = [];

  lines.push(`# ${page.headline}`, "");
  lines.push(`**${page.role}**`, "");
  if (page.meta.length) lines.push(page.meta.join(" · "), "");
  if (page.summary) lines.push(`> ${page.summary}`, "");

  const experiences = page.experienceIds
    .map((id) => person.experiences.find((item) => item.id === id))
    .filter(Boolean);

  if (experiences.length) {
    lines.push(`## ${label.experience}`, "");
    for (const exp of experiences) {
      if (!exp) continue;
      lines.push(`### ${exp.company} · ${exp.role}\u3000\`${exp.period}\``, "");
      const bullets = en ? exp.bulletsEn ?? exp.bullets : exp.bullets;
      for (const bullet of bullets) lines.push(`- ${bullet}`);
      lines.push("");
    }
  }

  const projects = page.projectIds
    .map((id) => person.projects.find((item) => item.id === id))
    .filter(Boolean);

  if (projects.length) {
    lines.push(`## ${label.project}`, "");
    for (const project of projects) {
      if (!project) continue;
      lines.push(`### ${project.title}\u3000\`${project.year}\``, "");
      lines.push(en ? project.descEn ?? project.desc : project.desc, "");
      if (project.tags.length) lines.push(`\`${project.tags.join("` `")}\``, "");
    }
  }

  if (person.education.length) {
    lines.push(`## ${label.education}`, "");
    for (const item of person.education) {
      lines.push(`- ${en ? item.schoolEn ?? item.school : item.school} · ${item.degree}\u3000\`${item.period}\``);
    }
    lines.push("");
  }

  if (person.skills.length) {
    lines.push(`## ${label.skill}`, "");
    for (const group of person.skills) {
      lines.push(`- **${group.label}**：${group.value}`);
    }
    lines.push("");
  }

  return lines.join("\n");
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
