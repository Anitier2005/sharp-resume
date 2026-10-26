import { useWorkspace } from "@/store/workspace";
import { FieldsEditor } from "@/components/ui/fields";
import type { Person } from "@/types";
import { PaneHead } from "./parts";
import styles from "./library.module.css";

/** 人物面板：这个人的信息同样只有字段，没有「姓名 / 邮箱 / 年限」这种预设 */
export function PersonPane({ person }: { person: Person }) {
  const { dispatch } = useWorkspace();
  return (
    <div className={styles.pane}>
      <PaneHead
        title="人物"
        desc="这里写的东西会出现在简历页头（如果那一页引用了它的话）。字段由你自己定。"
      />
      <div className={styles.itemBody}>
        <FieldsEditor
          fields={person.fields}
          onChange={(fields) => dispatch({ type: "personFields", personId: person.id, fields })}
          addLabel="加一个字段"
          placeholderLabel="例如 姓名 / 邮箱 / 现居"
        />
      </div>
    </div>
  );
}
