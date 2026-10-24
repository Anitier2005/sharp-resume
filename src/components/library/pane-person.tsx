import { Field, Grid, PaneHead } from "./parts";
import { usePatchPerson } from "./use-library";
import styles from "./library.module.css";
import type { PaneProps } from "./pane-experience";

/** 人物基本信息：这一级字段被所有简历页共享 */
export function PersonPane({ person }: PaneProps) {
  const patch = usePatchPerson(person);

  return (
    <div className={styles.pane}>
      <PaneHead title="基本信息" desc="新建简历时会用这里的姓名、职位与城市预填页头。" />
      <div className={`${styles.itemBody} ${styles.plain}`}>
        <Grid>
          <Field label="姓名" value={person.name} onChange={(value) => patch({ name: value })} />
          <Field
            label="Name"
            value={person.nameEn}
            onChange={(value) => patch({ nameEn: value })}
          />
          <Field label="职位" value={person.role} onChange={(value) => patch({ role: value })} />
          <Field
            label="Role"
            value={person.roleEn}
            onChange={(value) => patch({ roleEn: value })}
          />
          <Field label="邮箱" value={person.email} onChange={(value) => patch({ email: value })} />
          <Field
            label="城市"
            value={person.location}
            onChange={(value) => patch({ location: value })}
          />
          <Field
            label="年限"
            value={String(person.years)}
            onChange={(value) => patch({ years: Number(value.replace(/[^\d]/g, "")) || 0 })}
          />
        </Grid>
      </div>
    </div>
  );
}
