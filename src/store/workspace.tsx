import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type {
  AccentKey,
  ChatMessage,
  Experience,
  PageKind,
  Person,
  Resume,
  ResumePage,
  Workspace,
} from "@/types";
import { SEED } from "@/data/seed";
import { loadWorkspace, saveWorkspace } from "@/lib/storage";

const uid = (prefix: string) => `${prefix}-${crypto.randomUUID().slice(0, 8)}`;

type Action =
  | { type: "hydrate"; workspace: Workspace }
  | { type: "selectPerson"; personId: string }
  | { type: "togglePerson"; personId: string }
  | { type: "selectResume"; personId: string; resumeId: string }
  | { type: "setAccent"; accent: AccentKey }
  | { type: "setMode"; mode: "light" | "dark" }
  | { type: "createPerson" }
  | { type: "createResume"; personId: string }
  | { type: "renameResume"; personId: string; resumeId: string; title: string }
  | { type: "deleteResume"; personId: string; resumeId: string }
  | { type: "addPage"; personId: string; resumeId: string; kind: PageKind }
  | { type: "removePage"; personId: string; resumeId: string; pageId: string }
  | { type: "updatePage"; personId: string; resumeId: string; pageId: string; patch: Partial<ResumePage> }
  | { type: "updateExperience"; personId: string; expId: string; patch: Partial<Experience> }
  | { type: "appendMessage"; personId: string; resumeId: string; message: ChatMessage }
  | { type: "patchMessage"; personId: string; resumeId: string; messageId: string; patch: Partial<ChatMessage> };

function withExpanded(state: Workspace, personId: string): string[] {
  const list = state.ui.expanded ?? [];
  return list.includes(personId) ? list : [...list, personId];
}

function touch(resume: Resume): Resume {
  return { ...resume, updatedAt: Date.now() };
}

function mapResume(
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

function newPage(person: Person, kind: PageKind): ResumePage {
  const isEn = kind === "en";
  return {
    id: uid("pg"),
    kind,
    label: kind === "zh" ? "中文简历" : kind === "en" ? "English" : "作品集",
    headline: isEn ? person.nameEn : person.name,
    role: isEn ? person.roleEn : person.role,
    meta: [person.email, isEn ? "Shanghai" : person.location, `${person.years} ${isEn ? "years" : "年经验"}`],
    summary: "",
    experienceIds: person.experiences.map((item) => item.id),
    projectIds: person.projects.map((item) => item.id),
  };
}

function reducer(state: Workspace, action: Action): Workspace {
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

export type SaveState = "idle" | "saving" | "saved";

interface WorkspaceContextValue {
  state: Workspace;
  dispatch: React.Dispatch<Action>;
  person: Person | null;
  resume: Resume | null;
  page: ResumePage | null;
  activePageId: string | null;
  setActivePageId: (id: string | null) => void;
  expanded: Set<string>;
  toggleExpanded: (personId: string) => void;
  saveState: SaveState;
  ready: boolean;
}

const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, SEED);
  const [ready, setReady] = useState(false);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [activePageId, setActivePageId] = useState<string | null>(null);
  const firstRun = useRef(true);

  // 启动时载入
  useEffect(() => {
    let alive = true;
    loadWorkspace().then((saved) => {
      if (!alive) return;
      if (saved && saved.version === 1 && saved.people?.length) {
        dispatch({ type: "hydrate", workspace: saved });
      }
      setReady(true);
    });
    return () => {
      alive = false;
    };
  }, []);

  const person = useMemo(
    () => state.people.find((item) => item.id === state.ui.activePersonId) ?? state.people[0] ?? null,
    [state.people, state.ui.activePersonId],
  );

  const resume = useMemo(() => {
    if (!person) return null;
    return person.resumes.find((item) => item.id === state.ui.activeResumeId) ?? person.resumes[0] ?? null;
  }, [person, state.ui.activeResumeId]);

  // 当前页：切简历时自动落到第一页
  const resolvedPageId = useMemo(() => {
    if (!resume) return null;
    if (activePageId && resume.pages.some((page) => page.id === activePageId)) return activePageId;
    return resume.pages[0]?.id ?? null;
  }, [resume, activePageId]);

  const page = useMemo(
    () => resume?.pages.find((item) => item.id === resolvedPageId) ?? null,
    [resume, resolvedPageId],
  );

  // 自动保存
  useEffect(() => {
    if (!ready) return;
    if (firstRun.current) {
      firstRun.current = false;
      return;
    }
    setSaveState("saving");
    const timer = window.setTimeout(() => {
      saveWorkspace(state)
        .then(() => setSaveState("saved"))
        .catch((error) => {
          console.error("[storage] 保存失败", error);
          setSaveState("idle");
        });
    }, 500);
    return () => window.clearTimeout(timer);
  }, [state, ready]);

  const expanded = useMemo(() => new Set(state.ui.expanded ?? []), [state.ui.expanded]);

  const toggleExpanded = useCallback(
    (personId: string) => dispatch({ type: "togglePerson", personId }),
    [],
  );

  const value: WorkspaceContextValue = {
    state,
    dispatch,
    person,
    resume,
    page,
    activePageId: resolvedPageId,
    setActivePageId,
    expanded,
    toggleExpanded,
    saveState,
    ready,
  };

  return <WorkspaceContext.Provider value={value}>{children}</WorkspaceContext.Provider>;
}

export function useWorkspace(): WorkspaceContextValue {
  const ctx = useContext(WorkspaceContext);
  if (!ctx) throw new Error("useWorkspace 必须在 WorkspaceProvider 内使用");
  return ctx;
}

export { uid };
