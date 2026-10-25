import type { PageKind, Person, Resume, ResumePage, Workspace } from "@/types";
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

export function newPage(person: Person, kind: PageKind): ResumePage {
  const isEn = kind === "en";
  return {
    id: uid("pg"),
    kind,
    label: kind === "zh" ? "中文简历" : kind === "en" ? "English" : "作品集",
    headline: isEn ? person.nameEn : person.name,
    role: isEn ? person.roleEn : person.role,
    meta: [person.email, isEn ? "Shanghai" : person.location, `${person.years} ${isEn ? "years" : "年经验"}`],
    summary: "",
    // 新页默认把库里的经历都挂上，但不预填措辞：措辞要按岗位自己写（表单里有「起草」按钮）
    entries: person.experiences.map((item) => ({
      id: uid("en"),
      experienceId: item.id,
      bullets: [],
    })),
    projectIds: person.projects.map((item) => item.id),
  };
}
