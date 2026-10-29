import type { Workspace } from "@/types";
import { normalizeWorkspace } from "@/lib/blocks";

/**
 * 工作区迁移：这里**不再做内容迁移**。
 *
 * v1 → v5 的升级全部在后端 `model::adopt` 一处完成，前端只负责三件收尾的事：
 *   1. 结构自检：不是对象就报错，别把坏数据当成空工作区吞掉
 *   2. 补 `ui.openPages`：纯界面状态，后端不关心
 *   3. 把老的 header / sections 归一化成 blocks（老数据落到前端的最后一公里）
 *
 * 为什么不在这里再留一份：两份规则已经漂移过一次 —— 年限字段前端拼成了「3 年」，
 * 后端把数字当成空串直接丢掉。修好的方式是「只留一处」，不是「两边同步着改」。
 *
 * 更老的版本在这里直接报错：真正的迁移在 Rust 侧（`model::adopt`），
 * 前端只做结构自检与「老数据落到前端的最后一公里」，不在两边各迁一半。
 */
export function migrateWorkspace(raw: unknown): Workspace {
  const src = raw as Workspace | undefined;
  if (!src || typeof src !== "object" || !Array.isArray(src.people)) {
    throw new Error("工作区数据格式不对");
  }
  if (typeof src.version === "number" && src.version < 4) {
    throw new Error(
      `这份数据是老版本（v${src.version}），前端不做迁移 —— 用当前版本打开一次会自动升级`,
    );
  }

  const ui = Array.isArray(src.ui?.openPages)
    ? src.ui
    : { ...src.ui, openPages: initialOpenPages(src.ui, src.people) };

  return normalizeWorkspace({ ...src, version: 5, ui });
}

/**
 * 老数据没有 openPages：先按「当前简历的全部页」铺开。
 * 之后打开任意一页都会把它顶到最前，顺序由使用习惯自然接管。
 */
function initialOpenPages(ui: Workspace["ui"] | undefined, people: Workspace["people"]): string[] {
  const person = people.find((item) => item.id === ui?.activePersonId) ?? people[0];
  const resume =
    person?.resumes.find((item) => item.id === ui?.activeResumeId) ?? person?.resumes[0];
  return (resume?.pages ?? []).map((item) => item.id);
}
