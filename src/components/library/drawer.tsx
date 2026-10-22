import { useEffect, useState, type ReactNode } from "react";
import { useWorkspace } from "@/store/workspace";
import { useToast } from "@/components/ui/toast-context";
import { ArrowDown, ArrowUp, Check, Close, Plus, Trash } from "@/components/ui/icons";
import type { EducationItem, Experience, Person, ProjectItem, SkillGroup } from "@/types";

type Tab = "person" | "experience" | "project" | "education" | "skill";

const TABS: { key: Tab; label: string }[] = [
  { key: "person", label: "人物" },
  { key: "experience", label: "经历" },
  { key: "project", label: "作品" },
  { key: "education", label: "教育" },
  { key: "skill", label: "技能" },
];

const uid = (prefix: string) => `${prefix}-${crypto.randomUUID().slice(0, 8)}`;

interface FieldProps {
  label: string;
  value: string;
  placeholder?: string;
  onChange: (value: string) => void;
  wide?: boolean;
}

function Field({ label, value, placeholder, onChange, wide }: FieldProps) {
  return (
    <label className={`fld${wide ? " wide" : ""}`}>
      <span className="fld-k">{label}</span>
      <input
        value={value}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  );
}

/** 一组字段的容器：两列布局 */
function Grid({ children }: { children: ReactNode }) {
  return <div className="fld-grid">{children}</div>;
}

interface ItemActions {
  onUp: () => void;
  onDown: () => void;
  onRemove: () => void;
  first: boolean;
  last: boolean;
}

function ItemHead({
  index,
  title,
  note,
  pick,
  actions,
}: {
  index: number;
  title: string;
  note?: string;
  pick?: ReactNode;
  actions: ItemActions;
}) {
  return (
    <div className="lib-item-head">
      <span className="lib-idx">{String(index + 1).padStart(2, "0")}</span>
      <span className="lib-title">{title || "未命名"}</span>
      {note && <span className="lib-note">{note}</span>}
      <span className="lib-end">
        {pick}
        <span className="lib-acts">
          <button
            type="button"
            aria-label="上移"
            disabled={actions.first}
            onClick={actions.onUp}
          >
            <ArrowUp size={10} />
          </button>
          <button
            type="button"
            aria-label="下移"
            disabled={actions.last}
            onClick={actions.onDown}
          >
            <ArrowDown size={10} />
          </button>
          <button type="button" aria-label="删除" className="rm" onClick={actions.onRemove}>
            <Trash size={10} />
          </button>
        </span>
      </span>
    </div>
  );
}

/** 本页是否包含这一段（只在有当前页时出现） */
function PagePick({
  checked,
  onChange,
  label = "本页",
}: {
  checked: boolean;
  onChange: () => void;
  label?: string;
}) {
  return (
    <button
      type="button"
      className={`pick${checked ? " on" : ""}`}
      aria-pressed={checked}
      onClick={onChange}
    >
      <span className="box">{checked && <Check size={8} />}</span>
      {label}
    </button>
  );
}

interface DrawerProps {
  open: boolean;
  onClose: () => void;
}

