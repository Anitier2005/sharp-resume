import { Plus } from "@/components/ui/icons";
import type { ProjectItem } from "@/types";
import { Field, Grid, ItemHead, PagePick, PaneHead } from "./parts";
import { move, patchById, uid } from "./helpers";
import { usePageToggle, usePatchPerson } from "./use-library";
import shared from "@/components/ui/shared.module.css";
import styles from "./library.module.css";
import type { PaneProps } from "./pane-experience";

export function ProjectPane({ person, page, en }: PaneProps) {
  const patch = usePatchPerson(person);
  const toggleOnPage = usePageToggle();
  const list = person.projects;

  const add = () => {
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
    <div className={styles.pane}>
      <PaneHead
        title="共享作品"
        desc="作品集会按顺序取用；普通简历页只取勾选的。"
        action={
          <button type="button" className={styles.add} onClick={add}>
            <Plus size={10} />
            新增作品
          </button>
        }
      />

      {list.length === 0 && <div className={styles.empty}>还没有作品。</div>}

      {list.map((project, index) => {
        const onPage = page?.projectIds.includes(project.id) ?? false;
        const update = (next: Partial<ProjectItem>) =>
          patch({ projects: patchById(list, project.id, next) });

        return (
          <div className={styles.item} key={project.id}>
            <ItemHead
              index={index}
              title={en ? project.titleEn || project.title : project.title}
              note={project.year}
              pick={
                <PagePick
                  checked={onPage}
                  onChange={() => toggleOnPage("projectIds", project.id)}
                />
              }
              actions={{
                first: index === 0,
                last: index === list.length - 1,
                onUp: () => patch({ projects: move(list, project.id, -1) }),
                onDown: () => patch({ projects: move(list, project.id, 1) }),
                onRemove: () => patch({ projects: list.filter((item) => item.id !== project.id) }),
              }}
            />

            <div className={styles.itemBody}>
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

              <label className={`${shared.fld} ${shared.wide}`}>
                <span className={shared.fldK}>{en ? "Summary" : "描述"}</span>
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
}
