//! 成品层：把结构化内容编译成一份可直接渲染的 HTML。
//!
//! 为什么要这一层，而不是继续在 React 里拼 JSX：
//!   1. **预览、打印、Word 导出必须共用同一份成品**。以前预览走 CSS、Word 走
//!      硬编码 OOXML，两边各排各的，排版 agent 调的参数在 Word 里完全失效。
//!   2. **排版 agent 要能改写它**。改 HTML 片段比改组件代码现实得多。
//!   3. **清洗必须发生在受信侧**。模型产出的字符串不能直接塞进 WebView。
//!
//! 两档生成，优先级从高到低：
//!   1. `layout.html` —— 整页手写。作品集、或结构化确实做不出来的版面
//!   2. `layout.parts[blockId]` —— 只改某几个块
//!   3. **结构化生成**（默认）—— 按下面的规则从 blocks 拼，token 成本为零
//!
//! 结构化这一档刻意把样式全部收进 `paper.css` 的 class 里：生成一页只产出
//! 十来个标签、不产出任何样式声明。agent 想改哪一块，给那个元素加 `style`
//! 覆盖即可，不必重写整页。

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Asset, Block, Field, Person, ResumePage, SectionEntry};

/// 预设样式表，随编译结果一起返回。
pub const PRESET_CSS: &str = include_str!("paper.css");

const DEFAULT_PRESET: &str = "classic";

/// 版式预设。前三个只差字号与间距，`custom` 表示「完全由 agent 接管」。
const PRESETS: [&str; 4] = ["classic", "compact", "roomy", "custom"];

/// 允许出现在成品里的标签。
///
/// 这份清单就是「HTML 子集」的定义：超出范围的标签会被清洗掉、只留文字。
/// 它同时也是 Word 导出能表达的范围 —— 加了标签就要同步加导出映射，
/// 否则会出现「预览有、导出没有」。
const TAGS: &[&str] = &[
    // 结构
    "section", "div", "header", "footer", "article", "aside", "main", "nav",
    // 文本
    "h1", "h2", "h3", "h4", "h5", "h6", "p", "span", "strong", "b", "em", "i", "u", "small", "sub",
    "sup", "mark", "br", "hr", "blockquote", "q", "cite", "code", "pre",
    // 列表
    "ul", "ol", "li", "dl", "dt", "dd",
    // 媒体与链接
    "figure", "figcaption", "img", "a",
    // 表格（技能矩阵、时间线会用）
    "table", "thead", "tbody", "tfoot", "tr", "th", "td", "caption", "colgroup", "col",
    // 矢量图：图表、示意图
    "svg", "g", "path", "circle", "rect", "line", "polyline", "polygon", "ellipse", "text", "tspan",
    "defs", "use", "desc", "title",
];

/// 允许出现在任意标签上的属性。
const GENERIC_ATTRS: &[&str] = &[
    "style", "class", "src", "alt", "width", "height", "href", "colspan", "rowspan", "viewbox",
];

/// `style` 里允许的属性。
///
/// 只开「排版」这一类：字体、间距、颜色、盒模型、布局、边框、分栏。
/// 刻意不开 `position`（分页会失控）、`transform`（Word 表达不了）、
/// `animation`（没有意义）、`z-index`（同上）。
const CSS_PROPS: &[&str] = &[
    // 字体与文本
    "font-size", "font-weight", "font-family", "font-style", "line-height", "letter-spacing",
    "word-spacing", "text-align", "text-indent", "text-transform", "text-decoration",
    "text-decoration-color", "white-space", "word-break", "overflow-wrap", "vertical-align",
    // 颜色
    "color", "background-color", "opacity",
    // 盒模型
    "margin", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding",
    "padding-top", "padding-right", "padding-bottom", "padding-left", "width", "height",
    "min-width", "min-height", "max-width", "max-height",
    // 布局
    "display", "flex", "flex-direction", "flex-wrap", "flex-grow", "flex-shrink", "flex-basis",
    "justify-content", "align-items", "align-self", "align-content", "gap", "row-gap",
    "column-gap", "grid-template-columns", "grid-template-rows", "grid-column", "grid-row",
    // 边框
    "border", "border-top", "border-right", "border-bottom", "border-left", "border-radius",
    "border-width", "border-style", "border-color",
    // 分栏与分页
    "columns", "column-count", "break-inside", "page-break-inside", "float", "clear",
];

