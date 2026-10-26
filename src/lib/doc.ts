import type { Field, Person, ResumePage } from "@/types";

/** 当前页可见内容的字数（不含空白），用于状态栏 */
export function countChars(person: Person, page: ResumePage | null): number {
  if (!page) return 0;
  const chunks: string[] = [];

  const push = (fields: Field[]) => chunks.push(...fields.flatMap((field) => [field.label, field.value]));
  push(page.header);

  for (const section of page.sections) {
    chunks.push(section.title);
    for (const entry of section.items) {
      push(entry.fields);
      if (!entry.fields.length) {
        const source = person.items.find((item) => item.id === entry.refId);
        if (source) push(source.fields);
      }
      chunks.push(...entry.bullets);
    }
  }

  return chunks.join("").replace(/\s/g, "").length;
}

export function pageIndex(resume: { pages: ResumePage[] } | null, pageId: string | null): number {
  if (!resume || !pageId) return 0;
  const index = resume.pages.findIndex((page) => page.id === pageId);
  return index < 0 ? 0 : index;
}
