/**
 * 极简 DOCX 生成：不引第三方库，直接产出 OOXML + 一个 store（不压缩）的 zip。
 *
 * v4 之后这里不再认识「姓名 / 公司 / 教育 / 技能」，只按字段顺序排版：
 *   页头第一个字段 = 大标题，其余按「标签：值」；
 *   分区标题 = 用户写的那句话；条目第一个字段加粗，其余照排；措辞逐条列。
 */
import type { AccentKey, Person, ResumePage } from "@/types";
import { headerOf, sectionsOf } from "@/lib/blocks";
import { displayName, fieldText } from "@/types";

const ACCENT_HEX: Record<AccentKey, string> = {
  white: "242422",
  indigo: "3B50B4",
  pine: "2A6650",
  clay: "B2452C",
};

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
}

function run(text: string, opts: RunOpts = {}): string {
  const props: string[] = [];
  if (opts.bold) props.push("<w:b/>");
  if (opts.color) props.push(`<w:color w:val="${opts.color}"/>`);
  if (opts.size) props.push(`<w:sz w:val="${opts.size}"/><w:szCs w:val="${opts.size}"/>`);
  const rPr = props.length ? `<w:rPr>${props.join("")}</w:rPr>` : "";
  return `<w:r>${rPr}<w:t xml:space="preserve">${esc(text)}</w:t></w:r>`;
}

/** 一段：行距 1.25、段后 2pt，足够像简历 */
function para(text: string, opts: RunOpts = {}): string {
  return `<w:p><w:pPr><w:spacing w:after="40" w:line="300" w:lineRule="auto"/></w:pPr>${run(
    text,
    { size: 19, color: "333330", ...opts },
  )}</w:p>`;
}

function documentXml(person: Person, page: ResumePage, hex: string): string {
  const out: string[] = [];
  const [lead, ...rest] = headerOf(page);

  if (lead) out.push(para(lead.value || lead.label, { bold: true, size: 34, color: hex }));
  for (const field of rest) {
    out.push(para(fieldText(field), { size: 17, color: "77776F" }));
  }

  for (const section of sectionsOf(page)) {
    if (section.title.trim()) {
      out.push(para(section.title, { bold: true, size: 21, color: hex }));
    }
    for (const entry of section.items) {
      // 本页字段优先；没有就回落到库里那一条
      const source = person.items.find((item) => item.id === entry.refId);
      const fields = entry.fields.length ? entry.fields : source?.fields ?? [];
      fields.forEach((field, index) => {
        out.push(
          para(fieldText(field), {
            bold: index === 0,
            size: index === 0 ? 21 : 18,
            color: index === 0 ? "141412" : "55554F",
          }),
        );
      });
      for (const bullet of entry.bullets) {
        if (bullet.trim()) out.push(para(`· ${bullet}`, { size: 18, color: "55554F" }));
      }
    }
  }

  if (!out.length) out.push(para("（空白页）", { size: 18, color: "999990" }));

  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>${out.join("")}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1134" w:right="1134" w:bottom="1134" w:left="1134"/></w:sectPr></w:body></w:document>`;
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

  const total = locals.reduce((sum, item) => sum + item.length, 0) + centralSize + end.length;
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
    { name: "docProps/core.xml", text: coreXml(`${displayName(person.fields)} · ${page.label}`) },
    { name: "word/_rels/document.xml.rels", text: DOC_RELS },
    { name: "word/document.xml", text: documentXml(person, page, hex) },
    { name: "word/styles.xml", text: STYLES_XML },
  ]);
}
