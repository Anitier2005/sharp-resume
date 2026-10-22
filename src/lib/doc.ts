import type { Doc } from "@/types";

/** 粗略字数统计：忽略空白字符，用于状态栏展示。 */
export function countChars(doc: Doc): number {
  let total = 0;
  const walk = (value: unknown) => {
    if (typeof value === "string") {
      total += value.replace(/\s/g, "").length;
      return;
    }
    if (Array.isArray(value)) {
      value.forEach(walk);
      return;
    }
    if (value && typeof value === "object") {
      Object.values(value).forEach(walk);
    }
  };
  walk(doc);
  return total;
}
