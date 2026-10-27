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
pub const PRESETS: [&str; 4] = ["classic", "compact", "roomy", "custom"];

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
/// 组件目录：给排版 agent 看的说明书，由 `read_layout_guide` 工具原样返回。
///
/// 放在这个文件里而不是 prompts 里：`TAGS`、`CSS_PROPS`、`paper.css` 一改，
/// 这里必须跟着改。三样东西在同一个目录下，改一处时更容易被看见。
///
/// 内容取舍：只写「有什么、怎么用、不能做什么」。不写「为什么要这样」——
/// agent 不需要设计动机，它需要一份能照着拼的清单。
pub const GUIDE: &str = r#"【这一页默认长什么样】
生成器按内容块顺序产出：页头 → 字段组 → 分区 → 图片。
页头里第一个文本字段是大标题，其余按「标签：值」排列，图片字段渲染成证件照；
分区是标题 + 条目，条目里第一个文本字段加粗、其余做次要信息、措辞排成列表。
样式全在共用样式表里，你只需要写 class，不用写 font-size 之类的声明。

【四档介入，从轻到重】
1. 什么都不写 —— 生成器负责，零成本。内容量正常、没有特殊要求就用这档。
2. preset —— 换整体疏密：classic / compact（内容多，压到一页）/ roomy（内容少，透气）
3. parts[blockId] —— 只重写某一块，其余照常生成。想改一段的排法时用这档。
4. html —— 整页接管。作品集、或结构化确实表达不了的版面才用。
   一旦设了 html，parts 与 preset 都不再起作用。

【组件清单】写在 class 里，布局类互斥、修饰类可叠加

布局
  cols-2 / cols-3      多栏流式排布（技能、短要点），内容会自动分配到各栏
  grid-2 / grid-3      两列 / 三列网格，各块独立，不会被流断
  split                左窄右宽两栏：左边放时间地点，右边放内容
  row                  一行两端对齐：左边标题、右边 .end 放时间
  stack                条目之间不留空隙，靠虚线断开
  panel                浅底色块，适合把技能、证书独立出来

分区标题（加在 section 上）
  sec.plain            去掉下划线
  sec.tight            标题只画到自己脚下，不拉通整行
  sec.band             标题变成浅底色带
  t.split              标题左、说明右（里面放 <small>2020 至今</small>）

分隔
  rule / rule-dash / rule-strong     细线 / 虚线 / 粗线

文本
  lead                 导语，比正文略大
  note                 小字注
  quote                左竖线引文
  mark                 加粗强调（不换底色）
  accent               用强调色

标签与键值
  tags > tag           标签云，每个 tag 是一个圆角小条；tag.solid 是实心
  kv                   行内键值对，适合「语言：英语 四级：通过」这类短项

图片
  fig                  普通图，figcaption 是图注
  fig.wide             通栏图
  fig.thumb            小图，挂在条目里当配图
  fig.round            圆形，适合头像与图标
  fig-row              一排图并排，各占等宽

表格
  table / table.plain  技能矩阵、时间线；plain 去掉行分隔线

页头（加在 header 上）
  head.center          居中，里面的照片顺便变成圆形
  head.bare            不放分隔线
  head.thick           粗分隔线

【能写的标签】
section header footer div p span h1 h2 h3 h4 h5 h6 strong b em i u small mark br hr
ul ol li dl dt dd figure figcaption img a blockquote table thead tbody tr th td
svg g path circle rect line polyline polygon ellipse text

【不能写】
script、style、iframe、form、input —— 会被剥掉。
CSS 里不能有 position、transform、animation、transition、z-index、float 之外的定位能力。
理由很实际：分页靠正常文档流，绝对定位的元素不知道自己该落在第几页，会压到页脚外面。

【图片怎么放】
src 写 asset:<id>，id 从 read_person 的材料清单里取。
不要写 data: 或外部网址，会被清洗掉。

【收尾前自查】
- 用字数估容量：compact 950–1150 字、normal 700–900、roomy 550–700（含标点）
- 超了就压一档或减少留白；压过头就放回来，落在区间内才算排好
- 你改的只是版面：内容一个字都不会因为排版而丢，放手调
"#;

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
    /// 未知字段原样保留：新版本写的成品，旧版本读一遍再存不能把它丢掉
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, Value>,
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
        check_asset_refs(raw, person, &mut warnings);
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
            check_asset_refs(hand, person, warnings);
            out.push_str(&sanitize(hand));
            continue;
        }

        match block.kind.as_str() {
            "fields" => {
                if Some(index) == first_fields {
                    out.push_str(&render_head(&block.fields, person, warnings));
                } else if !block.fields.is_empty() {
                    out.push_str(&render_group(&block.fields, person, warnings));
                }
            }
            "section" => out.push_str(&render_section(block, person, warnings)),
            "media" => out.push_str(&render_media(block, person, warnings)),
            "" => {}
            other => warnings.push(format!("跳过不认识的块类型：{other}")),
        }
    }
    out
}