export function LibraryDrawer({ open, onClose }: DrawerProps) {
  const { person, resume, page, dispatch } = useWorkspace();
  const toast = useToast();
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

  const patch = (next: Partial<Person>) =>
    dispatch({ type: "patchPerson", personId: person.id, patch: next });

  const move = <T extends { id: string }>(list: T[], id: string, dir: -1 | 1): T[] => {
    const from = list.findIndex((item) => item.id === id);
    const to = from + dir;
    if (from < 0 || to < 0 || to >= list.length) return list;
    const next = [...list];
    [next[from], next[to]] = [next[to], next[from]];
    return next;
  };

  const toggleOnPage = (field: "experienceIds" | "projectIds", itemId: string) => {
    if (!resume || !page) return;
    dispatch({
      type: "togglePageItem",
      personId: person.id,
      resumeId: resume.id,
      pageId: page.id,
      field,
      itemId,
    });
  };

  const counts: Record<Tab, number> = {
    person: 0,
    experience: person.experiences.length,
    project: person.projects.length,
    education: person.education.length,
    skill: person.skills.length,
  };

  const pageName = page ? `${resume?.title ?? ""} · ${page.label}` : "还没有打开简历";

  /* ---------------- 经历 ---------------- */

  const renderExperiences = () => {
    const list = person.experiences;

    const addExperience = () => {
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
      <div className="lib-pane">
        <div className="lib-pane-head">
          <div>
            <b>共享经历</b>
            <span>
              {person.name} 名下 {person.resumes.length} 份简历共用这一份，改动即时生效。
              {page?.kind === "portfolio" && " 当前是作品集页，不列经历。"}
            </span>
          </div>
          <button type="button" className="lib-add" onClick={addExperience}>
            <Plus size={10} />
            新增一段
          </button>
        </div>

        {list.length === 0 && (
          <div className="lib-empty">还没有经历。点右上「新增一段」开始。</div>
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
            <div className={`lib-item${onPage ? " on" : ""}`} key={exp.id}>
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

              <div className="lib-item-body">
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

                <div className="lib-bullets">
                  {bullets.map((bullet, bulletIndex) => (
                    <div className="lib-bullet" key={bulletIndex}>
                      <span className="dot" />
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

                  <div className="lib-bullet-foot">
                    <button
                      type="button"
                      onClick={() => update({ [bulletsKey]: [...bullets, ""] } as Partial<Experience>)}
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
  };

  /* ---------------- 作品 ---------------- */

  const renderProjects = () => {
    const list = person.projects;

    const addProject = () => {
      const item: ProjectItem = {
        id: uid("pj"),
        title: "作品名称",
        titleEn: "",
        year: String(new Date().getFullYear()),
        desc: "一句话说清它解决了什么问题",
        descEn: "",
        tags: [],
        cover: list.length % 4,
      };
      patch({ projects: [...list, item] });
    };

    return (
      <div className="lib-pane">
        <div className="lib-pane-head">
          <div>
            <b>共享作品</b>
            <span>作品集会按顺序取用；普通简历页只取勾选的。</span>
          </div>
          <button type="button" className="lib-add" onClick={addProject}>
            <Plus size={10} />
            新增作品
          </button>
        </div>

        {list.length === 0 && <div className="lib-empty">还没有作品。</div>}

        {list.map((project, index) => {
          const onPage = page?.projectIds.includes(project.id) ?? false;
          const update = (next: Partial<ProjectItem>) =>
            patch({
              projects: list.map((item) => (item.id === project.id ? { ...item, ...next } : item)),
            });

          return (
            <div className={`lib-item${onPage ? " on" : ""}`} key={project.id}>
              <ItemHead
                index={index}
                title={en ? project.titleEn || project.title : project.title}
                note={project.year}
                pick={
                  <PagePick checked={onPage} onChange={() => toggleOnPage("projectIds", project.id)} />
                }
                actions={{
                  first: index === 0,
                  last: index === list.length - 1,
                  onUp: () => patch({ projects: move(list, project.id, -1) }),
                  onDown: () => patch({ projects: move(list, project.id, 1) }),
                  onRemove: () =>
                    patch({ projects: list.filter((item) => item.id !== project.id) }),
                }}
              />

              <div className="lib-item-body">
                <Grid>
                  <Field
                    label={en ? "Title" : "名称"}
                    value={en ? project.titleEn ?? "" : project.title}
                    onChange={(value) => update(en ? { titleEn: value } : { title: value })}
                  />
                  <Field
                    label="年份"
                    value={project.year}
                    onChange={(value) => update({ year: value })}
                  />
                  <Field
                    label={en ? "Tags" : "标签"}
                    value={project.tags.join(" / ")}
                    placeholder="用 / 分隔"
                    onChange={(value) =>
                      update({
                        tags: value
                          .split("/")
                          .map((item) => item.trim())
                          .filter(Boolean),
                      })
                    }
                  />
                </Grid>

                <label className="fld wide">
                  <span className="fld-k">{en ? "Summary" : "描述"}</span>
                  <textarea
                    rows={2}
                    value={en ? project.descEn ?? "" : project.desc}
                    onChange={(event) =>
                      update(en ? { descEn: event.target.value } : { desc: event.target.value })
                    }
                  />
                </label>
              </div>
            </div>
          );
        })}
      </div>
    );
  };

  /* ---------------- 教育 ---------------- */

  const renderEducation = () => {
    const list = person.education;

    const addEducation = () => {
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
      <div className="lib-pane">
        <div className="lib-pane-head">
          <div>
            <b>教育</b>
            <span>所有简历页共用，按顺序出现在「教育」小节。</span>
          </div>
          <button type="button" className="lib-add" onClick={addEducation}>
            <Plus size={10} />
            新增
          </button>
        </div>

        {list.length === 0 && <div className="lib-empty">还没有教育经历。</div>}

        {list.map((item, index) => {
          const update = (next: Partial<EducationItem>) =>
            patch({
              education: list.map((row) => (row.id === item.id ? { ...row, ...next } : row)),
            });
          return (
            <div className="lib-item" key={item.id}>
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
              <div className="lib-item-body">
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
  };

  /* ---------------- 技能 ---------------- */

  const renderSkills = () => {
    const list = person.skills;

    const addSkill = () => {
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
      <div className="lib-pane">
        <div className="lib-pane-head">
          <div>
            <b>技能</b>
            <span>按分组罗列，纸面上是「左标签 + 右内容」两列。</span>
          </div>
          <button type="button" className="lib-add" onClick={addSkill}>
            <Plus size={10} />
            新增一组
          </button>
        </div>

        {list.length === 0 && <div className="lib-empty">还没有技能分组。</div>}

        {list.map((group, index) => {
          const update = (next: Partial<SkillGroup>) =>
            patch({
              skills: list.map((row) => (row.id === group.id ? { ...row, ...next } : row)),
            });
          return (
            <div className="lib-item" key={group.id}>
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
              <div className="lib-item-body">
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
  };

  /* ---------------- 人物 ---------------- */

  const renderPerson = () => (
    <div className="lib-pane">
      <div className="lib-pane-head">
        <div>
          <b>基本信息</b>
          <span>新建简历时会用这里的姓名、职位与城市预填页头。</span>
        </div>
      </div>
      <div className="lib-item-body plain">
        <Grid>
          <Field label="姓名" value={person.name} onChange={(value) => patch({ name: value })} />
          <Field label="Name" value={person.nameEn} onChange={(value) => patch({ nameEn: value })} />
          <Field label="职位" value={person.role} onChange={(value) => patch({ role: value })} />
          <Field label="Role" value={person.roleEn} onChange={(value) => patch({ roleEn: value })} />
          <Field label="邮箱" value={person.email} onChange={(value) => patch({ email: value })} />
          <Field label="城市" value={person.location} onChange={(value) => patch({ location: value })} />
          <Field
            label="年限"
            value={String(person.years)}
            onChange={(value) => patch({ years: Number(value.replace(/[^\d]/g, "")) || 0 })}
          />
        </Grid>
      </div>
    </div>
  );

  return (
    <div className="veil" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="drawer" role="dialog" aria-modal="true" aria-label="经历库">
        <header className="drawer-head">
          <div className="drawer-title">
            <b>经历库</b>
            <span>
              {person.name} · 名下 {person.resumes.length} 份简历共用
            </span>
          </div>
          <div className="drawer-switch" role="group" aria-label="编辑语言">
            <button type="button" aria-pressed={!en} onClick={() => setEn(false)}>
              中文
            </button>
            <button type="button" aria-pressed={en} onClick={() => setEn(true)}>
              EN
            </button>
          </div>
          <button type="button" className="drawer-close" aria-label="关闭" onClick={onClose}>
            <Close size={11} />
          </button>
        </header>

        <div className="drawer-body">
          <nav className="drawer-rail">
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

          <div className="drawer-main">
            {tab === "person" && renderPerson()}
            {tab === "experience" && renderExperiences()}
            {tab === "project" && renderProjects()}
            {tab === "education" && renderEducation()}
            {tab === "skill" && renderSkills()}
          </div>
        </div>

        <footer className="drawer-foot">
          <span className="dim">当前页</span>
          <span className="cur">{pageName}</span>
          <span className="sp" />
          <span className="dim">勾选「本页」只影响这一页，其余改动同步所有简历</span>
        </footer>
      </div>
    </div>
  );
}
