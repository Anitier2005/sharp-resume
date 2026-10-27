import type { CSSProperties } from "react";
import { useEffect, useState } from "react";
import type { Asset, Block, Field, Person, ResumePage, SectionEntry } from "@/types";
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
 *
 * v5 起一页是一串块：第一个 `fields` 块当页头（带证件照与大标题），
 * 其余 `fields` 块按普通字段组排，`section` 是分区，`media` 是图片。
 * 认不出的块直接跳过 —— 老前端遇到新块不该把整页渲染成空白。
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

/** 材料可能挂在人身上，也可能挂在他自己的某条库里 */
function findAsset(person: Person, assetId?: string): Asset | undefined {
  if (!assetId) return undefined;
  const own = person.assets?.find((asset) => asset.id === assetId);
  if (own) return own;
  for (const item of person.items) {
    const hit = item.assets?.find((asset) => asset.id === assetId);
    if (hit) return hit;
  }
  return undefined;
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

/** 图片块：作品截图、证书扫描件、整页原件 */
function Media({ person, block }: { person: Person; block: Block }) {
  const asset = findAsset(person, block.assetId);
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!asset) return;
    let alive = true;
    void resolveUrl(asset).then((value) => alive && setUrl(value));
    return () => {
      alive = false;
    };
  }, [asset]);

  if (!asset) return null;
  return (
    <figure className={styles.media}>
      {url ? (
        <img
          src={url}
          alt={asset.name}
          data-fit={block.fit === "cover" ? "cover" : "contain"}
        />
      ) : (
        <span className={styles.mediaEmpty}>{asset.name}</span>
      )}
      {block.caption?.trim() && <figcaption>{block.caption}</figcaption>}
    </figure>
  );
}

/** 页头：第一个 fields 块 */
function Head({ person, fields }: { person: Person; fields: Field[] }) {
  const [lead, ...rest] = fields;
  const hasPhoto = Boolean(photoOf(person.assets));
  return (
    <>
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
    </>
  );
}

/** 分区：一个 section 块 */
function SectionView({ person, block }: { person: Person; block: Block }) {
  const items = block.items ?? [];
  return (
    <div className={styles.sec}>
      {block.title?.trim() && <div className={styles.secTitle}>{block.title}</div>}
      {items.map((entry) => {
        const fields = entryFields(person, entry);
        const [head, ...others] = fields;
        return (
          <div className={styles.item} key={entry.id}>
            {head && (
              <div className={styles.line}>
                <b>{head.value || head.label}</b>
              </div>
            )}
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
  );
}

/** 整页原件：用户直接上传的一页（扫描件、设计好的 PDF 转图），我们不排版，原样铺满 */
function FullPage({ person, assetId }: { person: Person; assetId?: string }) {
  const asset = findAsset(person, assetId);
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!asset) return;
    let alive = true;
    void resolveUrl(asset).then((value) => alive && setUrl(value));
    return () => {
      alive = false;
    };
  }, [asset]);

  if (!url) {
    return <div className={styles.fullPageEmpty}>{asset?.name ?? "原件找不到"}</div>;
  }
  return <img className={styles.fullPage} src={url} alt={asset?.name ?? "原件"} />;
}

export function Paper({ person, page }: PaperProps) {
  const blocks = page.blocks ?? [];
  const firstFields = blocks.findIndex((block) => block.kind === "fields");
  const isFile = page.kind === "file" && Boolean(page.assetId);

  return (
    <article
      className={styles.paper}
      data-print="sheet"
      data-selectable
      data-page-kind={page.kind === "file" ? "file" : "native"}
      data-density={page.style?.density ?? "normal"}
      data-align={page.style?.align ?? "left"}
      style={{ "--p-margin": String(page.style?.margin ?? 1) } as CSSProperties}
    >
      {isFile && <FullPage person={person} assetId={page.assetId} />}
      {blocks.map((block, index) => {
        if (block.kind === "fields") {
          // 第一个字段块是页头，后面的按普通字段组排（用户自己加的「联系方式」这类）
          return index === firstFields ? (
            <Head key={block.id} person={person} fields={block.fields ?? []} />
          ) : (
            <div className={styles.fieldGroup} key={block.id}>
              {(block.fields ?? []).map((field) => (
                <div className={styles.meta} key={field.id}>
                  {fieldText(field)}
                </div>
              ))}
            </div>
          );
        }
        if (block.kind === "section") {
          return <SectionView key={block.id} person={person} block={block} />;
        }
        if (block.kind === "media") {
          return <Media key={block.id} person={person} block={block} />;
        }
        return null;
      })}
    </article>
  );
}
