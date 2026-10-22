/**
 * 极简 DOCX 生成：不引第三方库，直接产出 OOXML + 一个 store（不压缩）的 zip。
 * Word / WPS / Pages 都能打开；版式对齐纸面：姓名、职位、分节、右对齐时间。
 */
import type { AccentKey, Person, ResumePage } from "@/types";

const ACCENT_HEX: Record<AccentKey, string> = {
  white: "242422",
  indigo: "3B50B4",
  pine: "2A6650",
  clay: "B2452C",
};

const LABELS = {
  zh: {
    experience: "工作经历",
    project: "项目",
    education: "教育",
    skill: "技能",
    portfolio: "作品集",
  },
  en: {
    experience: "Experience",
    project: "Selected Projects",
    education: "Education",
    skill: "Skills",
    portfolio: "Portfolio",
  },
};

const INK = "1F1F1C";
const MUTED = "6B6B64";
const FAINT = "9A9A92";

/** OOXML 只允许这些字符，控制符会让 Word 报文件损坏 */
const clean = (value: string) => {
  let out = "";
  for (const char of value) {
    const code = char.codePointAt(0) ?? 0;
    if (code === 9 || code === 10 || code === 13 || code >= 32) out += char;
  }
  return out;
};

const esc = (value: string) =>
  clean(value)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");

interface RunOpts {
  bold?: boolean;
  /** 半磅：21 = 10.5pt */
  size?: number;
  color?: string;
  /** 字距，单位 1/20 磅 */
  spacing?: number;
  caps?: boolean;
  italic?: boolean;
}

type Piece = { text: string; tab?: boolean; opts?: RunOpts };

interface ParaOpts {
  align?: "left" | "both";
  before?: number;
  after?: number;
  /** 行距，240 = 单倍 */
  line?: number;
  indent?: number;
  hanging?: number;
  tabRight?: number;
  border?: string;
}

function run({ text, tab, opts = {} }: Piece): string {
  const props: string[] = [];
  if (opts.bold) props.push("<w:b/>");
  if (opts.caps) props.push("<w:caps/>");
  if (opts.italic) props.push("<w:i/>");
  if (opts.color) props.push(`<w:color w:val="${opts.color}"/>`);
  if (opts.spacing) props.push(`<w:spacing w:val="${opts.spacing}"/>`);
  if (opts.size) props.push(`<w:sz w:val="${opts.size}"/><w:szCs w:val="${opts.size}"/>`);
  const rPr = props.length ? `<w:rPr>${props.join("")}</w:rPr>` : "";
  if (tab) return `<w:r>${rPr}<w:tab/></w:r>`;
  return `<w:r>${rPr}<w:t xml:space="preserve">${esc(text)}</w:t></w:r>`;
}

function para(pieces: Piece[] | string, opts: ParaOpts = {}): string {
  const list: Piece[] = typeof pieces === "string" ? [{ text: pieces }] : pieces;
  const props: string[] = [];
  if (opts.border) {
    props.push(
      `<w:pBdr><w:bottom w:val="single" w:sz="4" w:space="4" w:color="${opts.border}"/></w:pBdr>`,
    );
  }
  if (opts.tabRight) {
    props.push(`<w:tabs><w:tab w:val="right" w:pos="${opts.tabRight}"/></w:tabs>`);
  }
  const spacing: string[] = [];
  if (opts.before) spacing.push(`w:before="${opts.before}"`);
  if (opts.after !== undefined) spacing.push(`w:after="${opts.after}"`);
  if (opts.line) spacing.push(`w:line="${opts.line}" w:lineRule="auto"`);
  if (spacing.length) props.push(`<w:spacing ${spacing.join(" ")}/>`);
  if (opts.indent || opts.hanging) {
    props.push(
      `<w:ind w:left="${opts.indent ?? 0}"${opts.hanging ? ` w:hanging="${opts.hanging}"` : ""}/>`,
    );
  }
  if (opts.align === "both") props.push(`<w:jc w:val="both"/>`);
  // 空段落也要保留高度
  const body = list.map(run).join("") || "<w:r><w:t/></w:r>";
  return `<w:p>${props.length ? `<w:pPr>${props.join("")}</w:pPr>` : ""}${body}</w:p>`;
}

/* ---------------------------------------------------------------- 页面内容 */

/** A4 正文宽度：11906 - 2×1134 = 9638 twips，右对齐制表位贴在这个位置 */
const CONTENT_WIDTH = 9638;

