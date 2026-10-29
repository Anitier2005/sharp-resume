import type { Person, Resume, ResumePage, Workspace } from "@/types";
import { uid } from "@/lib/uid";

export function withExpanded(state: Workspace, personId: string): string[] {
  const list = state.ui.expanded ?? [];
  return list.includes(personId) ? list : [...list, personId];
}

export function touch(resume: Resume): Resume {
  return { ...resume, updatedAt: Date.now() };
}

/** 对某个人做变换（材料这类不属于某一页的数据走这里） */
export function mapPerson(
  state: Workspace,
  personId: string,
  fn: (person: Person) => Person,
): Workspace {
  return {
    ...state,
    people: state.people.map((person) => (person.id === personId ? fn(person) : person)),
  };
}

export function mapResume(
  state: Workspace,
  personId: string,
  resumeId: string,
  fn: (resume: Resume) => Resume,
): Workspace {
  return {
    ...state,
    people: state.people.map((person) =>
      person.id !== personId
        ? person
        : {
            ...person,
            resumes: person.resumes.map((resume) =>
              resume.id !== resumeId ? resume : touch(fn(resume)),
            ),
          },
    ),
  };
}

/**
 * 空页 = 没有任何内容可丢。
 *
 * `assetId` / `layout` / `note` 都算内容：file 页只有 `assetId`、没有 block，
 * 只看「blocks 为空」会把它当空页删掉 —— 那是用户的原件。
 */
export function isEmptyPage(page: ResumePage): boolean {
  const hasBlocks = (page.blocks ?? []).length > 0;
  const hasHeader = (page.header ?? []).length > 0;
  const hasSections = (page.sections ?? []).length > 0;
  const hasNote = !!page.note?.trim();
  return !hasBlocks && !hasHeader && !hasSections && !hasNote && !page.assetId && !page.layout;
}

/** 对某一页做变换 */
export function mapPage(
  state: Workspace,
  personId: string,
  resumeId: string,
  pageId: string,
  fn: (page: ResumePage) => ResumePage,
): Workspace {
  return mapResume(state, personId, resumeId, (resume) => ({
    ...resume,
    pages: resume.pages.map((page) => (page.id === pageId ? fn(page) : page)),
  }));
}

/**
 * 新建页面：**零预设**。
 * 没有类型、没有语言、没有预置分区与字段 —— 用户自己起名、自己加字段加块。
 *
 * 页名留空：自动叫「第 7 页」既没有信息量，又会和「它现在排在第几页」打架
 * （见过一页叫「第 25 页」但排在第三位）。界面显示占位「未命名页」。
 *
 * `id` 由调用方给：agent 通道要在 dispatch 之后立刻指向这一页，
 * 只能事先把 id 定下来（见 Action.addPage 的注释）。
 */
export function newPage(id?: string, label?: string): ResumePage {
  return {
    id: id ?? uid("pg"),
    kind: "native",
    label: label?.trim() ?? "",
    blocks: [],
  };
}

/** `id` 由调用方给时照用：工具会在同一轮里引用它（订阅、改名、往里加页） */
export function newResume(personId: string, title?: string, id?: string): Resume {
  return {
    id: id ?? uid("r"),
    personId,
    title: title?.trim() || "未命名简历",
    updatedAt: Date.now(),
    pages: [newPage()],
  };
}

/** 新建人物：同样零预设，连「姓名」都不预置 */
export function newPerson(): Person {
  const id = uid("p");
  return {
    id,
    fields: [],
    items: [],
    messages: [],
    resumes: [newResume(id)],
  };
}
