import { useWorkspace } from "@/store/workspace";
import { useToast } from "@/components/ui/toast-context";
import { Close, Plus } from "@/components/ui/icons";
import type { Experience, Person, ResumePage } from "@/types";
import { Field, Grid, ItemHead, PagePick, PaneHead } from "./parts";
import { move, uid } from "./helpers";
import { usePageToggle, usePatchPerson } from "./use-library";
import styles from "./library.module.css";

export interface PaneProps {
  person: Person;
  page: ResumePage | null;
  /** 是否在编辑英文版本 */
  en: boolean;
}

export function ExperiencePane({ person, page, en }: PaneProps) {
  const { dispatch } = useWorkspace();
  const toast = useToast();
  const patch = usePatchPerson(person);
  const toggleOnPage = usePageToggle();
  const list = person.experiences;

  const add = () => {
    const item: Experience = {
      id: uid("e"),
      company: "公司名称",
      companyEn: "",
      role: "职位",
      roleEn: "",
      period: "2023.01 – 至今",
      bullets: ["写一句你在这段经历里的产出"],
      bulletsEn: [],
    };
    patch({ experiences: [...list, item] });
  };

  return (
    <div className={styles.pane}>
      <PaneHead
        title="共享经历"
        desc={
          <>
            {person.name} 名下 {person.resumes.length} 份简历共用这一份，改动即时生效。
            {page?.kind === "portfolio" && " 当前是作品集页，不列经历。"}
          </>
        }
        action={
          <button type="button" className={styles.add} onClick={add}>
            <Plus size={10} />
            新增一段
          </button>
        }
      />

      {list.length === 0 && (
        <div className={styles.empty}>还没有经历。点右上「新增一段」开始。</div>
      )}

      {list.map((exp, index) => {
        const bulletsKey = en ? "bulletsEn" : "bullets";
        const bullets = (en ? exp.bulletsEn ?? [] : exp.bullets) ?? [];
        const onPage = page?.experienceIds.includes(exp.id) ?? false;
        const pickable = Boolean(page) && page?.kind !== "portfolio";

        const update = (next: Partial<Experience>) =>
          dispatch({
            type: "updateExperience",
            personId: person.id,
            expId: exp.id,
            patch: next,
          });

        return (
          <div className={styles.item} key={exp.id}>
            <ItemHead
              index={index}
              title={en ? exp.companyEn || exp.company : exp.company}
              note={exp.period}
              pick={
                pickable ? (
                  <PagePick
                    checked={onPage}
                    onChange={() => toggleOnPage("experienceIds", exp.id)}
                  />
                ) : undefined
              }
              actions={{
                first: index === 0,
                last: index === list.length - 1,
                onUp: () => patch({ experiences: move(list, exp.id, -1) }),
                onDown: () => patch({ experiences: move(list, exp.id, 1) }),
                onRemove: () => {
                  dispatch({ type: "removeExperience", personId: person.id, expId: exp.id });
                  toast("已删除这段经历");
                },
              }}
            />

            <div className={styles.itemBody}>
              <Grid>
                <Field
                  label={en ? "Company" : "公司"}
                  value={en ? exp.companyEn ?? "" : exp.company}
                  placeholder={en ? "Company name" : "公司名称"}
                  onChange={(value) => update(en ? { companyEn: value } : { company: value })}
                />
                <Field
                  label={en ? "Title" : "职位"}
                  value={en ? exp.roleEn ?? "" : exp.role}
                  placeholder={en ? "Job title" : "职位"}
                  onChange={(value) => update(en ? { roleEn: value } : { role: value })}
                />
                <Field
                  label="时间"
                  value={exp.period}
                  placeholder="2021.03 – 2024.09"
                  onChange={(value) => update({ period: value })}
                />
              </Grid>

              <div className={styles.bullets}>
                {bullets.map((bullet, bulletIndex) => (
                  <div className={styles.bullet} key={bulletIndex}>
                    <span className={styles.dot} />
                    <input
                      value={bullet}
                      spellCheck={false}
                      placeholder={en ? "Describe the outcome" : "描述这一段的产出"}
                      onChange={(event) => {
                        const next = bullets.map((item, i) =>
                          i === bulletIndex ? event.target.value : item,
                        );
                        update({ [bulletsKey]: next } as Partial<Experience>);
                      }}
                    />
                    <button
                      type="button"
                      aria-label="删除这一句"
                      onClick={() =>
                        update({
                          [bulletsKey]: bullets.filter((_, i) => i !== bulletIndex),
                        } as Partial<Experience>)
                      }
                    >
                      <Close size={9} />
                    </button>
                  </div>
                ))}

                <div className={styles.bulletFoot}>
                  <button
                    type="button"
                    onClick={() =>
                      update({ [bulletsKey]: [...bullets, ""] } as Partial<Experience>)
                    }
                  >
                    <Plus size={9} /> 加一句
                  </button>
                  {en && (exp.bulletsEn?.length ?? 0) === 0 && exp.bullets.length > 0 && (
                    <button type="button" onClick={() => update({ bulletsEn: [...exp.bullets] })}>
                      从中文复制
                    </button>
                  )}
                </div>
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