/// 成品层的存储形态。
///
/// 只存「怎么排」，不存内容 —— 内容永远以 blocks 为准。
/// 所以内容变了不需要显式让成品失效：`parts` 里对得上的块照用，
/// 对不上的（id 变了、块被删了）自然回落或消失。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLayout {
    /// classic / compact / roomy / custom；空表示用默认
    #[serde(default)]
    pub preset: String,
    /// 整页手写 HTML。存在时生成器完全不介入
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    /// 逐块手写片段：blockId → HTML
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub parts: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Compiled {
    /// `<article class="paper" data-preset="…">…</article>`
    pub html: String,
    pub css: &'static str,
    /// structured（生成器）/ manual（整页手写）
    pub mode: &'static str,
    pub preset: String,
    /// 生成过程中发现的问题：未知块、死引用、找不到的图片
    pub warnings: Vec<String>,
}

/* ---------- 入口 ---------- */

pub fn compile(page: &ResumePage, person: &Person) -> Compiled {
    let layout = page.layout.clone().unwrap_or_default();
    let preset = pick_preset(page, &layout);
    let mut warnings = Vec::new();

    // 整页手写：完全接管，生成器不介入
    if let Some(raw) = layout
        .html
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Compiled {
            html: shell(&sanitize(raw), &preset),
            css: PRESET_CSS,
            mode: "manual",
            preset,
            warnings,
        };
    }

    let body = if page.kind == "file" {
        render_file_page(page, &mut warnings)
    } else {
        render_blocks(page, person, &layout, &mut warnings)
    };

    Compiled {
        html: shell(&body, &preset),
        css: PRESET_CSS,
        mode: "structured",
        preset,
        warnings,
    }
}

