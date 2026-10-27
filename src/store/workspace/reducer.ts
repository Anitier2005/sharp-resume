import type { Person, Workspace } from "@/types";
import { mapSections, newSectionBlock, withEntry, withHeader, withoutBlock } from "@/lib/blocks";
import type { Action } from "./types";
import { mapPage, mapResume, newPerson, newPage, newResume, touch, withExpanded } from "./helpers";

const patchPerson = (state: Workspace, personId: string, fn: (person: Person) => Person): Workspace => ({
  ...state,
  people: state.people.map((person) => (person.id === personId ? fn(person) : person)),
});

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
      return { ...state, ui: { ...state.ui, accent: action.accent, accentHex: action.accentHex } };

    case "setMode":
      return { ...state, ui: { ...state.ui, mode: action.mode } };

    case "setTextScale":
      return { ...state, ui: { ...state.ui, textScale: action.textScale } };

    case "patchUi":
      return { ...state, ui: { ...state.ui, ...action.patch } };

    case "createPerson": {
      const person = newPerson();
      return {
        ...state,
        people: [...state.people, person],
        ui: {
          ...state.ui,
          activePersonId: person.id,
          activeResumeId: person.resumes[0].id,
          expanded: withExpanded(state, person.id),
        },
      };
    }

    case "createResume":
      return patchPerson(state, action.personId, (person) => {
        const resume = newResume(person.id);
        return {
          ...person,
          resumes: [...person.resumes, resume],
        };
      });

    case "renameResume":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        title: action.title,
      }));

    case "deleteResume": {
      const person = state.people.find((item) => item.id === action.personId);
      const remaining = person?.resumes.filter((resume) => resume.id !== action.resumeId) ?? [];
      const wasActive = state.ui.activeResumeId === action.resumeId;
      return {
        ...state,
        people: state.people.map((item) =>
          item.id !== action.personId ? item : { ...item, resumes: remaining },
        ),
        ui: wasActive ? { ...state.ui, activeResumeId: remaining[0]?.id ?? null } : state.ui,
      };
    }

    case "deletePerson": {
      const remaining = state.people.filter((person) => person.id !== action.personId);
      const wasActive = state.ui.activePersonId === action.personId;
      if (!wasActive) return { ...state, people: remaining };
      const next = remaining[0] ?? null;
      return {
        ...state,
        people: remaining,
        ui: {
          ...state.ui,
          activePersonId: next?.id ?? null,
          activeResumeId: next?.resumes[0]?.id ?? null,
        },
      };
    }

    /* ---------- 人的字段与库 ---------- */

    case "personFields":
      return patchPerson(state, action.personId, (person) => ({ ...person, fields: action.fields }));

    case "libraryItems":
      return patchPerson(state, action.personId, (person) => ({ ...person, items: action.items }));

    case "personAssets":
      return patchPerson(state, action.personId, (person) => ({ ...person, assets: action.assets }));

    case "itemAssets":
      return patchPerson(state, action.personId, (person) => ({
        ...person,
        items: person.items.map((item) =>
          item.id === action.itemId ? { ...item, assets: action.assets } : item,
        ),
      }));

    /* ---------- 页面 ---------- */

    case "addPage":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages: [...resume.pages, newPage()],
      }));

    // 排版只由「页排版 agent」写：会话 agent 想改排版得走 request_layout
    case "pageStyle":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => ({
        ...page,
        style: action.style,
      }));

    case "removePage":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages:
          resume.pages.length <= 1
            ? resume.pages
            : resume.pages.filter((page) => page.id !== action.pageId),
      }));

    case "pageLabel":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => ({
        ...page,
        label: action.label,
      }));

    case "pageHeader":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        withHeader(page, action.header),
      );

    /* ---------- 分区与条目（都是块） ---------- */

    case "addSection":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => ({
        ...page,
        blocks: [...(page.blocks ?? []), newSectionBlock(action.title)],
      }));

    case "removeSection":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        withoutBlock(page, action.sectionId),
      );

    case "removeBlock":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        withoutBlock(page, action.blockId),
      );

    case "blockFields":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => ({
        ...page,
        blocks: (page.blocks ?? []).map((block) =>
          block.id === action.blockId ? { ...block, fields: action.fields } : block,
        ),
      }));

    case "sectionTitle":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        mapSections(page, (block) =>
          block.id === action.sectionId ? { ...block, title: action.title } : block,
        ),
      );

    case "sectionItems":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        mapSections(page, (block) =>
          block.id === action.sectionId ? { ...block, items: action.items } : block,
        ),
      );

    case "entryPatch":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        withEntry(page, action.entryId, action.patch),
      );

    /* ---------- 会话 ---------- */

    case "appendMessage":
      return patchPerson(state, action.personId, (person) => ({
        ...person,
        messages: [...person.messages, action.message],
      }));

    case "patchMessage":
      return patchPerson(state, action.personId, (person) => ({
        ...person,
        messages: person.messages.map((message) =>
          message.id === action.messageId ? { ...message, ...action.patch } : message,
        ),
      }));

    default:
      return state;
  }
}

void touch;
