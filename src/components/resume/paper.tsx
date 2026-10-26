import { useEffect, useState } from "react";
import type { Field, Person, ResumePage, SectionEntry } from "@/types";
import { fieldText } from "@/types";
import { photoOf, resolveUrl } from "@/lib/assets";
import styles from "./paper.module.css";

/**
 * 纸面：只读成品，与「打印 / 存为 PDF」共用同一份 DOM。
 *
 * 渲染规则（不是 schema，只是排版约定）：
 *   页头第 1 个字段 = 大标题，其余按「标签：值」排
 *   条目第 1 个字段 = 加粗标题行，其余按「标签：值」排在下面
 * 所以用户调字段顺序就等于调主次，代码不认识「姓名」「公司」这些词。
 */
interface PaperProps {
  person: Person;
  page: ResumePage;
}

function entryFields(person: Person, entry: SectionEntry): Field[] {
  if (entry.fields.length) return entry.fields;
  const source = person.items.find((item) => item.id === entry.refId);
  return source?.fields ?? [];
}

/** 证件照：人物的第一张图片材料。没有就不占位置 */
function Photo({ person }: { person: Person }) {
  const asset = photoOf(person.assets);
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    // 没有照片就直接渲染空，不需要把 state 复位（复位也算 setState-in-effect）
    if (!asset) return;
    let alive = true;
    void resolveUrl(asset).then((value) => alive && setUrl(value));
    return () => {
      alive = false;
    };
  }, [asset]);

  if (!asset) return null;
  return (
    <div className={styles.photo}>
      {url ? <img src={url} alt={asset.name} /> : <span>照片</span>}
    </div>
  );
}

export function Paper({ person, page }: PaperProps) {
  const [lead, ...rest] = page.header;
  const hasPhoto = Boolean(photoOf(person.assets));

  return (
    <article className={styles.paper} data-print="sheet">
      <div className={hasPhoto ? styles.headRow : undefined}>
        <div className={styles.headText}>
          {lead && <h1 className={styles.name}>{lead.value || lead.label}</h1>}
          {rest.map((field) => (
            <div className={styles.meta} key={field.id}>
              {fieldText(field)}
            </div>
          ))}
        </div>
        <Photo person={person} />
      </div>
      {(lead || rest.length > 0) && <div className={styles.rule} />}

      {page.sections.map((section) => (
        <div className={styles.sec} key={section.id}>
          {section.title.trim() && <div className={styles.secTitle}>{section.title}</div>}
          {section.items.map((entry) => {
            const fields = entryFields(person, entry);
            const [head, ...others] = fields;
            return (
              <div className={styles.item} key={entry.id}>
                {head && <div className={styles.line}>
                  <b>{head.value || head.label}</b>
                </div>}
                {others.map((field) => (
                  <p key={field.id}>{fieldText(field)}</p>
                ))}
                <div className={styles.bullets}>
                  {entry.bullets
                    .filter((line) => line.trim())
                    .map((line, index) => (
                      <span className={styles.bullet} key={index}>
                        {line}
                      </span>
                    ))}
                </div>
              </div>
            );
          })}
        </div>
      ))}
    </article>
  );
}