function pageBody(person: Person, page: ResumePage, accent: string): string {
  const en = page.kind === "en";
  const label = LABELS[en ? "en" : "zh"];
  const out: string[] = [];

  out.push(
    `<w:p><w:pPr><w:spacing w:after="90"/></w:pPr>${run({
      text: page.headline,
      opts: { bold: true, size: 40, spacing: 60, color: INK },
    })}</w:p>`,
  );

  out.push(
    `<w:p><w:pPr><w:spacing w:after="60"/></w:pPr>${run({
      text: en ? page.role.toUpperCase() : page.role,
      opts: { size: 18, color: accent, spacing: 40 },
    })}</w:p>`,
  );

  if (page.meta.length) {
    out.push(
      `<w:p><w:pPr><w:spacing w:after="140"/></w:pPr>${run({
        text: page.meta.join("  ·  "),
        opts: { size: 18, color: FAINT },
      })}</w:p>`,
    );
  }

  out.push(para("", { border: "E4E4DE", after: 160 }));

  if (page.summary?.trim()) {
    out.push(
      `<w:p><w:pPr><w:spacing w:after="120" w:line="300" w:lineRule="auto"/></w:pPr>${run({
        text: page.summary,
        opts: { size: 19, color: MUTED },
      })}</w:p>`,
    );
  }

  const sectionTitle = (text: string) =>
    `<w:p><w:pPr><w:spacing w:before="200" w:after="90"/></w:pPr>${run({
      text,
      opts: { bold: true, size: 17, color: accent, spacing: 28 },
    })}</w:p>`;

  /** 左标题 + 右时间 */
  const lineWithPeriod = (left: string, period: string) =>
    `<w:p><w:pPr><w:tabs><w:tab w:val="right" w:pos="${CONTENT_WIDTH}"/></w:tabs><w:spacing w:after="30"/></w:pPr>${run(
      { text: left, opts: { bold: true, size: 21, color: INK } },
    )}${run({ text: "", tab: true })}${run({
      text: period,
      opts: { size: 17, color: FAINT },
    })}</w:p>`;

  const experiences = page.experienceIds
    .map((id) => person.experiences.find((item) => item.id === id))
    .filter((item): item is NonNullable<typeof item> => Boolean(item));

  if (experiences.length) {
    out.push(sectionTitle(label.experience));
    for (const exp of experiences) {
      const company = en ? exp.companyEn || exp.company : exp.company;
      const role = en ? exp.roleEn || exp.role : exp.role;
      const bullets = (en ? exp.bulletsEn?.length ? exp.bulletsEn : exp.bullets : exp.bullets) ?? [];
      out.push(lineWithPeriod(`${company} · ${role}`, exp.period));
      for (const bullet of bullets) {
        out.push(
          `<w:p><w:pPr><w:spacing w:after="16" w:line="300" w:lineRule="auto"/><w:ind w:left="227" w:hanging="227"/></w:pPr>${run(
            { text: `· ${bullet}`, opts: { size: 19, color: MUTED } },
          )}</w:p>`,
        );
      }
    }
  }

  const projects = page.projectIds
    .map((id) => person.projects.find((item) => item.id === id))
    .filter((item): item is NonNullable<typeof item> => Boolean(item));

  if (projects.length) {
    out.push(sectionTitle(page.kind === "portfolio" ? label.portfolio : label.project));
    for (const project of projects) {
      const title = en ? project.titleEn || project.title : project.title;
      const desc = en ? project.descEn || project.desc : project.desc;
      out.push(lineWithPeriod(title, project.year));
      if (desc) {
        out.push(
          `<w:p><w:pPr><w:spacing w:after="40" w:line="300" w:lineRule="auto"/></w:pPr>${run({
            text: desc,
            opts: { size: 19, color: MUTED },
          })}</w:p>`,
        );
      }
      if (project.tags.length) {
        out.push(
          `<w:p><w:pPr><w:spacing w:after="60"/></w:pPr>${run({
            text: project.tags.join(" / "),
            opts: { size: 16, color: FAINT },
          })}</w:p>`,
        );
      }
    }
  }

  if (person.education.length) {
    out.push(sectionTitle(label.education));
    for (const item of person.education) {
      const school = en ? item.schoolEn || item.school : item.school;
      const degree = en ? item.degreeEn || item.degree : item.degree;
      out.push(lineWithPeriod(`${school} · ${degree}`, item.period));
    }
  }

  if (person.skills.length) {
    out.push(sectionTitle(label.skill));
    for (const group of person.skills) {
      const key = en ? group.labelEn || group.label : group.label;
      const value = en ? group.valueEn || group.value : group.value;
      out.push(
        `<w:p><w:pPr><w:spacing w:after="24" w:line="288" w:lineRule="auto"/><w:ind w:left="620" w:hanging="620"/></w:pPr>${run(
          { text: key, opts: { size: 17, color: FAINT } },
        )}${run({ text: "\u00a0\u00a0\u00a0\u00a0", opts: { size: 17 } })}${run({
          text: value,
          opts: { size: 19, color: MUTED },
        })}</w:p>`,
      );
    }
  }

  return out.join("");
}

/* ---------------------------------------------------------------- 文档零件 */

function documentXml(person: Person, page: ResumePage, accent: string): string {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>${pageBody(
    person,
    page,
    accent,
  )}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1134" w:right="1134" w:bottom="1134" w:left="1134" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>`;
}

const STYLES_XML = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:eastAsia="微软雅黑" w:cs="Segoe UI"/><w:color w:val="1F1F1C"/><w:sz w:val="21"/><w:szCs w:val="21"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style></w:styles>`;

const CONTENT_TYPES = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>`;

const ROOT_RELS = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>`;

const DOC_RELS = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>`;

