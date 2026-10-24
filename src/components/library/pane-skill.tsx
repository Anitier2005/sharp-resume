import { Plus } from "@/components/ui/icons";
import type { SkillGroup } from "@/types";
import { Field, Grid, ItemHead, PaneHead } from "./parts";
import { move, patchById, uid } from "./helpers";
import { usePatchPerson } from "./use-library";
import styles from "./library.module.css";
import type { PaneProps } from "./pane-experience";

export function SkillPane({ person, en }: PaneProps) {
  const patch = usePatchPerson(person);
  const list = person.skills;

  const add = () => {
    const item: SkillGroup = {
      id: uid("sk"),
      label: "分组",
      labelEn: "",
      value: "",
      valueEn: "",
    };
    patch({ skills: [...list, item] });
  };

  return (
    <div className={styles.pane}>
      <PaneHead
        title="技能"
        desc="按分组罗列，纸面上是「左标签 + 右内容」两列。"
        action={
          <button type="button" className={styles.add} onClick={add}>
            <Plus size={10} />
            新增一组
          </button>
        }
      />

      {list.length === 0 && <div className={styles.empty}>还没有技能分组。</div>}

      {list.map((group, index) => {
        const update = (next: Partial<SkillGroup>) =>
          patch({ skills: patchById(list, group.id, next) });

        return (
          <div className={styles.item} key={group.id}>
            <ItemHead
              index={index}
              title={en ? group.labelEn || group.label : group.label}
              actions={{
                first: index === 0,
                last: index === list.length - 1,
                onUp: () => patch({ skills: move(list, group.id, -1) }),
                onDown: () => patch({ skills: move(list, group.id, 1) }),
                onRemove: () => patch({ skills: list.filter((row) => row.id !== group.id) }),
              }}
            />
            <div className={styles.itemBody}>
              <Grid>
                <Field
                  label={en ? "Label" : "分组"}
                  value={en ? group.labelEn ?? "" : group.label}
                  onChange={(value) => update(en ? { labelEn: value } : { label: value })}
                />
                <Field
                  label={en ? "Content" : "内容"}
                  wide
                  value={en ? group.valueEn ?? "" : group.value}
                  onChange={(value) => update(en ? { valueEn: value } : { value: value })}
                />
              </Grid>
            </div>
          </div>
        );
      })}
    </div>
  );
}
