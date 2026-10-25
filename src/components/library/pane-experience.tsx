import { useWorkspace } from "@/store/workspace";
import { useToast } from "@/components/ui/toast-context";
import { Close, Plus } from "@/components/ui/icons";
import { blankExperience, FACT_LABELS, type Experience, type Person, type ResumePage } from "@/types";
import { Field, Grid, ItemHead, PagePick, PaneHead } from "./parts";
import { move, uid } from "./helpers";
import { useEntryToggle, usePatchPerson } from "./use-library";
import styles from "./library.module.css";

export interface PaneProps {
  person: Person;
  page: ResumePage | null;
  /** 是否在编辑英文版本 */
  en: boolean;
}

/**
 * 经历库：存「详实记录」，不存简历措辞。
 *
 * 记录写给自己看 —— 母语、口语、琐碎都行，越细越好；
 * 简历上那句要投某个岗位的话，去右侧表单里对着记录改。
 */
export function ExperiencePane({ person, page, en }: PaneProps) {
  const { dispatch } = useWorkspace();
  const toast = useToast();
  const patch = usePatchPerson(person);
  const toggleOnPage = useEntryToggle();
  const list = person.experiences;

  const add = () => {
    patch({ experiences: [...list, blankExperience(uid("e"))] });
  };

  return (
    <div className={styles.pane}>
      <PaneHead
        title="共享经历"
        desc={
          <>
            {person.name} 名下 {person.resumes.length} 份简历共用这一份记录，改动即时生效。
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
        const onPage = page?.entries.some((entry) => entry.experienceId === exp.id) ?? false;
        const pickable = Boolean(page) && page?.kind !== "portfolio";

        const update = (next: Partial<Experience>) =>
          dispatch({
            type: "updateExperience",
            personId: person.id,
            expId: exp.id,
            patch: next,
          });

        const setFact = (factId: string, patchFact: { label?: string; text?: string }) =>
          update({
            facts: exp.facts.map((fact) => (fact.id === factId ? { ...fact, ...patchFact } : fact)),
          });

        return (
          <div className={styles.item} key={exp.id}>
            <ItemHead
              index={index}
              title={en ? exp.companyEn || exp.company : exp.company}
              note={exp.period}
              pick={
                pickable ? <PagePick checked={onPage} onChange={() => toggleOnPage(exp.id)} /> : undefined
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

              <label className={styles.noteField}>
                <span className={styles.noteK}>详实记录 · 只存在这里，不会直接出现在简历上</span>
                <textarea
                  rows={6}
                  value={exp.notes}
                  spellCheck={false}
                  placeholder={
                    "想到什么写什么：当时业务在做什么、你负责哪块、做过哪些尝试、最后什么结果、\n" +
                    "和谁配合、踩过什么坑、哪些数字能佐证。可以粘聊天记录 / 周报 / 年终总结。"
                  }
                  onChange={(event) => update({ notes: event.target.value })}
                />
              </label>

              <div className={styles.facts}>
                <div className={styles.factsHead}>
                  <span>细节</span>
                  <span>一条一个点，改简历时按岗位挑</span>
                </div>
                {exp.facts.map((fact) => (
                  <div className={styles.factRow} key={fact.id}>
                    <input
                      className={styles.factLabel}
                      value={fact.label}
                      placeholder="背景"
                      list="exp-fact-labels"
                      spellCheck={false}
                      onChange={(event) => setFact(fact.id, { label: event.target.value })}
                    />
                    <input
                      className={styles.factText}
                      value={fact.text}
                      placeholder="例如：日活 120 万，投放配置平均 27 分钟一次"
                      spellCheck={false}
                      onChange={(event) => setFact(fact.id, { text: event.target.value })}
                    />
                    <button
                      type="button"
                      aria-label="删掉这条细节"
                      onClick={() => update({ facts: exp.facts.filter((row) => row.id !== fact.id) })}
                    >
                      <Close size={9} />
                    </button>
                  </div>
                ))}
                <button
                  type="button"
                  className={styles.addFact}
                  onClick={() =>
                    update({ facts: [...exp.facts, { id: uid("f"), label: "", text: "" }] })
                  }
                >
                  <Plus size={9} /> 加一条细节
                </button>
              </div>
            </div>
          </div>
        );
      })}

      <datalist id="exp-fact-labels">
        {FACT_LABELS.map((label) => (
          <option value={label} key={label} />
        ))}
      </datalist>
    </div>
  );
}
