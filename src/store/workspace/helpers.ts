import type { Person, Resume, ResumePage, Workspace } from "@/types";
import { uid } from "@/lib/uid";

export function withExpanded(state: Workspace, personId: string): string[] {
  const list = state.ui.expanded ?? [];
  return list.includes(personId) ? list : [...list, personId];
}

export function touch(resume: Resume): Resume {
  return { ...resume, updatedAt: Date.now() };
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
 */
export function newPage(order: number): ResumePage {
  return {
    id: uid("pg"),
    kind: "native",
    label: `第 ${order} 页`,
    blocks: [],
  };
}

export function newResume(personId: string): Resume {
  return {
    id: uid("r"),
    personId,
    title: "未命名简历",
    updatedAt: Date.now(),
    pages: [newPage(1)],
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