/// 页头：第一个文本字段是大标题，其余按「标签：值」排；照片与文字并排
fn render_head(fields: &[Field], person: &Person, warnings: &mut Vec<String>) -> String {
    let (texts, images) = split_fields(fields);

    // 照片优先用页头里显式指定的那张字段图；旧数据没指定时退回人身上第一张，
    // 免得升级之后用户原有的证件照突然不见了
    let photo = images
        .iter()
        .filter_map(|field| field.image_id())
        .find(|id| {
            // 指向一个不存在的资产时不要输出 <img src="asset:…">：
            // 前端换不成真地址，纸上就是个破图标，比不显示更难看
            let known = find_asset(person, id).is_some();
            if !known {
                warnings.push(format!("页头照片引用了找不到的资产：{id}"));
            }
            known
        })
        .map(str::to_string)
        .or_else(|| photo_of(person));

    let title = texts.first().map(|field| lead_title(field)).unwrap_or_default();
    // 字段都填着名却一个值都没有时，整块留空比留一条光秃秃的分隔线好
    let blank = title.trim().is_empty()
        && texts.iter().skip(1).all(|field| field_text(field).trim().is_empty())
        && photo.is_none();
    if blank {
        return String::new();
    }

    let mut text = String::new();
    if !title.trim().is_empty() {
        text.push_str(&format!(r#"<h1 class="name">{}</h1>"#, escape(&title)));
    }
    for field in texts.iter().skip(1) {
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

/// 扫一段手写 HTML 里的 `asset:<id>`，把不存在的记成警告。
///
/// 清洗器只保证安全，不管引用有没有落脚点。不查的话，agent 写错一个 id
/// 之后，纸面上会留一个破图标，而它自己以为排好了。
fn check_asset_refs(html: &str, person: &Person, warnings: &mut Vec<String>) {
    let mut seen = HashSet::new();
    let mut rest = html;
    while let Some(at) = rest.find("asset:") {
        rest = &rest[at + "asset:".len()..];
        let id: String = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
            .collect();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        if find_asset(person, &id).is_none() {
            warnings.push(format!("手写 HTML 引用了找不到的图片：{id}"));
        }
    }
}

/// 一组字段里的图片：正文里按图排版，带图注/// 一组字段里的图片：正文里按图排版，带图注
fn render_field_images(
    images: &[&Field],
    class: &str,
    person: &Person,
    warnings: &mut Vec<String>,
) -> String {
    let mut out = String::new();
    for field in images {
        let Some(asset_id) = field.image_id() else {
            continue;
        };
        if find_asset(person, asset_id).is_none() {
            warnings.push(format!("字段图片引用了找不到的资产：{asset_id}"));
            continue;
        }
        let caption = field.value.trim();
        let mut inner = format!(r#"<img src="asset:{}" alt="">"#, escape(asset_id));
        if !caption.is_empty() {
            inner.push_str(&format!("<figcaption>{}</figcaption>", escape(caption)));
        }
        out.push_str(&format!(r#"<figure class="{class}">{inner}</figure>"#));
    }
    out
}

fn render_group(fields: &[Field], person: &Person, warnings: &mut Vec<String>) -> String {
    let (texts, images) = split_fields(fields);
    let mut inner = String::new();
    for field in &texts {
        let line = field_text(field);
        if line.trim().is_empty() {
            continue;
        }
        inner.push_str(&format!(r#"<div class="meta">{}</div>"#, escape(&line)));
    }
    inner.push_str(&render_field_images(&images, "fig", person, warnings));
    // 全空（字段没填、图片还指向不存在的资产）就别留一个空壳 div：
    // 它带着 margin-bottom，纸上会出现一段说不出来源的空白
    if inner.is_empty() {
        return String::new();
    }
    format!(r#"<div class="group">{inner}</div>"#)
}

fn render_section(block: &Block, person: &Person, warnings: &mut Vec<String>) -> String {
    let mut inner = String::new();
    if !block.title.trim().is_empty() {
        inner.push_str(&format!(r#"<h2 class="t">{}</h2>"#, escape(&block.title)));
    }
    for entry in &block.items {
        inner.push_str(&render_entry(entry, person, warnings));
    }
    format!(r#"<section class="sec">{inner}</section>"#)
}

fn render_entry(entry: &SectionEntry, person: &Person, warnings: &mut Vec<String>) -> String {
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

    let (texts, images) = split_fields(fields);

    let mut inner = String::new();
    if let Some((lead, rest)) = texts.split_first() {
        inner.push_str(&format!("<b>{}</b>", escape(&lead_title(lead))));
        for field in rest {
            inner.push_str(&format!(
                r#"<div class="meta">{}</div>"#,
                escape(&field_text(field))
            ));
        }
    }
    // 条目里的图片是「这一条的配图」：作品截图、证书照
    inner.push_str(&render_field_images(&images, "fig thumb", person, warnings));

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
    // 图片字段：值只当图注。空着时不要输出半截的「照片：」
    if field.image_id().is_some() {
        return if value.is_empty() {
            label.to_string()
        } else if label.is_empty() {
            value.to_string()
        } else {
            format!("{label}：{value}")
        };
    }
    if label.is_empty() {
        return value.to_string();
    }
    if value.is_empty() {
        format!("{label}：")
    } else {
        format!("{label}：{value}")
    }
}

/// 把一组字段拆成「能排成文字的行」与「图片」
///
/// 页头、补充字段组、条目都可能是混合的：一张照片夹在「姓名 / 邮箱」中间
/// 是很自然的事。分别处理比让图片混进文本行里靠谱 ——
/// 后者会输出一行「照片：a-3f2c」。
fn split_fields(fields: &[Field]) -> (Vec<&Field>, Vec<&Field>) {
    let mut text = Vec::new();
    let mut images = Vec::new();
    for field in fields {
        if field.image_id().is_some() {
            images.push(field);
        } else {
            text.push(field);
        }
    }
    (text, images)
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
            "assets": [
                { "id": "a-photo", "kind": "image", "name": "照片.jpg", "stored": "x.jpg" },
                { "id": "a-work", "kind": "image", "name": "作品.png", "stored": "y.png" }
            ],
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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

    /* ---------- 图片字段 ---------- */

    /// 页头里的图片字段就是证件照。
    ///
    /// 以前照片靠「人身上第一张图」猜，猜错时用户没有任何纠正手段；
    /// 现在由字段明确指定，而且图片字段不能再当文本行输出（会印出「照片：a-xxx」）。
    #[test]
    fn image_field_in_header_becomes_the_photo() {
        let mut page = page();
        page.blocks[0] = serde_json::from_value(json!({
            "id": "b1",
            "kind": "fields",
            "fields": [
                { "id": "h1", "label": "姓名", "value": "张三" },
                { "id": "h2", "label": "照片", "assetId": "a-photo", "value": "蓝底" }
            ]
        }))
        .expect("field block");

        let out = compile(&page, &person());
        assert!(out.html.contains(r#"<img class="photo" src="asset:a-photo""#), "{}", out.html);
        assert!(!out.html.contains("a-photo："), "资产 id 不能当文字印出来：{}", out.html);
        // 图注不进页头（页头只放身份信息），但也不能丢：这里不显示是刻意的
        assert!(out.html.contains("张三"), "{}", out.html);
    }

    /// 正文里的图片字段是一张带图注的图，不是一行字
    #[test]
    fn image_field_in_body_becomes_a_figure() {
        let mut page = page();
        page.blocks.push(
            serde_json::from_value(json!({
                "id": "b3",
                "kind": "fields",
                "fields": [
                    { "id": "f1", "label": "作品", "assetId": "a-work", "value": "系统截图" }
                ]
            }))
            .expect("field block"),
        );

        let out = compile(&page, &person());
        assert!(
            out.html.contains(r#"<figure class="fig"><img src="asset:a-work" alt=""><figcaption>系统截图</figcaption></figure>"#),
            "{}",
            out.html
        );
    }

    /// 字段全空的页头什么都不留：真实数据里见过只剩一条分隔线的页
    #[test]
    fn header_with_only_an_empty_image_leaves_nothing() {
        let mut page = page();
        page.blocks[0] = serde_json::from_value(json!({
            "id": "b1",
            "kind": "fields",
            "fields": [{ "id": "h1", "label": "", "value": "" }]
        }))
        .expect("field block");
        let mut bare = person();
        bare.assets.clear();

        let out = compile(&page, &bare);
        assert!(!out.html.contains(r#"class="head""#), "{}", out.html);
    }

    /// 条目里的图片字段是这一条的配图，用 thumb 尺寸
    #[test]
    fn image_field_in_entry_becomes_a_thumbnail() {
        let mut page = page();
        page.blocks[1] = serde_json::from_value(json!({
            "id": "b2",
            "kind": "section",
            "title": "作品",
            "items": [{
                "id": "e1",
                "fields": [
                    { "id": "f1", "label": "项目", "value": "调度系统" },
                    { "id": "f2", "label": "", "assetId": "a-work" }
                ],
                "bullets": []
            }]
        }))
        .expect("section block");

        let out = compile(&page, &person());
        assert!(out.html.contains(r#"<figure class="fig thumb">"#), "{}", out.html);
        assert!(out.html.contains("asset:a-work"), "{}", out.html);
    }

    /// 手写 HTML 里引用了不存在的图：要警告，不能静默留一个破图标
    #[test]
    fn hand_written_html_with_a_dead_image_id_warns() {
        let mut page = page();
        page.layout = Some(PageLayout {
            preset: "custom".into(),
            html: Some(r#"<p>看图</p><img src="asset:nope"><img src="asset:a-work">"#.into()),
            ..Default::default()
        });

        let out = compile(&page, &person());
        let dead: Vec<&String> = out
            .warnings
            .iter()
            .filter(|line| line.contains("nope"))
            .collect();
        assert_eq!(dead.len(), 1, "{:?}", out.warnings);
        // 存在的那个不能误报
        assert!(!out.warnings.iter().any(|line| line.contains("a-work")), "{:?}", out.warnings);
    }
}
