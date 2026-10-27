//! 极简 DOCX 生成：手写 OOXML + store（不压缩）zip，不加任何依赖。
//!
//! 为什么在 Rust：导出是数据处理，产物结构不该由页面层决定。
//! 搬到这里之后，前端只负责发起「导出这一页」，不再拼 XML 也不写字。
//!
//! v5 起按 block 排版，代码里没有「姓名 / 公司 / 教育」这类预设键：
//!   fields 块的第一个字段 = 大标题，其余按「标签：值」；
//!   section 块的标题就是用户写的那句话，条目第一个字段加粗；
//!   media 块在正文里留一行「见图」，图片本体旁挂到附件目录（不硬塞进 docx，
//!   否则一张 8MB 的证书会把简历文档撑得没法发）。

use std::collections::HashMap;

use crate::model::{Asset, Block, Field, ResumePage};

const ACCENT_HEX_WHITE: &str = "242422";

fn accent_hex(accent: &str) -> &'static str {
    match accent {
        "indigo" => "3B50B4",
        "pine" => "2A6650",
        "clay" => "B2452C",
        _ => ACCENT_HEX_WHITE,
    }
}

/// OOXML 只允许这些字符，控制符会让 Word 报文件损坏
fn clean(value: &str) -> String {
    value
        .chars()
        .filter(|ch| {
            let code = *ch as u32;
            code == 9 || code == 10 || code == 13 || code >= 32
        })
        .collect()
}

fn esc(value: &str) -> String {
    clean(value)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn run(text: &str, bold: bool, size: u32, color: &str) -> String {
    let mut props = String::new();
    if bold {
        props.push_str("<w:b/>");
    }
    props.push_str(&format!("<w:color w:val=\"{color}\"/>"));
    props.push_str(&format!("<w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/>"));
    format!(
        "<w:r><w:rPr>{props}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
        esc(text)
    )
}

fn para(text: &str, bold: bool, size: u32, color: &str) -> String {
    format!(
        "<w:p><w:pPr><w:spacing w:after=\"40\" w:line=\"300\" w:lineRule=\"auto\"/></w:pPr>{}</w:p>",
        run(text, bold, size, color)
    )
}

fn field_text(field: &Field) -> String {
    let value = field.value.trim();
    if field.label.trim().is_empty() {
        return value.to_string();
    }
    if value.is_empty() {
        format!("{}：", field.label)
    } else {
        format!("{}：{}", field.label, value)
    }
}

/// 本页字段优先；为空时回落到库里那一条（与纸面、前端同一套规则）
fn entry_fields<'a>(entry: &'a crate::model::SectionEntry, library: &'a [crate::model::LibraryItem]) -> &'a [Field] {
    if !entry.fields.is_empty() {
        return &entry.fields;
    }
    entry
        .ref_id
        .as_deref()
        .and_then(|id| library.iter().find(|item| item.id == id))
        .map(|item| item.fields.as_slice())
        .unwrap_or(&[])
}

fn block_xml(
    block: &Block,
    library: &[crate::model::LibraryItem],
    assets: &HashMap<String, Asset>,
    hex: &str,
    out: &mut Vec<String>,
) {
    match block.kind.as_str() {
        "fields" => {
            let mut iter = block.fields.iter();
            if let Some(lead) = iter.next() {
                let text = if lead.value.trim().is_empty() { &lead.label } else { &lead.value };
                out.push(para(text, true, 34, hex));
            }
            for field in iter {
                out.push(para(&field_text(field), false, 17, "77776F"));
            }
        }
        "section" => {
            if !block.title.trim().is_empty() {
                out.push(para(&block.title, true, 21, hex));
            }
            for entry in &block.items {
                for (index, field) in entry_fields(entry, library).iter().enumerate() {
                    if index == 0 {
                        let text = if field.value.trim().is_empty() { &field.label } else { &field.value };
                        out.push(para(text, true, 21, "141412"));
                    } else {
                        out.push(para(&field_text(field), false, 18, "55554F"));
                    }
                }
                for bullet in &entry.bullets {
                    if !bullet.trim().is_empty() {
                        out.push(para(&format!("· {bullet}"), false, 18, "55554F"));
                    }
                }
            }
        }
        "media" => {
            let name = block
                .asset_id
                .as_deref()
                .and_then(|id| assets.get(id))
                .map(|asset| asset.name.as_str())
                .unwrap_or("附件");
            let caption = block.caption.as_deref().unwrap_or("");
            let line = if caption.trim().is_empty() {
                format!("［图：{name}］")
            } else {
                format!("［图：{caption}（{name}）］")
            };
            out.push(para(&line, false, 17, "77776F"));
        }
        _ => {}
    }
}

fn document_xml(
    page: &ResumePage,
    library: &[crate::model::LibraryItem],
    assets: &HashMap<String, Asset>,
    hex: &str,
) -> String {
    let mut body: Vec<String> = Vec::new();
    for block in &page.blocks {
        block_xml(block, library, assets, hex, &mut body);
    }
    if body.is_empty() {
        body.push(para("（空白页）", false, 18, "999990"));
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\"/></w:sectPr></w:body></w:document>",
        body.join("")
    )
}

const STYLES_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Segoe UI\" w:hAnsi=\"Segoe UI\" w:eastAsia=\"微软雅黑\" w:cs=\"Segoe UI\"/><w:color w:val=\"1F1F1C\"/><w:sz w:val=\"21\"/><w:szCs w:val=\"21\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"0\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style></w:styles>";

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>";

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";

const DOC_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>";

fn core_xml(title: &str, stamp: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>Sharp Resume</dc:creator><cp:lastModifiedBy>Sharp Resume</cp:lastModifiedBy><dcterms:created xsi:type=\"dcterms:W3CDTF\">{stamp}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{stamp}</dcterms:modified></cp:coreProperties>",
        esc(title)
    )
}

