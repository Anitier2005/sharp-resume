import type { Person, ResumePage } from "@/types";
import { resolveEntries, wordingOf } from "@/lib/entries";
import styles from "./paper.module.css";

/**
 * 纸面 = 最终成品样式，只读。
 *
 * 这里刻意不接受任何编辑回调：预览态与「打印 / 存为 PDF」用的是同一个组件、
 * 同一份 DOM，所以屏幕上看到的就是导出的样子。任何就地编辑能力都会破坏这个
 * 恒等关系（contentEditable 会带出占位符、聚焦底色等非成品样式）。
 * 要改内容请走右侧表单。
 */
interface PaperProps {
  person: Person;
  page: ResumePage;
}

const LABELS = {
  zh: { experience: "工作经历", project: "项目", education: "教育", skill: "技能" },
  en: { experience: "Experience", project: "Selected Projects", education: "Education", skill: "Skills" },
} as const;

export function Paper({ person, page }: PaperProps) {
  const en = page.kind === "en";
  const label = LABELS[en ? "en" : "zh"];

  const rows = resolveEntries(person, page);

  const projects = page.projectIds
    .map((id) => person.projects.find((item) => item.id === id))
    .filter((item): item is NonNullable<typeof item> => Boolean(item));

  return (
    <article className={styles.paper} data-lang={en ? "en" : "zh"} data-print="sheet">
      <h1 className={styles.name}>{page.headline}</h1>
      {page.role && <div className={styles.role}>{page.role}</div>}
      {page.meta.length > 0 && <div className={styles.meta}>{page.meta.join(" · ")}</div>}
      <div className={styles.rule} />

      {page.summary ? <p className={styles.summary}>{page.summary}</p> : null}

      {rows.length > 0 && (
        <div className={styles.sec}>
          <div className={styles.secTitle}>{label.experience}</div>
          {rows.map(({ entry, exp }) => {
            const bullets = wordingOf(entry, en);
            return (
              <div className={styles.item} key={entry.id}>
                <div className={styles.line}>
                  <b>
                    {en ? exp.companyEn || exp.company : exp.company} ·{" "}
                    {en ? exp.roleEn || exp.role : exp.role}
                  </b>
                  <span>{exp.period}</span>
                </div>
                <div className={styles.bullets}>
                  {bullets.map((bullet, index) => (
                    <span className={styles.bullet} key={index}>
                      {bullet}
                    </span>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {projects.length > 0 && (
        <div className={styles.sec}>
          <div className={styles.secTitle}>{label.project}</div>
          {projects.map((project) => (
            <div className={styles.item} key={project.id}>
              <div className={styles.line}>
                <b>{en ? project.titleEn || project.title : project.title}</b>
                <span>{project.year}</span>
              </div>
              <p>{en ? project.descEn || project.desc : project.desc}</p>
            </div>
          ))}
        </div>
      )}

      {person.education.length > 0 && (
        <div className={styles.sec}>
          <div className={styles.secTitle}>{label.education}</div>
          {person.education.map((item) => (
            <div className={styles.item} key={item.id}>
              <div className={styles.line}>
                <b>
                  {en ? item.schoolEn || item.school : item.school} ·{" "}
                  {en ? item.degreeEn || item.degree : item.degree}
                </b>
                <span>{item.period}</span>
              </div>
            </div>
          ))}
        </div>
      )}

      {person.skills.length > 0 && (
        <div className={styles.sec}>
          <div className={styles.secTitle}>{label.skill}</div>
          {person.skills.map((group) => (
            <div className={styles.skill} key={group.id}>
              <span>{en ? group.labelEn || group.label : group.label}</span>
              <p>{en ? group.valueEn || group.value : group.value}</p>
            </div>
          ))}
        </div>
      )}
    </article>
  );
}
