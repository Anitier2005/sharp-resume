import type { Person, Resume, Workspace } from "@/types";
import { uid } from "@/lib/uid";
import type { Action } from "./types";
import { mapResume, newPage, touch, withExpanded } from "./helpers";

/** 纯函数：状态变换只在这里，便于单测与推理 */
export function reducer(state: Workspace, action: Action): Workspace {
  switch (action.type) {
    case "hydrate":
      return action.workspace;

    case "selectPerson":
      return { ...state, ui: { ...state.ui, activePersonId: action.personId } };

    case "togglePerson": {
      const list = state.ui.expanded ?? [];
      return {
        ...state,
        ui: {
          ...state.ui,
          expanded: list.includes(action.personId)
            ? list.filter((id) => id !== action.personId)
            : [...list, action.personId],
        },
      };
    }

    case "selectResume":
      return {
        ...state,
        ui: {
          ...state.ui,
          activePersonId: action.personId,
          activeResumeId: action.resumeId,
          expanded: withExpanded(state, action.personId),
        },
      };

    case "setAccent":
      return { ...state, ui: { ...state.ui, accent: action.accent } };

    case "setMode":
      return { ...state, ui: { ...state.ui, mode: action.mode } };

    case "setTextScale":
      return { ...state, ui: { ...state.ui, textScale: action.textScale } };

    case "createPerson": {
      const id = uid("p");
      const person: Person = {
        id,
        name: "新候选人",
        nameEn: "New Candidate",
        role: "待填写职位",
        roleEn: "Role",
        email: "name@mail.com",
        location: "城市",
        years: 0,
        experiences: [],
        projects: [],
        education: [],
        skills: [],
        resumes: [],
      };
      const resume: Resume = {
        id: uid("r"),
        personId: id,
        title: "通用版",
        updatedAt: Date.now(),
        messages: [],
        pages: [newPage(person, "zh")],
      };
      person.resumes = [resume];
      return {
        ...state,
        people: [...state.people, person],
        ui: {
          ...state.ui,
          activePersonId: id,
          activeResumeId: resume.id,
          expanded: withExpanded(state, id),
        },
      };
    }

    case "createResume": {
      const person = state.people.find((item) => item.id === action.personId);
      if (!person) return state;
      const resume: Resume = {
        id: uid("r"),
        personId: person.id,
        title: `新简历 ${person.resumes.length + 1}`,
        updatedAt: Date.now(),
        messages: [],
        pages: [newPage(person, "zh"), newPage(person, "en")],
      };
      return {
        ...state,
        people: state.people.map((item) =>
          item.id !== person.id ? item : { ...item, resumes: [...item.resumes, resume] },
        ),
        ui: {
          ...state.ui,
          activePersonId: person.id,
          activeResumeId: resume.id,
          expanded: withExpanded(state, person.id),
        },
      };
    }

    case "renameResume":
      return {
        ...state,
        people: state.people.map((person) =>
          person.id !== action.personId
            ? person
            : {
                ...person,
                resumes: person.resumes.map((resume) =>
                  resume.id === action.resumeId ? touch({ ...resume, title: action.title }) : resume,
                ),
              },
        ),
      };

    case "deleteResume": {
      const person = state.people.find((item) => item.id === action.personId);
      const remaining = person?.resumes.filter((resume) => resume.id !== action.resumeId) ?? [];
      const wasActive = state.ui.activeResumeId === action.resumeId;
      return {
        ...state,
        people: state.people.map((item) =>
          item.id !== action.personId ? item : { ...item, resumes: remaining },
        ),
        ui: wasActive
          ? { ...state.ui, activeResumeId: remaining[0]?.id ?? null }
          : state.ui,
      };
    }

    case "addPage":
      return mapResume(state, action.personId, action.resumeId, (resume) => {
        const person = state.people.find((item) => item.id === action.personId);
        if (!person) return resume;
        return { ...resume, pages: [...resume.pages, newPage(person, action.kind)] };
      });

    case "removePage":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages: resume.pages.length <= 1 ? resume.pages : resume.pages.filter((page) => page.id !== action.pageId),
      }));

    case "updatePage":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages: resume.pages.map((page) =>
          page.id === action.pageId ? { ...page, ...action.patch } : page,
        ),
      }));

    case "updateExperience":
      return {
        ...state,
        people: state.people.map((person) =>
          person.id !== action.personId
            ? person
            : {
                ...person,
                experiences: person.experiences.map((item) =>
                  item.id === action.expId ? { ...item, ...action.patch } : item,
                ),
              },
        ),
      };

    /** 批量改「人」这一级：增删经历 / 作品 / 教育 / 技能都走这里 */
    case "patchPerson":
      return {
        ...state,
        people: state.people.map((person) =>
          person.id !== action.personId ? person : { ...person, ...action.patch },
        ),
      };

    /** 删一段经历：同时把它从这个人所有简历的所有页面里摘掉，避免悬空引用 */
    case "removeExperience":
      return {
        ...state,
        people: state.people.map((person) => {
          if (person.id !== action.personId) return person;
          return {
            ...person,
            experiences: person.experiences.filter((item) => item.id !== action.expId),
            resumes: person.resumes.map((resume) => ({
              ...resume,
              pages: resume.pages.map((page) => ({
                ...page,
                entries: page.entries.filter((entry) => entry.experienceId !== action.expId),
              })),
            })),
          };
        }),
      };

    /* ---------- 本页经历条目（措辞属于简历这一级） ---------- */

    case "addEntry":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages: resume.pages.map((page) => {
          if (page.id !== action.pageId) return page;
          // 同一段经历在一页里只出现一次
          if (page.entries.some((entry) => entry.experienceId === action.experienceId)) return page;
          return {
            ...page,
            entries: [
              ...page.entries,
              {
                id: uid("en"),
                experienceId: action.experienceId,
                bullets: action.bullets ?? [],
              },
            ],
          };
        }),
      }));

    case "removeEntry":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages: resume.pages.map((page) =>
          page.id === action.pageId
            ? { ...page, entries: page.entries.filter((entry) => entry.id !== action.entryId) }
            : page,
        ),
      }));

    case "moveEntry":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages: resume.pages.map((page) => {
          if (page.id !== action.pageId) return page;
          const list = [...page.entries];
          const from = list.findIndex((entry) => entry.id === action.entryId);
          const to = from + action.dir;
          if (from < 0 || to < 0 || to >= list.length) return page;
          [list[from], list[to]] = [list[to], list[from]];
          return { ...page, entries: list };
        }),
      }));

    case "updateEntry":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages: resume.pages.map((page) =>
          page.id === action.pageId
            ? {
                ...page,
                entries: page.entries.map((entry) =>
                  entry.id === action.entryId ? { ...entry, ...action.patch } : entry,
                ),
              }
            : page,
        ),
      }));

    case "togglePageItem":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages: resume.pages.map((page) => {
          if (page.id !== action.pageId) return page;
          const list = page[action.field];
          const next = list.includes(action.itemId)
            ? list.filter((id) => id !== action.itemId)
            : [...list, action.itemId];
          return { ...page, [action.field]: next };
        }),
      }));

    case "movePageItem":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages: resume.pages.map((page) => {
          if (page.id !== action.pageId) return page;
          const list = [...page[action.field]];
          const from = list.indexOf(action.itemId);
          const to = from + action.dir;
          if (from < 0 || to < 0 || to >= list.length) return page;
          [list[from], list[to]] = [list[to], list[from]];
          return { ...page, [action.field]: list };
        }),
      }));

    case "appendMessage":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        messages: [...resume.messages, action.message],
      }));

    case "patchMessage":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        messages: resume.messages.map((message) =>
          message.id === action.messageId ? { ...message, ...action.patch } : message,
        ),
      }));

    default:
      return state;
  }
}
