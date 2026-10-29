import { useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { Modal } from "@/components/ui/modal";
import { displayName } from "@/types";
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

  if (!open || !person) return null;

  const pageName = page ? `${resume?.title ?? ""} · ${page.label}` : "还没有打开简历";

  return (
    <Modal
      title="资料"
      subtitle={`${displayName(person.fields)} · 名下 ${person.resumes.length} 份简历共用 · ${pageName}`}
      onClose={onClose}
      size="lg"
      bodyClassName={styles.body}
    >
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
    </Modal>
  );
}
