import { useState, type ReactNode } from "react";
import type { Person, ResumeEntry, ResumePage } from "@/types";
import { useToast } from "@/components/ui/toast-context";
import { Dropdown, MenuItem } from "@/components/ui/menu";
import { ArrowDown, ArrowUp, Check, ChevronRight, Close, Plus, Trash } from "@/components/ui/icons";
import { attachedIds, draftFromNotes, resolveEntries } from "@/lib/entries";
import { cx } from "@/lib/cx";
import shared from "@/components/ui/shared.module.css";
import styles from "./form.module.css";

/**
 * 编辑态：表单。
 *
 * 这里只改数据，不碰纸面。简历措辞（entry.bullets）属于当前这一页，
 * 经历库里的 notes / facts 只是素材，用「查看记录」展开对照着改。
 */
interface FormProps {
  person: Person;
  page: ResumePage;
  onPatchPage: (patch: Partial<ResumePage>) => void;
  onAddEntry: (experienceId: string) => void;
  onRemoveEntry: (entryId: string) => void;
  onMoveEntry: (entryId: string, dir: -1 | 1) => void;
  onUpdateEntry: (entryId: string, patch: Partial<ResumeEntry>) => void;
  onToggleProject: (projectId: string) => void;
  onMoveProject: (projectId: string, dir: -1 | 1) => void;
}

type SectionKey = "head" | "experience" | "project" | "education" | "skill";

const DEFAULT_OPEN: Record<SectionKey, boolean> = {
  head: true,
  experience: true,
  project: false,
  education: false,
  skill: false,
};

function Section({
  title,
  hint,
  count,
  open,
  onToggle,
  children,
}: {
  title: string;
  hint?: string;
  count?: string;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <div className={styles.sec}>
      <button type="button" className={styles.head} onClick={onToggle} aria-expanded={open}>
        <ChevronRight size={10} className={cx(styles.chev, open && styles.chevOn)} />
        <b>{title}</b>
        {hint && <span className={styles.hint}>{hint}</span>}
        {count && <span className={styles.cnt}>{count}</span>}
      </button>
      {open && <div className={styles.body}>{children}</div>}
    </div>
  );
}

