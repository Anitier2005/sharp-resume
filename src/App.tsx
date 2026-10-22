import { useEffect, useState } from "react";
import { Group, Panel, Separator } from "react-resizable-panels";
import { Toaster, toast } from "sonner";

import { AppTitleBar } from "@/components/app-title-bar";
import { ChatPanel } from "@/components/chat-panel";
import { ResumeListPanel } from "@/components/resume-list-panel";
import { ResumePanel } from "@/components/resume-panel";
import { emptyEnDoc, emptyPortfolioDoc, emptyZhDoc, mockResumes } from "@/data/mock";
import type { Doc, PageKind, ResumeItem, ResumePage } from "@/types";
import { PAGE_KIND_META } from "@/types";

const ACCENTS = [
  "from-violet-500 to-indigo-500",
  "from-sky-500 to-cyan-500",
  "from-amber-500 to-orange-500",
  "from-rose-500 to-pink-500",
  "from-emerald-500 to-teal-500",
];

function badgeOf(kind: PageKind) {
  if (kind === "resume-zh") return "中";
  if (kind === "resume-en") return "EN";
  return "PORT";
}

export default function App() {
  const [theme, setTheme] = useState<"light" | "dark">(() => {
    const saved = window.localStorage.getItem("sharp-resume-theme");
    if (saved === "light" || saved === "dark") return saved;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  });

  const [resumes, setResumes] = useState<ResumeItem[]>(mockResumes);
  const [activeResumeId, setActiveResumeId] = useState(mockResumes[0].id);
  const [activePageId, setActivePageId] = useState(mockResumes[0].pages[0].id);
  const [editing, setEditing] = useState(true);

  const activeResume = resumes.find((item) => item.id === activeResumeId) ?? resumes[0];
  const activePage =
    activeResume.pages.find((page) => page.id === activePageId) ?? activeResume.pages[0];

  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("dark", theme === "dark");
    root.style.colorScheme = theme;
    window.localStorage.setItem("sharp-resume-theme", theme);
  }, [theme]);

  const selectResume = (id: string) => {
    const target = resumes.find((item) => item.id === id);
    if (!target) return;
    setActiveResumeId(id);
    setActivePageId(target.pages[0].id);
  };

  const createResume = () => {
    const id = `resume-${Date.now()}`;
    const pageId = `${id}-p1`;
    const item: ResumeItem = {
      id,
      name: "未命名简历",
      role: "待完善",
      company: "未设置目标岗位",
      updatedAt: "刚刚",
      accent: ACCENTS[resumes.length % ACCENTS.length],
      pages: [
        {
          id: pageId,
          kind: "resume-zh",
          label: PAGE_KIND_META["resume-zh"].label,
          badge: badgeOf("resume-zh"),
          updatedAt: "刚刚",
          doc: structuredClone(emptyZhDoc),
        },
      ],
      messages: [],
    };
    setResumes((prev) => [item, ...prev]);
    setActiveResumeId(id);
    setActivePageId(pageId);
    toast.success("已新建简历", {
      description: "在会话区上传旧简历或 JD，我可以先帮你搭结构。",
    });
  };

  const addPage = (kind: PageKind) => {
    const template =
      kind === "portfolio" ? emptyPortfolioDoc : kind === "resume-en" ? emptyEnDoc : emptyZhDoc;
    const id = `page-${Date.now()}`;
    const page: ResumePage = {
      id,
      kind,
      label: PAGE_KIND_META[kind].label,
      badge: badgeOf(kind),
      updatedAt: "刚刚",
      doc: structuredClone(template),
    };
    setResumes((prev) =>
      prev.map((item) =>
        item.id === activeResume.id ? { ...item, pages: [...item.pages, page] } : item,
      ),
    );
    setActivePageId(id);
    toast.success(`已添加「${page.label}」`, {
      description: "这一页会作为独立文件导出，也会并入整份简历。",
    });
  };

  const removePage = (pageId: string) => {
    if (activeResume.pages.length <= 1) {
      toast.error("至少保留一页", { description: "一份简历需要至少一个页面才能导出。" });
      return;
    }
    const index = activeResume.pages.findIndex((page) => page.id === pageId);
    const nextPages = activeResume.pages.filter((page) => page.id !== pageId);
    setResumes((prev) =>
      prev.map((item) => (item.id === activeResume.id ? { ...item, pages: nextPages } : item)),
    );
    if (pageId === activePageId) {
      setActivePageId(nextPages[Math.max(0, index - 1)].id);
    }
    toast("已删除页面", { description: "原型阶段直接移除，功能阶段会进入回收站。" });
  };

  const updateDoc = (pageId: string, doc: Doc) => {
    setResumes((prev) =>
      prev.map((item) =>
        item.id !== activeResume.id
          ? item
          : {
              ...item,
              updatedAt: "刚刚",
              pages: item.pages.map((page) =>
                page.id === pageId ? { ...page, doc, updatedAt: "刚刚" } : page,
              ),
            },
      ),
    );
  };

  return (
    <div className="flex h-full flex-col overflow-hidden bg-background">
      <AppTitleBar
        theme={theme}
        onToggleTheme={() => setTheme((prev) => (prev === "light" ? "dark" : "light"))}
      />

      <div className="min-h-0 flex-1">
        <Group orientation="horizontal" className="h-full">
          <Panel defaultSize="17%" minSize="14%" maxSize="24%" className="min-w-0">
            <ResumeListPanel
              resumes={resumes}
              activeId={activeResume.id}
              onSelect={selectResume}
              onCreate={createResume}
            />
          </Panel>

          <Separator className="relative w-px bg-border transition-colors after:absolute after:inset-y-0 after:-left-[3px] after:w-[7px] after:content-[''] hover:bg-brand/50 data-[state=dragging]:bg-brand" />

          <Panel defaultSize="31%" minSize="26%" maxSize="44%" className="min-w-0">
            <ChatPanel
              resume={activeResume}
              pageLabel={activePage.label}
              pageCount={activeResume.pages.length}
            />
          </Panel>

          <Separator className="relative w-px bg-border transition-colors after:absolute after:inset-y-0 after:-left-[3px] after:w-[7px] after:content-[''] hover:bg-brand/50 data-[state=dragging]:bg-brand" />

          <Panel defaultSize="52%" minSize="36%" className="min-w-0">
            <ResumePanel
              resume={activeResume}
              activePageId={activePage.id}
              editing={editing}
              onEditingChange={setEditing}
              onSelectPage={setActivePageId}
              onAddPage={addPage}
              onRemovePage={removePage}
              onDocChange={updateDoc}
            />
          </Panel>
        </Group>
      </div>

      <Toaster
        theme={theme}
        position="bottom-right"
        toastOptions={{
          style: {
            fontSize: "12px",
          },
        }}
      />
    </div>
  );
}
