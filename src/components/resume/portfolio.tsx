import type { Person, ResumePage } from "@/types";
import { Editable } from "@/components/ui/editable";
import paper from "./paper.module.css";
import styles from "./portfolio.module.css";

interface PortfolioProps {
  person: Person;
  page: ResumePage;
  onPatchPage: (patch: Partial<ResumePage>) => void;
}

export function Portfolio({ person, page, onPatchPage }: PortfolioProps) {
  const projects = page.projectIds
    .map((id) => person.projects.find((item) => item.id === id))
    .filter((item): item is NonNullable<typeof item> => Boolean(item));

  return (
    <article className={paper.paper} data-kind="portfolio" data-print="sheet">
      <Editable
        className={paper.name}
        value={page.headline}
        placeholder="姓名"
        multiline={false}
        onChange={(value) => onPatchPage({ headline: value })}
      />
      <Editable
        className={paper.role}
        value={page.role}
        placeholder="作品集副标题"
        multiline={false}
        onChange={(value) => onPatchPage({ role: value })}
      />
      <Editable
        className={paper.meta}
        value={page.meta.join(" · ")}
        placeholder="邮箱 · 城市"
        multiline={false}
        onChange={(value) =>
          onPatchPage({ meta: value.split("·").map((item) => item.trim()).filter(Boolean) })
        }
      />
      <div className={paper.rule} />

      {page.summary !== undefined && (
        <Editable
          className={paper.summary}
          value={page.summary}
          placeholder="一段话说明这组作品的线索"
          onChange={(value) => onPatchPage({ summary: value })}
        />
      )}

      {projects.length === 0 && <div className={styles.empty}>这个人还没有共享作品，先去经历库补一件。</div>}

      <div className={styles.list}>
        {projects.map((project, index) => (
          <div className={styles.item} key={project.id}>
            <div className={styles.body}>
              <div className={styles.head}>
                <span className={styles.index}>{String(index + 1).padStart(2, "0")}</span>
                <b>{project.title}</b>
                <span className={styles.year}>{project.year}</span>
              </div>
              <p className={styles.desc}>{project.desc}</p>
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
