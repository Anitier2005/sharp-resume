import type { PageKind, Person, ResumePage } from "@/types";

/** 当前页可见内容的字数（不含空白），用于状态栏 */
export function countChars(person: Person, page: ResumePage | null): number {
  if (!page) return 0;
  const chunks: string[] = [page.headline, page.role, ...page.meta, page.summary ?? ""];

  for (const id of page.experienceIds) {
    const exp = person.experiences.find((item) => item.id === id);
    if (!exp) continue;
    chunks.push(exp.company, exp.role, exp.period);
    chunks.push(...(page.kind === "en" ? exp.bulletsEn ?? exp.bullets : exp.bullets));
  }

  for (const id of page.projectIds) {
    const project = person.projects.find((item) => item.id === id);
    if (!project) continue;
    chunks.push(project.title, project.year, page.kind === "en" ? project.descEn ?? project.desc : project.desc);
  }

  for (const item of person.education) {
    chunks.push(item.school, item.degree, item.period);
  }

  return chunks.join("").replace(/\s/g, "").length;
}

export function pageIndex(resume: { pages: ResumePage[] } | null, pageId: string | null): number {
  if (!resume || !pageId) return 0;
  const index = resume.pages.findIndex((page) => page.id === pageId);
  return index < 0 ? 0 : index;
}

export function kindFromIndex(index: number): PageKind {
  return (["zh", "en", "portfolio"] as PageKind[])[index % 3];
}