/* ---------- zip（store，不压缩） ---------- */

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    crc ^ 0xffff_ffff
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// DOS 时间戳。固定成 1980-01-01：zip 是内容容器，时间戳只会让同样的内容
/// 每次导出都不一样的字节，反而妨碍比对。
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = 0x0021;

fn zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();

    for (name, text) in files {
        let name_bytes = name.as_bytes();
        let data = text.as_bytes();
        let crc = crc32(data);
        let offset = body.len() as u32;

        push_u32(&mut body, 0x0403_4b50);
        push_u16(&mut body, 20);
        push_u16(&mut body, 0x0800);
        push_u16(&mut body, 0);
        push_u16(&mut body, DOS_TIME);
        push_u16(&mut body, DOS_DATE);
        push_u32(&mut body, crc);
        push_u32(&mut body, data.len() as u32);
        push_u32(&mut body, data.len() as u32);
        push_u16(&mut body, name_bytes.len() as u16);
        push_u16(&mut body, 0);
        body.extend_from_slice(name_bytes);
        body.extend_from_slice(data);

        push_u32(&mut central, 0x0201_4b50);
        push_u16(&mut central, 20);
        push_u16(&mut central, 20);
        push_u16(&mut central, 0x0800);
        push_u16(&mut central, 0);
        push_u16(&mut central, DOS_TIME);
        push_u16(&mut central, DOS_DATE);
        push_u32(&mut central, crc);
        push_u32(&mut central, data.len() as u32);
        push_u32(&mut central, data.len() as u32);
        push_u16(&mut central, name_bytes.len() as u16);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, 0);
        push_u32(&mut central, offset);
        central.extend_from_slice(name_bytes);
    }

    let central_size = central.len() as u32;
    let central_offset = body.len() as u32;
    let mut end: Vec<u8> = Vec::new();
    push_u32(&mut end, 0x0605_4b50);
    push_u16(&mut end, 0);
    push_u16(&mut end, 0);
    push_u16(&mut end, files.len() as u16);
    push_u16(&mut end, files.len() as u16);
    push_u32(&mut end, central_size);
    push_u32(&mut end, central_offset);
    push_u16(&mut end, 0);

    let mut out = body;
    out.extend_from_slice(&central);
    out.extend_from_slice(&end);
    out
}

/* ---------- 出口 ---------- */

/// 生成一页 docx 的字节。
pub fn page_docx(
    person_name: &str,
    page: &ResumePage,
    library: &[crate::model::LibraryItem],
    assets: &HashMap<String, Asset>,
    accent: &str,
) -> Vec<u8> {
    let hex = accent_hex(accent);
    // 固定时间戳：内容相同则字节相同，方便校验与去重
    let stamp = "2026-01-01T00:00:00Z";
    let title = if page.label.trim().is_empty() {
        person_name.to_string()
    } else {
        format!("{person_name} · {}", page.label)
    };
    let document = document_xml(page, library, assets, hex);
    let core = core_xml(&title, stamp);
    zip(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("docProps/core.xml", &core),
        ("word/_rels/document.xml.rels", DOC_RELS),
        ("word/document.xml", &document),
        ("word/styles.xml", STYLES_XML),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn page_with(blocks: serde_json::Value) -> ResumePage {
        serde_json::from_value(json!({
            "id": "pg1",
            "kind": "native",
            "label": "中文简历",
            "blocks": blocks,
        }))
        .unwrap()
    }

    #[test]
    fn docx_starts_with_zip_signature() {
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [{ "id": "f1", "label": "姓名", "value": "张三" }] }
        ]));
        let bytes = page_docx("张三", &page, &[], &HashMap::new(), "white");
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        // 中央目录结尾在尾部
        assert_eq!(&bytes[bytes.len() - 22..bytes.len() - 18], b"PK\x05\x06");
    }

    #[test]
    fn crc32_matches_known_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn control_chars_are_dropped_and_xml_escaped() {
        assert_eq!(clean("a\u{0}b\u{7}c"), "abc");
        assert_eq!(esc("a & b < c"), "a &amp; b &lt; c");
    }

    #[test]
    fn media_block_does_not_panic_without_asset() {
        let page = page_with(json!([
            { "id": "b1", "kind": "media", "assetId": "missing", "caption": "报关员证书" }
        ]));
        let bytes = page_docx("张三", &page, &[], &HashMap::new(), "pine");
        assert!(!bytes.is_empty());
    }
}
