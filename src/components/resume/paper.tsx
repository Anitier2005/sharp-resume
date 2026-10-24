import type { Person, ResumePage } from "@/types";
import { Editable } from "@/components/ui/editable";
import styles from "./paper.module.css";

interface PaperProps {
  person: Person;
  page: ResumePage;
  onPatchPage: (patch: Partial<ResumePage>) => void;
  onPatchBullet: (expId: string, index: number, text: string) => void;
}

const LABELS = {
  zh: { experience: "工作经历", project: "项目", education: "教育", skill: "技能" },
  en: { experience: "Experience", project: "Selected Projects", education: "Education", skill: "Skills" },
} as const;

export function Paper({ person, page, onPatchPage, onPatchBullet }: PaperProps) {
  const en = page.kind === "en";
  const label = LABELS[en ? "en" : "zh"];
  const experiences = page.experienceIds
    .map((id) => person.experiences.find((item) => item.id === id))
    .filter((item): item is NonNullable<typeof item> => Boolean(item));
  const projects = page.projectIds
    .map((id) => person.projects.find((item) => item.id === id))
    .filter((item): item is NonNullable<typeof item> => Boolean(item));

  return (
    <article className={styles.paper} data-lang={en ? "en" : "zh"} data-print="sheet">
      <Editable
        className={styles.name}
        value={page.headline}
        placeholder="姓名"
        multiline={false}
        onChange={(value) => onPatchPage({ headline: value })}
      />
      <Editable
        className={styles.role}
        value={page.role}
        placeholder="目标岗位"
        multiline={false}
        onChange={(value) => onPatchPage({ role: value })}
      />
      <Editable
        className={styles.meta}
        value={page.meta.join(" · ")}
        placeholder="邮箱 · 城市 · 年限"
        multiline={false}
        onChange={(value) =>
          onPatchPage({ meta: value.split("·").map((item) => item.trim()).filter(Boolean) })
        }
      />
      <div className={styles.rule} />

      {page.summary !== undefined && (
        <Editable
          className={styles.summary}
          value={page.summary}
          placeholder="一句话摘要（可留空）"
          onChange={(value) => onPatchPage({ summary: value })}
        />
      )}

      {experiences.length > 0 && (
        <div className={styles.sec}>
          <div className={styles.secTitle}>{label.experience}</div>
          {experiences.map((exp) => (
            <div className={styles.item} key={exp.id}>
              <div className={styles.line}>
                <b>
                  {en ? exp.companyEn ?? exp.company : exp.company} ·{" "}
                  {en ? exp.roleEn ?? exp.role : exp.role}
                </b>
                <span>{exp.period}</span>
              </div>
              <div className={styles.bullets}>
                {(en ? exp.bulletsEn ?? exp.bullets : exp.bullets).map((bullet, index) => (
                  <Editable
                    key={index}
                    as="span"
                    className={styles.bullet}
                    value={bullet}
                    placeholder="描述这一段的产出"
                    onChange={(value) => onPatchBullet(exp.id, index, value)}
                  />
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {projects.length > 0 && (
        <div className={styles.sec}>
          <div className={styles.secTitle}>{label.project}</div>
          {projects.map((project) => (
            <div className={styles.item} key={project.id}>
              <div className={styles.line}>
                <b>{en ? project.titleEn ?? project.title : project.title}</b>
                <span>{project.year}</span>
              </div>
              <p>{en ? project.descEn ?? project.desc : project.desc}</p>
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
                  {en ? item.schoolEn ?? item.school : item.school} ·{" "}
                  {en ? item.degreeEn ?? item.degree : item.degree}
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
              <span>{en ? group.labelEn ?? group.label : group.label}</span>
              <p>{en ? group.valueEn ?? group.value : group.value}</p>
            </div>
          ))}
        </div>
      )}
    </article>
  );
}
