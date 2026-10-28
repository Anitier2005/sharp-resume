import type { ResumePage } from "@/types";

/**
 * 给「用户自己新建的页」起一个不重名的名字。
 *
 * 页名是页签栏上唯一的识别信息：连点两次「+」得到两个「未命名页」之后，
 * 用户点开的是哪一页只能靠猜，改名前就一直糊着。所以第一页用 base，
 * 之后从 2 开始往后找一个没被占用的序号。
 *
 * agent 建的页仍然留空（见 `store/workspace/helpers.ts` 的 `newPage`）——
 * 起名是用户的活，不该由程序替他决定这页叫什么。
 */
export function nextPageLabel(pages: ResumePage[], base = "未命名页"): string {
  const trimmed = base.trim() || "未命名页";
  const used = new Set(pages.map((page) => page.label.trim()));
  if (!used.has(trimmed)) return trimmed;
  // 有空名就一定有第一个空位：候选集有限，这里不会转不出去
  for (let n = 2; ; n += 1) {
    const candidate = `${trimmed} ${n}`;
    if (!used.has(candidate)) return candidate;
  }
}
