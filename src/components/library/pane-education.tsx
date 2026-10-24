import { Plus } from "@/components/ui/icons";
import type { EducationItem } from "@/types";
import { Field, Grid, ItemHead, PaneHead } from "./parts";
import { move, patchById, uid } from "./helpers";
import { usePatchPerson } from "./use-library";
import styles from "./library.module.css";
import type { PaneProps } from "./pane-experience";

export function EducationPane({ person, en }: PaneProps) {
  const patch = usePatchPerson(person);
  const list = person.education;

  const add = () => {
    const item: EducationItem = {
      id: uid("ed"),
      school: "学校",
      schoolEn: "",
      degree: "学位",
      degreeEn: "",
      period: "2015 – 2019",
    };
    patch({ education: [...list, item] });
  };

  return (
    <div className={styles.pane}>
      <PaneHead
        title="教育"
        desc="所有简历页共用，按顺序出现在「教育」小节。"
        action={
          <button type="button" className={styles.add} onClick={add}>
            <Plus size={10} />
            新增
          </button>
        }
      />

      {list.length === 0 && <div className={styles.empty}>还没有教育经历。</div>}

      {list.map((item, index) => {
        const update = (next: Partial<EducationItem>) =>
          patch({ education: patchById(list, item.id, next) });

        return (
          <div className={styles.item} key={item.id}>
            <ItemHead
              index={index}
              title={en ? item.schoolEn || item.school : item.school}
              note={item.period}
              actions={{
                first: index === 0,
                last: index === list.length - 1,
                onUp: () => patch({ education: move(list, item.id, -1) }),
                onDown: () => patch({ education: move(list, item.id, 1) }),
                onRemove: () => patch({ education: list.filter((row) => row.id !== item.id) }),
              }}
            />
            <div className={styles.itemBody}>
              <Grid>
                <Field
                  label={en ? "School" : "学校"}
                  value={en ? item.schoolEn ?? "" : item.school}
                  onChange={(value) => update(en ? { schoolEn: value } : { school: value })}
                />
                <Field
                  label={en ? "Degree" : "学位"}
                  value={en ? item.degreeEn ?? "" : item.degree}
                  onChange={(value) => update(en ? { degreeEn: value } : { degree: value })}
                />
                <Field
                  label="时间"
                  value={item.period}
                  onChange={(value) => update({ period: value })}
                />
              </Grid>
            </div>
          </div>
        );
      })}
    </div>
  );
}
