import { useEffect, useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { Close } from "@/components/ui/icons";
import { displayName } from "@/types";
import shared from "@/components/ui/shared.module.css";
import styles from "./library.module.css";
import { FieldsPane } from "./pane-fields";
import { PersonPane } from "./pane-person";

type Tab = "person" | "items";

const TABS: { key: Tab; label: string }[] = [
  { key: "person", label: "人物" },
  { key: "items", label: "库" },
];

/**
 * 资料抽屉：只有两个面板。
 *
 * v4 不再按「经历 / 作品 / 教育 / 技能」分类 —— 那只是同一件事的四种叫法。
 * 也没了「中文 / EN」开关：记录当时用什么语言写就用什么语言，不做语言版本。
 */
export function LibraryDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { person, resume, page } = useWorkspace();
  const [tab, setTab] = useState<Tab>("person");

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open || !person) return null;

  const pageName = page ? `${resume?.title ?? ""} · ${page.label}` : "还没有打开简历";

  return (
    <div
      className={shared.veil}
      data-print="hide"
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <div className={styles.drawer} role="dialog" aria-modal="true" aria-label="资料">
        <header className={styles.head}>
          <div className={shared.overlayTitle}>
            <b>资料</b>
            <span>
              {displayName(person.fields)} · 名下 {person.resumes.length} 份简历共用
            </span>
          </div>
          <button type="button" className={shared.overlayClose} aria-label="关闭" onClick={onClose}>
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
                {item.key === "items" && person.items.length > 0 && <i>{person.items.length}</i>}
              </button>
            ))}
          </nav>

          <div className={styles.main}>
            {tab === "person" ? <PersonPane person={person} /> : <FieldsPane person={person} />}
          </div>
        </div>

        <footer className={styles.foot}>
          <span className={styles.dim}>当前页</span>
          <span className={styles.cur}>{pageName}</span>
          <span className={styles.sp} />
          <span className={styles.dim}>库属于这个人；页面里引用后，措辞归页面</span>
        </footer>
      </div>
    </div>
  );
}