fn shell(body: &str, preset: &str) -> String {
    format!(r#"<article class="doc" data-preset="{preset}">{body}</article>"#)
}

fn pick_preset(page: &ResumePage, layout: &PageLayout) -> String {
    let asked = layout.preset.trim();
    if PRESETS.contains(&asked) {
        return asked.to_string();
    }
    // 旧数据只有 style.density（三档疏密）：映射成预设，
    // 不让用户之前在参数层做过的调整凭空消失
    if let Some(density) = page
        .style
        .as_ref()
        .and_then(|style| style.get("density"))
        .and_then(Value::as_str)
    {
        if PRESETS.contains(&density) {
            return density.to_string();
        }
    }
    DEFAULT_PRESET.to_string()
}

/* ---------- 结构化生成 ---------- */

fn render_file_page(page: &ResumePage, warnings: &mut Vec<String>) -> String {
    let Some(asset_id) = page.asset_id.as_deref().filter(|id| !id.trim().is_empty()) else {
        warnings.push("这一页标成了原件页，但没有引用任何文件".into());
        return String::new();
    };
    format!(r#"<img class="full" src="asset:{}" alt="">"#, escape(asset_id))
}

fn render_blocks(
    page: &ResumePage,
    person: &Person,
    layout: &PageLayout,
    warnings: &mut Vec<String>,
) -> String {
    let mut out = String::new();
    let first_fields = page.blocks.iter().position(|block| block.kind == "fields");

    for (index, block) in page.blocks.iter().enumerate() {
        // 手写片段优先：agent 只改过这一块，其余照常生成
        if let Some(hand) = layout.parts.get(&block.id) {
            out.push_str(&sanitize(hand));
            continue;
        }

        match block.kind.as_str() {
            "fields" => {
                if Some(index) == first_fields {
                    out.push_str(&render_head(&block.fields, person));
                } else if !block.fields.is_empty() {
                    out.push_str(&render_group(&block.fields));
                }
            }
            "section" => out.push_str(&render_section(block, person)),
            "media" => out.push_str(&render_media(block, person, warnings)),
            "" => {}
            other => warnings.push(format!("跳过不认识的块类型：{other}")),
        }
    }
    out
}

/// 页头：第一个字段是大标题，其余按「标签：值」排；有证件照就并排
fn render_head(fields: &[Field], person: &Person) -> String {
    let Some((lead, rest)) = fields.split_first() else {
        return String::new();
    };

    let photo = photo_of(person);
    let title = lead_title(lead);
    // 字段都填着名却一个值都没有时，整块留空比留一条光秃秃的分隔线好
    let blank = title.trim().is_empty()
        && rest.iter().all(|field| field_text(field).trim().is_empty())
        && photo.is_none();
    if blank {
        return String::new();
    }

    let mut text = String::new();
    if !title.trim().is_empty() {
        text.push_str(&format!(r#"<h1 class="name">{}</h1>"#, escape(&title)));
    }
    for field in rest {
        let line = field_text(field);
        if line.trim().is_empty() {
            continue;
        }
        text.push_str(&format!(r#"<div class="meta">{}</div>"#, escape(&line)));
    }

    let inner = match photo {
        Some(asset_id) => format!(
            r#"<div class="top"><div class="txt">{text}</div><img class="photo" src="asset:{}" alt=""></div>"#,
            escape(&asset_id)
        ),
        None => text,
    };

    format!(r#"<header class="head">{inner}<div class="rule"></div></header>"#)
}

fn render_group(fields: &[Field]) -> String {
    let mut out = String::from(r#"<div class="group">"#);
    for field in fields {
        out.push_str(&format!(
            r#"<div class="meta">{}</div>"#,
            escape(&field_text(field))
        ));
    }
    out.push_str("</div>");
    out
}

fn render_section(block: &Block, person: &Person) -> String {
    let mut inner = String::new();
    if !block.title.trim().is_empty() {
        inner.push_str(&format!(r#"<h2 class="t">{}</h2>"#, escape(&block.title)));
    }
    for entry in &block.items {
        inner.push_str(&render_entry(entry, person));
    }
    format!(r#"<section class="sec">{inner}</section>"#)
}

fn render_entry(entry: &SectionEntry, person: &Person) -> String {
    // 本页字段优先；为空表示继承库里那一条
    let inherited;
    let fields: &[Field] = if !entry.fields.is_empty() {
        &entry.fields
    } else {
        inherited = entry
            .ref_id
            .as_deref()
            .and_then(|id| person.items.iter().find(|item| item.id == id));
        match inherited {
            Some(source) => &source.fields,
            None => &[],
        }
    };

    let mut inner = String::new();
    if let Some((lead, rest)) = fields.split_first() {
        inner.push_str(&format!("<b>{}</b>", escape(&lead_title(lead))));
        for field in rest {
            inner.push_str(&format!(
                r#"<div class="meta">{}</div>"#,
                escape(&field_text(field))
            ));
        }
    }

    let mut list = String::new();
    for bullet in entry.bullets.iter().filter(|line| !line.trim().is_empty()) {
        list.push_str(&format!("<li>{}</li>", escape(bullet)));
    }
    if !list.is_empty() {
        inner.push_str(&format!(r#"<ul class="bullets">{list}</ul>"#));
    }

    format!(r#"<div class="entry">{inner}</div>"#)
}

fn render_media(block: &Block, person: &Person, warnings: &mut Vec<String>) -> String {
    let Some(asset_id) = block.asset_id.as_deref().filter(|id| !id.trim().is_empty()) else {
        warnings.push("图片块没有引用任何资产".into());
        return String::new();
    };

    if find_asset(person, asset_id).is_none() {
        warnings.push(format!("图片块引用了找不到的资产：{asset_id}"));
        return r#"<figure class="fig missing">图片缺失</figure>"#.to_string();
    }

    let mut out = format!(
        r#"<figure class="fig"><img src="asset:{}" alt="">"#,
        escape(asset_id)
    );
    if let Some(caption) = block.caption.as_deref().filter(|text| !text.trim().is_empty()) {
        out.push_str(&format!("<figcaption>{}</figcaption>", escape(caption)));
    }
    out.push_str("</figure>");
    out
}

/* ---------- 小工具 ---------- */

fn photo_of(person: &Person) -> Option<String> {
    person
        .assets
        .iter()
        .find(|asset| asset.kind == "image")
        .map(|asset| asset.id.clone())
}

fn find_asset<'a>(person: &'a Person, id: &str) -> Option<&'a Asset> {
    person
        .assets
        .iter()
        .find(|asset| asset.id == id)
        .or_else(|| {
            person
                .items
                .iter()
                .flat_map(|item| item.assets.iter())
                .find(|asset| asset.id == id)
        })
}

/// 页头大标题、条目首字段：有值用值，没值退回标签
fn lead_title(field: &Field) -> String {
    let value = field.value.trim();
    if value.is_empty() {
        field.label.trim().to_string()
    } else {
        value.to_string()
    }
}

/// 与前端 `fieldText` 同一规则：有标签就「标签：值」
fn field_text(field: &Field) -> String {
    let value = field.value.trim();
    let label = field.label.trim();
    if label.is_empty() {
        return value.to_string();
    }
    if value.is_empty() {
        format!("{label}：")
    } else {
        format!("{label}：{value}")
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/* ---------- 清洗 ---------- */

fn sanitizer() -> &'static ammonia::Builder<'static> {
    static BUILDER: OnceLock<ammonia::Builder<'static>> = OnceLock::new();
    BUILDER.get_or_init(|| {
        let mut builder = ammonia::Builder::default();
        builder
            .tags(TAGS.iter().copied().collect::<HashSet<_>>())
            .generic_attributes(GENERIC_ATTRS.iter().copied().collect::<HashSet<_>>())
            // 只允许排版类属性；非法声明与 @ 规则会被这里剔掉
            .filter_style_properties(CSS_PROPS.iter().copied().collect::<HashSet<_>>())
            // 图片走 asset: 协议，由前端换成真实地址；data: 一律不放行，
            // 否则模型可以塞一段 data:text/html 进来
            .url_schemes(["asset", "http", "https", "mailto"].into_iter().collect())
            .attribute_filter(|_element, attribute, value| {
                // style 里带 url(...) 等于绕过协议白名单（背景图可以指向任意地址）
                if attribute == "style" && value.contains("url(") {
                    return None;
                }
                Some(Cow::Borrowed(value))
            });
        builder
    })
}

/// 清洗 agent 手写的片段。结构化生成的内容不走这里 —— 它由 `escape` 保证。
fn sanitize(html: &str) -> String {
    sanitizer().clean(html).to_string()
}

/* ---------- 命令 ---------- */

/// 把一页编译成可渲染的 HTML。
///
/// 前端只负责塞进容器、把 `asset:` 换成真实地址 —— 排版逻辑全在 Rust 侧，
/// 预览、打印、Word 导出才不会各排各的。
#[tauri::command]
pub fn compile_page(page: Value, person: Value) -> Result<Compiled, String> {
    let page: ResumePage =
        serde_json::from_value(page).map_err(|error| format!("这一页的数据不合法：{error}"))?;
    let person: Person =
        serde_json::from_value(person).map_err(|error| format!("候选人数据不合法：{error}"))?;
    Ok(compile(&page, &person))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn person() -> Person {
        serde_json::from_value(json!({
            "id": "p1",
            "fields": [],
            "assets": [{ "id": "a-photo", "kind": "image", "name": "照片.jpg", "stored": "x.jpg" }],
            "items": [{
                "id": "i1",
                "fields": [
                    { "id": "f1", "label": "公司", "value": "中远海运" },
                    { "id": "f2", "label": "职位", "value": "单证员" }
                ],
                "notes": ""
            }],
            "messages": [],
            "resumes": []
        }))
        .expect("person")
    }

    fn page() -> ResumePage {
        serde_json::from_value(json!({
            "id": "pg1",
            "kind": "native",
            "label": "简历",
            "blocks": [
                {
                    "id": "b1",
                    "kind": "fields",
                    "fields": [
                        { "id": "h1", "label": "姓名", "value": "张三" },
                        { "id": "h2", "label": "邮箱", "value": "z@example.com" }
                    ]
                },
                {
                    "id": "b2",
                    "kind": "section",
                    "title": "工作经历",
                    "items": [{
                        "id": "e1",
                        "refId": "i1",
                        "fields": [],
                        "bullets": ["负责报关单证", "对接船公司"]
                    }]
                }
            ]
        }))
        .expect("page")
    }

    #[test]
    fn head_uses_first_field_as_title_and_keeps_photo() {
        let out = compile(&page(), &person());
        assert!(out.html.contains(r#"<h1 class="name">张三</h1>"#), "{}", out.html);
        assert!(out.html.contains("邮箱：z@example.com"), "{}", out.html);
        // 证件照是人物第一张图片
        assert!(out.html.contains(r#"src="asset:a-photo""#), "{}", out.html);
        assert_eq!(out.mode, "structured");
    }

    #[test]
    fn entry_inherits_library_fields_when_page_has_none() {
        let out = compile(&page(), &person());
        assert!(out.html.contains("<b>中远海运</b>"), "{}", out.html);
        assert!(out.html.contains("职位：单证员"), "{}", out.html);
        assert!(out.html.contains("<li>负责报关单证</li>"), "{}", out.html);
    }

    #[test]
    fn generated_markup_stays_inside_the_subset() {
        let out = compile(&page(), &person());
        // 生成器自己不该产出 style：样式全在预设 CSS 里
        assert!(!out.html.contains("style="), "生成器不该内联样式：{}", out.html);
        // 也不该产出预设之外的标签
        for tag in ["script", "iframe", "form", "input", "link", "object"] {
            assert!(!out.html.contains(&format!("<{tag}")), "不该出现 {tag}");
        }
    }

    #[test]
    fn content_is_escaped_not_interpreted() {
        let mut page = page();
        page.blocks[0].fields[1].value = r#"<script>alert(1)</script>"#.into();
        let out = compile(&page, &person());
        assert!(!out.html.contains("<script>"), "{}", out.html);
        assert!(out.html.contains("&lt;script&gt;"), "{}", out.html);
    }

    #[test]
    fn hand_written_part_overrides_only_that_block() {
        let mut page = page();
        page.layout = Some(PageLayout {
            preset: "classic".into(),
            html: None,
            parts: [(
                "b2".to_string(),
                r#"<section class="sec"><h2 class="t">自定义</h2><p>手写内容</p></section>"#
                    .to_string(),
            )]
            .into_iter()
            .collect(),
        });

        let out = compile(&page, &person());
        assert!(out.html.contains("手写内容"), "{}", out.html);
        assert!(!out.html.contains("工作经历"), "被覆盖的块不该还在");
        // 没被覆盖的块照常生成
        assert!(out.html.contains(r#"<h1 class="name">张三</h1>"#), "{}", out.html);
    }

    #[test]
    fn hand_written_html_takes_over_the_whole_page() {
        let mut page = page();
        page.layout = Some(PageLayout {
            preset: "custom".into(),
            html: Some(r#"<div><h1>整页手写</h1></div>"#.into()),
            parts: HashMap::new(),
        });

        let out = compile(&page, &person());
        assert_eq!(out.mode, "manual");
        assert!(out.html.contains("整页手写"));
        assert!(!out.html.contains("张三"), "整页接管后不再有生成内容");
    }

    #[test]
    fn scripts_and_bad_attributes_are_stripped_from_hand_written_html() {
        let mut page = page();
        page.layout = Some(PageLayout {
            preset: "custom".into(),
            html: Some(
                r#"<div onclick="steal()"><script>alert(1)</script><p style="font-size:20px;position:fixed">保留</p></div>"#
                    .into(),
            ),
            parts: HashMap::new(),
        });

        let out = compile(&page, &person());
        assert!(!out.html.contains("<script"), "{}", out.html);
        assert!(!out.html.contains("onclick"), "{}", out.html);
        assert!(out.html.contains("保留"), "正文要留下：{}", out.html);
        // position 不在白名单里，必须被剔除；font-size 保留
        assert!(!out.html.contains("position"), "{}", out.html);
        assert!(out.html.contains("font-size"), "{}", out.html);
    }

    #[test]
    fn missing_media_is_reported_instead_of_rendering_a_broken_image() {
        let mut page = page();
        page.blocks.push(
            serde_json::from_value(json!({
                "id": "b3", "kind": "media", "assetId": "nope"
            }))
            .expect("media block"),
        );
        let out = compile(&page, &person());
        assert!(out.html.contains("missing"), "{}", out.html);
        assert!(!out.html.contains(r#"src="asset:nope""#), "{}", out.html);
        assert!(out.warnings.iter().any(|w| w.contains("nope")), "{:?}", out.warnings);
    }

    #[test]
    fn file_page_renders_the_original_as_one_image() {
        let mut page = page();
        page.kind = "file".into();
        page.asset_id = Some("a-photo".into());
        let out = compile(&page, &person());
        assert!(out.html.contains(r#"<img class="full" src="asset:a-photo""#), "{}", out.html);
    }

    #[test]
    fn legacy_density_maps_to_a_preset() {
        let mut page = page();
        page.style = Some(json!({ "density": "compact" }));
        assert_eq!(compile(&page, &person()).preset, "compact");

        page.style = Some(json!({ "density": "nonsense" }));
        assert_eq!(compile(&page, &person()).preset, DEFAULT_PRESET);
    }

    #[test]
    fn unknown_block_kind_is_skipped_with_a_warning() {
        let mut page = page();
        page.blocks.push(
            serde_json::from_value(json!({ "id": "b9", "kind": "chart" })).expect("block"),
        );
        let out = compile(&page, &person());
        assert!(out.warnings.iter().any(|w| w.contains("chart")), "{:?}", out.warnings);
        assert!(out.html.contains("张三"), "前面的内容要照常渲染");
    }

    #[test]
    fn svg_in_hand_written_html_survives_the_sanitizer() {
        let mut page = page();
        page.layout = Some(PageLayout {
            preset: "custom".into(),
            html: Some(
                r#"<svg viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" style="fill:#333"/></svg>"#
                    .into(),
            ),
            parts: HashMap::new(),
        });
        let out = compile(&page, &person());
        assert!(out.html.contains("<svg"), "{}", out.html);
        assert!(out.html.contains("<circle"), "{}", out.html);
    }

    #[test]
    #[ignore]
    fn dump_sample() {
        let out = compile(&page(), &person());
        println!("HTML://n{}", out.html);
        println!("\nHTML 字节数: {}", out.html.len());
        println!("CSS 字节数: {}", out.css.len());
        println!("warnings: {:?}", out.warnings);
    }

    /// 用真实数据库跑一遍：确认每页都能编译出东西，而不是静默产出空壳
    #[test]
    #[ignore]
    fn compile_real_pages() {
        let Ok(path) = std::env::var("DB_PATH") else {
            return;
        };
        let conn = crate::db::open(std::path::Path::new(&path)).expect("打开数据库");
        let Some(ws) = crate::db::load(&conn).expect("读库") else {
            println!("库里没有数据");
            return;
        };
        for person in &ws.people {
            for resume in &person.resumes {
                for page in &resume.pages {
                    let out = compile(page, person);
                    println!(
                        "{:>8} | {:<10} | {:>5} 字节 | {:<10} | 块 {} | 警告 {:?}",
                        page.id.chars().take(8).collect::<String>(),
                        if page.label.is_empty() { "未命名" } else { &page.label },
                        out.html.len(),
                        out.mode,
                        page.blocks.len(),
                        out.warnings
                    );
                }
            }
        }
    }

    /// 把真实数据里有内容的一页写成独立 HTML，交给真浏览器渲染用来肉眼验收
    #[test]
    #[ignore]
    fn dump_real_page_html() {
        let (Ok(db), Ok(out_path)) = (
            std::env::var("DB_PATH"),
            std::env::var("OUT_PATH"),
        ) else {
            return;
        };
        let conn = crate::db::open(std::path::Path::new(&db)).expect("打开数据库");
        let Some(ws) = crate::db::load(&conn).expect("读库") else {
            return;
        };
        for person in &ws.people {
            for resume in &person.resumes {
                for page in &resume.pages {
                    let out = compile(page, person);
                    if out.html.len() < 240 {
                        continue;
                    }
                    let html = format!(
                        "<!doctype html><meta charset=utf-8>\
                         <style>body{{margin:0;background:#9a9a94;padding:24px}}\
                         .sheet{{width:374px;min-height:528px;margin:0 auto;background:#fff;\
                         box-shadow:0 2px 12px rgba(0,0,0,.25)}}{}</style>\
                         <div class=sheet>{}</div>",
                        out.css, out.html
                    );
                    std::fs::write(&out_path, html).expect("写文件");
                    println!("{} 字节 / 页 {} / {} 块", out.html.len(), page.label, page.blocks.len());
                    return;
                }
            }
        }
        println!("没找到有内容的页");
    }

    #[test]
    fn an_all_empty_header_leaves_nothing_instead_of_a_lone_line() {
        // 真实数据里见过这种页：字段建好了但一个值都没填，
        // 结果纸上只剩一条分隔线，看着像渲染坏了
        let mut page = page();
        page.blocks[0] = serde_json::from_value(json!({
            "id": "b1",
            "kind": "fields",
            "fields": [{ "id": "f1", "label": "", "value": "" }]
        }))
        .expect("field block");

        let mut bare = person();
        bare.assets.clear();

        let out = compile(&page, &bare);
        assert!(!out.html.contains(r#"class="head""#), "{}", out.html);
        assert!(!out.html.contains(r#"class="rule""#), "{}", out.html);
        // 分区照常渲染，不能因为页头空就把整页吞了
        assert!(out.html.contains("工作经历"), "{}", out.html);
    }
}