function coreXml(title: string): string {
  const now = new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>${esc(
    title,
  )}</dc:title><dc:creator>Sharp Resume</dc:creator><cp:lastModifiedBy>Sharp Resume</cp:lastModifiedBy><dcterms:created xsi:type="dcterms:W3CDTF">${now}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">${now}</dcterms:modified></cp:coreProperties>`;
}

/* ---------------------------------------------------------------- zip */

let CRC_TABLE: Uint32Array | null = null;

function crcTable(): Uint32Array {
  if (CRC_TABLE) return CRC_TABLE;
  const table = new Uint32Array(256);
  for (let i = 0; i < 256; i += 1) {
    let value = i;
    for (let bit = 0; bit < 8; bit += 1) {
      value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
    }
    table[i] = value >>> 0;
  }
  CRC_TABLE = table;
  return table;
}

function crc32(bytes: Uint8Array): number {
  const table = crcTable();
  let crc = 0xffffffff;
  for (let i = 0; i < bytes.length; i += 1) {
    crc = table[(crc ^ bytes[i]) & 0xff] ^ (crc >>> 8);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function dosStamp(date: Date): { time: number; date: number } {
  return {
    time:
      ((date.getHours() & 0x1f) << 11) |
      ((date.getMinutes() & 0x3f) << 5) |
      ((date.getSeconds() >> 1) & 0x1f),
    date:
      (((date.getFullYear() - 1980) & 0x7f) << 9) |
      (((date.getMonth() + 1) & 0x0f) << 5) |
      (date.getDate() & 0x1f),
  };
}

/** store（无压缩）zip：体积对简历来说无所谓，换来的是零依赖 */
function zip(files: { name: string; text: string }[]): Uint8Array {
  const encoder = new TextEncoder();
  const stamp = dosStamp(new Date());
  const locals: Uint8Array[] = [];
  const centrals: Uint8Array[] = [];
  let offset = 0;

  for (const file of files) {
    const name = encoder.encode(file.name);
    const data = encoder.encode(file.text);
    const crc = crc32(data);

    const local = new Uint8Array(30 + name.length + data.length);
    const lv = new DataView(local.buffer);
    lv.setUint32(0, 0x04034b50, true);
    lv.setUint16(4, 20, true);
    lv.setUint16(6, 0x0800, true);
    lv.setUint16(8, 0, true);
    lv.setUint16(10, stamp.time, true);
    lv.setUint16(12, stamp.date, true);
    lv.setUint32(14, crc, true);
    lv.setUint32(18, data.length, true);
    lv.setUint32(22, data.length, true);
    lv.setUint16(26, name.length, true);
    lv.setUint16(28, 0, true);
    local.set(name, 30);
    local.set(data, 30 + name.length);
    locals.push(local);

    const central = new Uint8Array(46 + name.length);
    const cv = new DataView(central.buffer);
    cv.setUint32(0, 0x02014b50, true);
    cv.setUint16(4, 20, true);
    cv.setUint16(6, 20, true);
    cv.setUint16(8, 0x0800, true);
    cv.setUint16(10, 0, true);
    cv.setUint16(12, stamp.time, true);
    cv.setUint16(14, stamp.date, true);
    cv.setUint32(16, crc, true);
    cv.setUint32(20, data.length, true);
    cv.setUint32(24, data.length, true);
    cv.setUint16(28, name.length, true);
    cv.setUint16(30, 0, true);
    cv.setUint16(32, 0, true);
    cv.setUint16(34, 0, true);
    cv.setUint16(36, 0, true);
    cv.setUint32(38, 0, true);
    cv.setUint32(42, offset, true);
    central.set(name, 46);
    centrals.push(central);

    offset += local.length;
  }

  const centralSize = centrals.reduce((sum, item) => sum + item.length, 0);
  const end = new Uint8Array(22);
  const ev = new DataView(end.buffer);
  ev.setUint32(0, 0x06054b50, true);
  ev.setUint16(4, 0, true);
  ev.setUint16(6, 0, true);
  ev.setUint16(8, files.length, true);
  ev.setUint16(10, files.length, true);
  ev.setUint32(12, centralSize, true);
  ev.setUint32(16, offset, true);
  ev.setUint16(20, 0, true);

  const total =
    locals.reduce((sum, item) => sum + item.length, 0) + centralSize + end.length;
  const out = new Uint8Array(total);
  let at = 0;
  for (const part of [...locals, ...centrals, end]) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

/* ---------------------------------------------------------------- 出口 */

export function pageToDocx(person: Person, page: ResumePage, accent: AccentKey): Uint8Array {
  const hex = ACCENT_HEX[accent] ?? ACCENT_HEX.white;
  return zip([
    { name: "[Content_Types].xml", text: CONTENT_TYPES },
    { name: "_rels/.rels", text: ROOT_RELS },
    { name: "docProps/core.xml", text: coreXml(`${person.name} · ${page.label}`) },
    { name: "word/_rels/document.xml.rels", text: DOC_RELS },
    { name: "word/document.xml", text: documentXml(person, page, hex) },
    { name: "word/styles.xml", text: STYLES_XML },
  ]);
}
