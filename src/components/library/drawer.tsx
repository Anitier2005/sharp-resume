import { useEffect, useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { Close } from "@/components/ui/icons";
import shared from "@/components/ui/shared.module.css";
import styles from "./library.module.css";
import { ExperiencePane } from "./pane-experience";
import { ProjectPane } from "./pane-project";
import { EducationPane } from "./pane-education";
import { SkillPane } from "./pane-skill";
import { PersonPane } from "./pane-person";

type Tab = "person" | "experience" | "project" | "education" | "skill";

const TABS: { key: Tab; label: string }[] = [
  { key: "person", label: "人物" },
  { key: "experience", label: "经历" },
  { key: "project", label: "作品" },
  { key: "education", label: "教育" },
  { key: "skill", label: "技能" },
];

/**
 * 经历库抽屉：只负责外壳（遮罩、导航、语言开关、底栏）。
 * 每个面板自己从 store 取数据，五块内容互不影响，也不再挤在同一个文件里。
 */
export function LibraryDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { person, resume, page } = useWorkspace();
  const [tab, setTab] = useState<Tab>("experience");
  const [en, setEn] = useState(false);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // 切人时抽屉保持原样：语言开关是使用习惯，不随人重置
  if (!open || !person) return null;

  const counts: Record<Tab, number> = {
    person: 0,
    experience: person.experiences.length,
    project: person.projects.length,
    education: person.education.length,
    skill: person.skills.length,
  };

  const pageName = page ? `${resume?.title ?? ""} · ${page.label}` : "还没有打开简历";
  const paneProps = { person, page, en };

  return (
    <div
      className={shared.veil}
      data-print="hide"
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <div className={styles.drawer} role="dialog" aria-modal="true" aria-label="经历库">
        <header className={styles.head}>
          <div className={shared.overlayTitle}>
            <b>经历库</b>
            <span>
              {person.name} · 名下 {person.resumes.length} 份简历共用
            </span>
          </div>
          <div className={styles.switch} role="group" aria-label="编辑语言">
            <button type="button" aria-pressed={!en} onClick={() => setEn(false)}>
              中文
            </button>
            <button type="button" aria-pressed={en} onClick={() => setEn(true)}>
              EN
            </button>
          </div>
          <button
            type="button"
            className={shared.overlayClose}
            aria-label="关闭"
            onClick={onClose}
          >
            <Close size={11} />
          </button>
        </header>

        <div className={styles.body}>
          <nav className={styles.rail}>
            {TABS.map((item) => (
              <button
                key={item.key}
                type="button"
                aria-current={tab === item.key}
                onClick={() => setTab(item.key)}
              >
                {item.label}
                {counts[item.key] > 0 && <i>{counts[item.key]}</i>}
              </button>
            ))}
          </nav>

          <div className={styles.main}>
            {tab === "person" && <PersonPane {...paneProps} />}
            {tab === "experience" && <ExperiencePane {...paneProps} />}
            {tab === "project" && <ProjectPane {...paneProps} />}
            {tab === "education" && <EducationPane {...paneProps} />}
            {tab === "skill" && <SkillPane {...paneProps} />}
          </div>
        </div>

        <footer className={styles.foot}>
          <span className={styles.dim}>当前页</span>
          <span className={styles.cur}>{pageName}</span>
          <span className={styles.sp} />
          <span className={styles.dim}>勾选「本页」只影响这一页，其余改动同步所有简历</span>
        </footer>
      </div>
    </div>
  );
}
