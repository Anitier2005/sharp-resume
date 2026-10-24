import type { Person } from "@/types";

/** 生成带前缀的短 id */
export const uid = (prefix: string) => `${prefix}-${crypto.randomUUID().slice(0, 8)}`;

/** 列表内上移 / 下移一项，越界时原样返回 */
export function move<T extends { id: string }>(list: T[], id: string, dir: -1 | 1): T[] {
  const from = list.findIndex((item) => item.id === id);
  const to = from + dir;
  if (from < 0 || to < 0 || to >= list.length) return list;
  const next = [...list];
  [next[from], next[to]] = [next[to], next[from]];
  return next;
}

/** 只替换指定 id 的字段，其余保持同一引用，减少无谓重渲染 */
export function patchById<T extends { id: string }>(
  list: T[],
  id: string,
  patch: Partial<T>,
): T[] {
  return list.map((row) => (row.id === id ? { ...row, ...patch } : row));
}

/** 从 Person 上取某个数组字段的泛型键 */
export type PersonListKey = "experiences" | "projects" | "education" | "skills";

export type PatchPerson = (next: Partial<Person>) => void;

/** 页面上要勾选的引用字段 */
export type PageRefField = "experienceIds" | "projectIds";