export function ResumeForm({
  person,
  page,
  onPatchPage,
  onAddEntry,
  onRemoveEntry,
  onMoveEntry,
  onUpdateEntry,
  onToggleProject,
  onMoveProject,
}: FormProps) {
  const toast = useToast();
  const [open, setOpen] = useState(DEFAULT_OPEN);
  const [peek, setPeek] = useState<Record<string, boolean>>({});
  const en = page.kind === "en";
  const rows = resolveEntries(person, page);
  const taken = attachedIds(page);
  const spare = person.experiences.filter((item) => !taken.has(item.id));

  const toggle = (key: SectionKey) => setOpen((prev) => ({ ...prev, [key]: !prev[key] }));

  const patchMeta = (index: number, value: string) => {
    const next = [...page.meta];
    if (value.trim()) next[index] = value;
    else next.splice(index, 1);
    onPatchPage({ meta: next });
  };

  /* ---------- 页头 ---------- */
  const headSection = (
    <Section
      title="页头"
      hint="姓名 · 岗位 · 联系方式"
      open={open.head}
      onToggle={() => toggle("head")}
    >
      <div className={styles.two}>
        <label className={shared.fld}>
          <span className={shared.fldK}>{en ? "Name" : "姓名"}</span>
          <input
            value={page.headline}
            placeholder={en ? "Your name" : "姓名"}
            spellCheck={false}
            onChange={(event) => onPatchPage({ headline: event.target.value })}
          />
        </label>
        <label className={shared.fld}>
          <span className={shared.fldK}>{en ? "Target role" : "目标岗位"}</span>
          <input
            value={page.role}
            placeholder={en ? "Product designer" : "这个简历投的岗位"}
            spellCheck={false}
            onChange={(event) => onPatchPage({ role: event.target.value })}
          />
        </label>
      </div>

      <div className={styles.metaGroup}>
        <span className={shared.fldK}>{en ? "Contact" : "联系方式"} · 逐行一项</span>
        {page.meta.map((item, index) => (
          <div className={styles.metaRow} key={index}>
            <input
              value={item}
              spellCheck={false}
              placeholder={en ? "email / city / years" : "邮箱 / 城市 / 年限"}
              onChange={(event) => patchMeta(index, event.target.value)}
            />
            <button type="button" aria-label="移除这一项" onClick={() => patchMeta(index, "")}>
              <Close size={9} />
            </button>
          </div>
        ))}
        <button
          type="button"
          className={styles.addRow}
          onClick={() => onPatchPage({ meta: [...page.meta, ""] })}
        >
          <Plus size={9} /> 加一项
        </button>
      </div>

      <label className={shared.fld}>
        <span className={shared.fldK}>{en ? "Summary" : "一句话摘要"} · 可留空</span>
        <textarea
          rows={2}
          value={page.summary ?? ""}
          placeholder={en ? "One line about you" : "一句话说清你是谁、能解决什么问题"}
          onChange={(event) => onPatchPage({ summary: event.target.value })}
        />
      </label>
    </Section>
  );

  /* ---------- 经历 ---------- */
  const experienceSection = (
    <Section
      title="经历"
      hint="措辞归当前这份简历"
      count={`${rows.length} 段`}
      open={open.experience}
      onToggle={() => toggle("experience")}
    >
      {rows.length === 0 && (
        <div className={styles.emptyNote}>
          这一页还没放经历。下面「加入经历」从库里挑一段，公司 / 职位 / 时间会自动带上。
        </div>
      )}

      {rows.map(({ entry, exp }, index) => {
        const bullets = en ? entry.bulletsEn ?? [] : entry.bullets;
        const write = (next: string[]) =>
          onUpdateEntry(entry.id, en ? { bulletsEn: next } : { bullets: next });
        const draft = draftFromNotes(exp.notes);
        const showRecord = peek[entry.id] ?? false;

        return (
          <div className={styles.entry} key={entry.id}>
            <div className={styles.entryHead}>
              <span className={styles.idx}>{String(index + 1).padStart(2, "0")}</span>
              <span className={styles.who}>
                {(en ? exp.companyEn || exp.company : exp.company) || "未命名公司"} ·{" "}
                {(en ? exp.roleEn || exp.role : exp.role) || "职位"}
              </span>
              <span className={styles.period}>{exp.period}</span>
              <span className={styles.acts}>
                <button
                  type="button"
                  aria-label="上移"
                  disabled={index === 0}
                  onClick={() => onMoveEntry(entry.id, -1)}
                >
                  <ArrowUp size={10} />
                </button>
                <button
                  type="button"
                  aria-label="下移"
                  disabled={index === rows.length - 1}
                  onClick={() => onMoveEntry(entry.id, 1)}
                >
                  <ArrowDown size={10} />
                </button>
                <button
                  type="button"
                  className={styles.rm}
                  aria-label="从本页移除（不会删库里的记录）"
                  onClick={() => onRemoveEntry(entry.id)}
                >
                  <Trash size={10} />
                </button>
              </span>
            </div>

            <div className={styles.bullets}>
              {bullets.map((bullet, i) => (
                <div className={styles.bulletRow} key={i}>
                  <span className={styles.dot} />
                  <textarea
                    rows={2}
                    value={bullet}
                    spellCheck={false}
                    placeholder={en ? "What should this role notice here?" : "这个岗位要强调什么"}
                    onChange={(event) => {
                      const next = bullets.map((text, at) =>
                        at === i ? event.target.value : text,
                      );
                      write(next);
                    }}
                  />
                  <button
                    type="button"
                    aria-label="删掉这一句"
                    onClick={() => write(bullets.filter((_, at) => at !== i))}
                  >
                    <Close size={9} />
                  </button>
                </div>
              ))}

              {bullets.length === 0 && (
                <div className={styles.emptyNote}>
                  还没有措辞。可以直接写一句，或按库里的记录起草再改。
                </div>
              )}

              <div className={styles.btnRow}>
                <button type="button" className={styles.addRow} onClick={() => write([...bullets, ""])}>
                  <Plus size={9} /> 加一句
                </button>
                <button
                  type="button"
                  className={styles.addRow}
                  disabled={draft.length === 0}
                  title={draft.length === 0 ? "这段经历在库里还没有记录" : "把记录里的句子搬过来当草稿"}
                  onClick={() => {
                    const fresh = draft.filter((line) => !bullets.includes(line));
                    if (!fresh.length) {
                      toast("记录里的句子都已经在用了");
                      return;
                    }
                    write([...bullets, ...fresh]);
                    toast(`搬了 ${fresh.length} 句，接着改成这个岗位要的说法`);
                  }}
                >
                  从记录起草
                </button>
              </div>
            </div>

            <div className={styles.record}>
              <button
                type="button"
                className={styles.recordHead}
                aria-expanded={showRecord}
                onClick={() => setPeek((prev) => ({ ...prev, [entry.id]: !showRecord }))}
              >
                <ChevronRight size={9} className={cx(styles.chev, showRecord && styles.chevOn)} />
                查看库里的记录
              </button>
              {showRecord && (
                <div className={styles.recordBody}>
                  {exp.notes.trim() ? (
                    <p className={styles.notes}>{exp.notes}</p>
                  ) : (
                    <div className={styles.recordEmpty}>这段经历在库里还没有详实记录。</div>
                  )}
                  {exp.facts.length > 0 && (
                    <div className={styles.facts}>
                      {exp.facts.map((fact) => (
                        <div className={styles.fact} key={fact.id}>
                          <b>{fact.label}</b>
                          <span>{fact.text}</span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
          </div>
        );
      })}

      <Dropdown
        width={200}
        trigger={
          <button type="button" className={styles.addRow}>
            <Plus size={9} /> 加入经历
          </button>
        }
      >
        {spare.length === 0 ? (
          <MenuItem label="库里没有可加的经历" onSelect={() => toast("先去经历库新增一段")} />
        ) : (
          spare.map((exp) => (
            <MenuItem key={exp.id} label={`${exp.company} · ${exp.role}`} onSelect={() => onAddEntry(exp.id)} />
          ))
        )}
      </Dropdown>
    </Section>
  );

  /* ---------- 作品 / 教育 / 技能 ---------- */
  const projectSection = (
    <Section
      title="作品"
      hint="勾选进本页，内容在经历库里改"
      count={`${page.projectIds.length} 件`}
      open={open.project}
      onToggle={() => toggle("project")}
    >
      {person.projects.length === 0 && <div className={styles.emptyNote}>还没有共享作品。</div>}
      <div className={styles.pickList}>
        {person.projects.map((project) => {
          const on = page.projectIds.includes(project.id);
          const at = page.projectIds.indexOf(project.id);
          return (
            <div className={cx(styles.pickRow, on && styles.on)} key={project.id}>
              <button
                type="button"
                role="checkbox"
                aria-checked={on}
                aria-label={project.title}
                className={styles.tick}
                onClick={() => onToggleProject(project.id)}
              >
                {on && <Check size={8} />}
              </button>
              <span className={styles.label}>{project.title}</span>
              <span className={styles.note}>{project.year}</span>
              {on && (
                <span className={styles.acts}>
                  <button
                    type="button"
                    aria-label="上移"
                    disabled={at <= 0}
                    onClick={() => onMoveProject(project.id, -1)}
                  >
                    <ArrowUp size={9} />
                  </button>
                  <button
                    type="button"
                    aria-label="下移"
                    disabled={at === page.projectIds.length - 1}
                    onClick={() => onMoveProject(project.id, 1)}
                  >
                    <ArrowDown size={9} />
                  </button>
                </span>
              )}
            </div>
          );
        })}
      </div>
    </Section>
  );

  const educationSection = (
    <Section
      title="教育"
      hint="人的一级，所有简历共用"
      count={`${person.education.length} 条`}
      open={open.education}
      onToggle={() => toggle("education")}
    >
      {person.education.length === 0 && (
        <div className={styles.emptyNote}>还没有教育经历，去经历库补。</div>
      )}
      {person.education.map((item) => (
        <div className={styles.readonly} key={item.id}>
          <b>{en ? item.schoolEn || item.school : item.school}</b>
          <span className={styles.k}>
            {" · "}
            {en ? item.degreeEn || item.degree : item.degree} · {item.period}
          </span>
        </div>
      ))}
    </Section>
  );

  const skillSection = (
    <Section
      title="技能"
      hint="人的一级，所有简历共用"
      count={`${person.skills.length} 组`}
      open={open.skill}
      onToggle={() => toggle("skill")}
    >
      {person.skills.length === 0 && <div className={styles.emptyNote}>还没有技能分组。</div>}
      {person.skills.map((group) => (
        <div className={styles.readonly} key={group.id}>
          <span className={styles.k}>{en ? group.labelEn || group.label : group.label}：</span>
          {en ? group.valueEn || group.value : group.value}
        </div>
      ))}
    </Section>
  );

  return (
    <div className={styles.form} data-print="hide">
      <div className={styles.scroll}>
        {headSection}
        {page.kind === "portfolio" ? projectSection : experienceSection}
        {page.kind === "portfolio" ? null : projectSection}
        {page.kind === "portfolio" ? null : educationSection}
        {page.kind === "portfolio" ? null : skillSection}
      </div>
    </div>
  );
}
