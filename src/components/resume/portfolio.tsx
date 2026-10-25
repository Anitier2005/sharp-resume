import type { Person, ResumePage } from "@/types";
import paper from "./paper.module.css";
import styles from "./portfolio.module.css";

/** 作品集纸面：与 paper.tsx 一样是只读成品，预览与导出共用 */
interface PortfolioProps {
  person: Person;
  page: ResumePage;
}

export function Portfolio({ person, page }: PortfolioProps) {
  const en = page.kind === "en";
  const projects = page.projectIds
    .map((id) => person.projects.find((item) => item.id === id))
    .filter((item): item is NonNullable<typeof item> => Boolean(item));

  return (
    <article className={paper.paper} data-kind="portfolio" data-print="sheet">
      <h1 className={paper.name}>{page.headline}</h1>
      {page.role && <div className={paper.role}>{page.role}</div>}
      {page.meta.length > 0 && <div className={paper.meta}>{page.meta.join(" · ")}</div>}
      <div className={paper.rule} />

      {page.summary ? <p className={paper.summary}>{page.summary}</p> : null}

      {projects.length === 0 && <div className={styles.empty}>这个人还没有共享作品，先去经历库补一件。</div>}

      <div className={styles.list}>
        {projects.map((project, index) => (
          <div className={styles.item} key={project.id}>
            <div className={styles.body}>
              <div className={styles.head}>
                <span className={styles.index}>{String(index + 1).padStart(2, "0")}</span>
                <b>{en ? project.titleEn || project.title : project.title}</b>
                <span className={styles.year}>{project.year}</span>
              </div>
              <p className={styles.desc}>{en ? project.descEn || project.desc : project.desc}</p>
              {project.tags.length > 0 && (
                <div className={styles.tags}>
                  {project.tags.map((tag) => (
                    <span key={tag}>{tag}</span>
                  ))}
                </div>
              )}
            </div>
            <div className={styles.thumb} data-cover={project.cover % 4} aria-hidden="true">
              <span className={styles.mark}>{String(index + 1).padStart(2, "0")}</span>
            </div>
          </div>
        ))}
      </div>
    </article>
  );
}
