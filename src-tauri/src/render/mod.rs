//! 成品层：把结构化内容编译成一份可直接渲染的 HTML。
//!
//! 为什么要这一层，而不是继续在 React 里拼 JSX：
//!   1. **预览、打印、Word 导出必须共用同一份成品**。以前预览走 CSS、Word 走
//!      硬编码 OOXML，两边各排各的，agent 调的排版参数在 Word 里完全失效。
//!   2. **agent 要能改写它**。改 HTML 片段比改组件代码现实得多。
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

/// 版式预设。前四个只差字号与间距，`custom` 表示「完全由 agent 接管」。
///
/// `dense` 是「原件级」：正文 9.6px / 行高 1.42 / 无内边距。它存在的理由不是
/// 「再挤一点」，而是让一份本来就密的原件能被忠实复刻 —— 排版的第一个目标是
/// 像原件，装进一页是第二个目标。
pub const PRESETS: [&str; 5] = ["classic", "compact", "dense", "roomy", "custom"];

/// 页头版式。空串 = 生成器默认（照片在右、文字在左）。
///
/// 前三个是纸面上的 class（手写 parts 也能加），`aside` 是整块版式：
/// 姓名与联系方式居中、照片绝对定位在右上角 —— 原件最常见的排法。
/// 校验（set_page_layout）与生成（render_head）读同一份清单，免得
/// 「工具认了、生成器不认」。
pub const HEAD_STYLES: [&str; 5] = ["", "center", "bare", "thick", "aside"];

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
///
/// `data-block` 是块锚点：批注要贴到具体块上、以后「点纸面某段 → 跳到表单」
/// 也要靠它。`data-*` 默认会被 ammonia 丢掉，所以必须显式放行。
const GENERIC_ATTRS: &[&str] = &[
    "style", "class", "src", "alt", "width", "height", "href", "colspan", "rowspan", "viewbox",
    "data-block",
];

/// 契约第 5 节的 role 全集。认识的 role 才分派，其余回落 `plain` 并警告 ——
/// 静默回落会让 agent 以为自己排好了，实际什么都没变。
const ROLES: [&str; 12] = [
    "plain", "tight", "band", "tags", "kv", "table", "grid-2", "grid-3", "cols-2", "cols-3",
    "split", "row",
];

/// `split` 是「左栏元素 + 右栏元素」的双列结构，只有手写片段能保证这个 shape。
/// 结构化条目永远长成 `<b> + .meta… + <ul>`，不警告就只剩一张挤扁的纸。
const SPLIT_UNSUPPORTED: &str = "split 只适合手写片段（parts），结构化档不支持，已按 plain 处理";

/// `style` 里允许的属性。
///
/// 只开「排版」这一类：字体、间距、颜色、盒模型、布局、边框、分栏。
/// 刻意不开 `position`（模型手写绝对定位会脱离文档流，见 `ALLOW_ABSOLUTE`）、
/// `animation`（没有意义）、`z-index`（同上）。`transform` 只放行平移：
/// 缩放 / 旋转在 Word 里没有对应。
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

/// **模型手写** `style` 里能不能写 `position`。
///
/// 关着的时候 `position` / `top` / `left` / `right` / `bottom` / `z-index` 一律被
/// 清洗掉。注意这个开关只管模型手写的那一部分 —— 页头照片的绝对定位走
/// `HEAD_ASIDE_ABSOLUTE`，两者刻意分开。
///
/// 为什么仍然关着：页头照片只是一个 `<img class="photo abs">` 盒子，坐标由生成器
/// 写死成 `top:0;right:0`，跨页时最多是「这张图被切走」，风险可控；而模型自己把
/// 任意段落设成 `position:absolute`，一次就能把整段正文抽出文档流 —— 屏幕上看着
/// 还在，纸上的页数与 Word 的 `w:framePr` 同时失真，而它看不见渲染结果，发现不了。
///
/// 留成常量而不是配置项：这是**能力边界**，不是偏好，模型不该有机会把它打开。
pub const ALLOW_ABSOLUTE: bool = false;

/// 页头 `aside` 能不能真的把照片绝对定位到版心右上角。
///
/// 开启前提已经满足，三件事同时到位：
///   1. **Paged.js 兜底**：前端 `src/lib/paginate.ts` 已用 `Previewer` 真分页，
///      绝对定位元素跨页由分页引擎按页切分，不会压到页脚外面；
///   2. **`.doc` 是定位包含块**：`paper.css` 里 `.doc { position: relative }`，
///      照片的 `top:0;right:0` 就是「相对版心左上角」，与 Word 侧的 `w:framePr`
///      （`w:x` 已加上页边距）是同一个坐标系；
///   3. **docx 侧有 `w:framePr`**：`photo_para` 把照片放进 `w:framePr` 段落钉在
///      右上角，导出不是另一张纸。
///
/// 与 `ALLOW_ABSOLUTE` 拆开是因为风险不等价：这里只动生成器写死的那个 `<img>`，
/// 模型既写不到它、也改不了它的坐标；放开它不应该顺带放开整页正文的定位能力。
pub const HEAD_ASIDE_ABSOLUTE: bool = true;

/// 进白名单的定位属性。**当前恒为空**：`ALLOW_ABSOLUTE` 关着，模型写的
/// `position` / `top` … 全部清洗掉；留着这份清单是为了让「要开时开哪些」有唯一出处。
/// `width` / `height` 本来就在 `CSS_PROPS` 里（做卡片、图框用得到），不重复列。
const ABSOLUTE_PROPS: &[&str] = &["position", "top", "left", "right", "bottom", "z-index"];

/// 成品层的存储形态。
///
/// 只存「怎么排」，不存内容 —— 内容永远以 blocks 为准。
/// 所以内容变了不需要显式让成品失效：`parts` 里对得上的块照用，
/// 对不上的（id 变了、块被删了）自然回落或消失。
/// 组件目录：给 agent 看的说明书，由 `read_layout_guide` 工具原样返回。
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
2. preset —— 换整体疏密：classic / compact（内容多，压到一页）/ dense（原件级，字号与行距都贴着原件）/
   roomy（内容少，透气）
3. parts[blockId] —— 只重写某一块，其余照常生成。想改一段的排法时用这档。
   这一档**只改结构、class、样式，一个字都不能改**：文字和这一块的结构化内容对不上
   会被直接拒绝，并告诉你是哪一块。
4. html —— 整页接管。作品集、或结构化确实表达不了的版面才用。
   一旦设了 html，parts 与 preset 都不再起作用。
   它和 parts 一样**不许改字**：整页的纯文本要等于这一页所有块的文字之和，
   多一句少一句都会被拒。要改字就用内容工具（set_field / update_entry），
   写在 HTML 里的新字进不了数据层，只会让这次写入被拒。

【组件清单】写在 class 里，布局类互斥、修饰类可叠加

布局
  cols-2 / cols-3      多栏流式排布（技能、短要点），内容会自动分配到各栏
  grid-2 / grid-3      两列 / 三列网格，各块独立，不会被流断
  row                  一行两端对齐：左边标题、右边放时间。逐条生效
  stack                条目之间不留空隙，靠虚线断开
  panel                浅底色块，适合把技能、证书独立出来

手写片段（parts）专用，结构化档不支持
  split                左窄右宽两栏：左边放时间地点，右边放内容
                       要求里面正好是「一个左栏元素 + 一个右栏元素」；
                       生成器产出的条目给不出这个形状，role=split 会被忽略

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

页头（加在 header 上，也可以直接用 set_page_layout 的 head 选项）
  head.center          居中，里面的照片顺便变成圆形
  head.bare            不放分隔线
  head.thick           粗分隔线
  head.aside           姓名与联系方式居中、照片贴右上角（原件最常见的排法）
                      用 head 选项选它；自己手写页头会丢掉这套结构

**页头请用 head 选项，不要手写 <h1>。** 手写的页头会绕开 .head .top/.txt/.name/.meta
这套结构：名字居不居得中、照片在不在右上、导出的 Word 对不对得上，全靠它。

【能写的标签】
section header footer div p span h1 h2 h3 h4 h5 h6 strong b em i u small mark br hr
ul ol li dl dt dd figure figcaption img a blockquote table thead tbody tr th td
svg g path circle rect line polyline polygon ellipse text

【不能写】
script、style、iframe、form、input —— 会被剥掉。
CSS 里不能有 position、animation、transition、z-index —— 绝对定位的元素不知道自己
该落在第几页，会压到页脚外面（分页引擎接上之前一律关着）。
transform 只认 translate(...)：缩放在 Word 里没有对应，放开了就是「屏幕上对、导出去错」。

【图片怎么放】
src 写 asset:<id>，id 从 read_person 的材料清单里取。
不要写 data: 或外部网址，会被清洗掉。

【收尾前自查】
- 字数只是粗估：compact 约 950–1150 字、normal 约 700–900、roomy 约 550–700（含标点）。真实容量看「这一页的实测」，冲突时以实测为准。
- 超了就压一档或减少留白；压过头就放回来，实测达标才算排好（区间只作粗估）
- 你改的只是版面：内容一个字都不会因为排版而丢，放手调

【矢量与图表】
曲线、流程图、时间轴、进度条一律用 <svg>（含 g/path/circle/rect/line/polyline/polygon/ellipse/text）。
canvas 会被清洗掉，不要用；也不要写 <script>、事件属性（onclick 等）。
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
    /// 页头版式，取值见 `HEAD_STYLES`。
    ///
    /// 存成布局的一项，而不是让模型手写页头：页头结构一旦被手写覆盖，
    /// 「姓名居中 + 联系方式居中 + 照片右上」这个原件版式就没了 —— 而它正是
    /// 用户认得出「这是我那份简历」的地方。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub head: String,
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
                    out.push_str(&render_head(block, person, layout.head.trim(), warnings));
                } else if !block.fields.is_empty() {
                    out.push_str(&render_group(block, person, warnings));
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

/// 块的锚点：`data-block="<blockId>"`，加在每个块的根元素上。
///
/// id 来自库，形态是 `[A-Za-z0-9_-]`，本身安全；仍然过一遍 `escape` ——
/// 同一个字段将来会被导入的外部数据填进来，那时就不一定干净了。
fn block_attr(id: &str) -> String {
    format!(r#" data-block="{}""#, escape(id))
}

fn class_of(base: &str, extra: &str) -> String {
    match extra {
        "" => base.to_string(),
        _ => format!("{base} {extra}"),
    }
}

/// 规范化后的分区角色。用枚举而不是字符串，是为了让「哪种条目渲染」
/// 在 match 里必须被穷尽处理一次，不会漏掉某一档。
#[derive(Clone, Copy, PartialEq)]
enum SectionRole {
    Plain,
    Tight,
    Band,
    Tags,
    Kv,
    Table,
    /// class 加在 section 自己身上：整个分区变成网格 / 分栏
    SectionLayout(&'static str),
    /// class 只包条目、标题留在外面：`row` 的两端对齐不该把标题也算作一行里的元素
    EntryLayout(&'static str),
}

impl SectionRole {
    /// `role` 缺失 = plain（旧数据没有这个字段，不该刷警告）
    fn parse(block: &Block, warnings: &mut Vec<String>) -> Self {
        let Some(raw) = block
            .role
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        else {
            return SectionRole::Plain;
        };
        match raw {
            "plain" => SectionRole::Plain,
            "tight" => SectionRole::Tight,
            "band" => SectionRole::Band,
            "tags" => SectionRole::Tags,
            "kv" => SectionRole::Kv,
            "table" => SectionRole::Table,
            // 网格与多栏作用于整块：标题占满整行（CSS 里补了 grid-column: 1/-1）
            "grid-2" => SectionRole::SectionLayout("grid-2"),
            "grid-3" => SectionRole::SectionLayout("grid-3"),
            "cols-2" => SectionRole::SectionLayout("cols-2"),
            "cols-3" => SectionRole::SectionLayout("cols-3"),
            // 一行两端对齐，但只包条目：标题留在 section 里
            "row" => SectionRole::EntryLayout("row"),
            // split 是「一个左栏元素 + 一个右栏元素」的双列结构，结构化条目是
            // <b> + 若干 .meta + <ul>，塞进 78px 窄栏只会把公司名挤成三行
            "split" => {
                warnings.push(SPLIT_UNSUPPORTED.into());
                SectionRole::Plain
            }
            other => {
                warnings.push(format!("不认识的 role：{other}，按 plain 处理"));
                SectionRole::Plain
            }
        }
    }

    /// 追加到 `sec` 后面的 class；plain 什么都不加，保持旧成品逐字节不变
    fn class(self) -> &'static str {
        match self {
            SectionRole::Plain => "",
            SectionRole::Tight => "tight",
            SectionRole::Band => "band",
            SectionRole::Tags | SectionRole::Kv | SectionRole::Table => "",
            SectionRole::SectionLayout(role) => role,
            // 包条目的那两档不许碰到 section
            SectionRole::EntryLayout(_) => "",
        }
    }
}

/// 页头：第一个文本字段是大标题，其余按「标签：值」排；照片与文字并排
///
/// `head` 是版面里选定的页头版式（取值见 `HEAD_STYLES`，空 = 默认）。
/// `aside` 是原件最常见的排法：姓名与联系方式居中，照片贴在右上角。
/// 照片必须出到正常流外面 —— 证件照一旦参与那两行居中文字，居中就不是视觉居中了。
fn render_head(
    block: &Block,
    person: &Person,
    head: &str,
    warnings: &mut Vec<String>,
) -> String {
    let (texts, images) = split_fields(&block.fields);

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

    // `aside` 依赖绝对定位，由 `HEAD_ASIDE_ABSOLUTE` 单独把着（Paged.js 分页 +
    // `.doc` 定位包含块 + docx 的 `w:framePr` 都已就位）。它和「模型手写 position」
    // 是两件事：这里出的是生成器写死坐标的 `<img>`，不是模型的样式声明。
    let aside = head == "aside" && HEAD_ASIDE_ABSOLUTE;

    let inner = if aside {
        aside_inner(&text, photo.as_deref())
    } else {
        match photo {
            Some(asset_id) => format!(
                r#"<div class="top"><div class="txt">{text}</div><img class="photo" src="asset:{}" alt=""></div>"#,
                escape(&asset_id)
            ),
            None => text,
        }
    };

    let class = match head {
        "aside" if aside => "head aside",
        // 开关关掉时不要留 aside：留着会去匹配一条不存在的规则，纸上什么都不会变
        "center" => "head center",
        "bare" => "head bare",
        "thick" => "head thick",
        _ => "head",
    };

    // 居中写成内联样式，不新建 `.head.aside` 规则：paper.css 是共用样式表，
    // 为一条只在 `HEAD_ASIDE_ABSOLUTE` 下才走的分支加类，不如把这条声明就近
    // 写在生成处 —— 一眼能看出是谁要的
    let style = if aside {
        r#" style="text-align:center""#
    } else {
        ""
    };

    format!(
        r#"<header class="{class}"{style}{}>{inner}<div class="rule"></div></header>"#,
        block_attr(&block.id)
    )
}

/// 页头 `aside` 的内层：居中的文字 + 贴在右上角的照片。
///
/// 独立成函数是为了能单测 —— 这条分支由 `HEAD_ASIDE_ABSOLUTE` 把着，
/// 测试不该靠翻开关才能跑到它。
/// `abs` 只负责「脱出文档流」（`paper.css` 的 `.doc .abs`），偏移写成内联样式：
/// 类里写死 top/right 就绑死了一种纸。
fn aside_inner(text: &str, photo: Option<&str>) -> String {
    let image = photo
        .map(|asset_id| {
            format!(
                r#"<img class="photo abs" style="top:0;right:0" src="asset:{}" alt="">"#,
                escape(asset_id)
            )
        })
        .unwrap_or_default();
    format!(r#"<div class="txt">{text}</div>{image}"#)
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

fn render_group(block: &Block, person: &Person, warnings: &mut Vec<String>) -> String {
    let (texts, images) = split_fields(&block.fields);
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
    format!(
        r#"<div class="{}"{}>{inner}</div>"#,
        class_of("group", group_extra_class(block, warnings)),
        block_attr(&block.id)
    )
}

/// 字段组只认两栏：`grid-2` 各占一格不串行，`cols-2` 按流分栏。
/// 其余 role 在字段组上没有意义（没有标题、没有条目可拆），说出来比静默吞掉好。
fn group_extra_class(block: &Block, warnings: &mut Vec<String>) -> &'static str {
    let Some(role) = block
        .role
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return "";
    };
    match role {
        "plain" => "",
        "grid-2" => "grid-2",
        "cols-2" => "cols-2",
        // 字段组同样给不出「左栏 + 右栏」，理由与分区档相同
        "split" => {
            warnings.push(SPLIT_UNSUPPORTED.into());
            ""
        }
        other if ROLES.contains(&other) => {
            warnings.push(format!("role {other} 只对分区生效，字段组按 plain 处理"));
            ""
        }
        other => {
            warnings.push(format!("不认识的 role：{other}，按 plain 处理"));
            ""
        }
    }
}

/// 分区 → 容器。role 决定「用哪个 class」和「条目怎么落成子元素」。
///
/// 为什么拆成四个 renderer 而不是在一个函数里串 if：
/// tags / kv / table 对**条目**的解读完全不同（一条 = 一个标签 / 两列键值 / 一行表格），
/// 堆在同一个函数里，改 tags 的规则时会误伤 kv。
fn render_section(block: &Block, person: &Person, warnings: &mut Vec<String>) -> String {
    let role = SectionRole::parse(block, warnings);

    let mut inner = section_title(block);
    match role {
        SectionRole::Tags => {
            warn_if_lossy(block, person, "tags", warnings);
            inner.push_str(&render_tags(block, person));
        }
        SectionRole::Kv => {
            warn_if_lossy(block, person, "kv", warnings);
            inner.push_str(&render_kv(block, person));
        }
        SectionRole::Table => {
            warn_if_lossy(block, person, "table", warnings);
            inner.push_str(&render_table(block, person));
        }
        // 标题已经写进 inner 了，这里只把条目包进布局容器：
        // <section class="sec"><h2>标题</h2><div class="row">…</div></section>
        SectionRole::EntryLayout(container) => {
            let entries = render_entries(block, person, warnings);
            if !entries.is_empty() {
                inner.push_str(&format!(r#"<div class="{container}">{entries}</div>"#));
            }
        }
        // 纯样式档：条目还是「加粗首字段 + 次要信息 + 要点列表」，只是容器换了 class
        _ => inner.push_str(&render_entries(block, person, warnings)),
    }

    format!(
        r#"<section class="{}"{}>{inner}</section>"#,
        class_of("sec", role.class()),
        block_attr(&block.id)
    )
}

/// 常规条目序列。放在这里而不是内联进 `render_section`，
/// 是因为它会被两个分支用到，而 tags/kv/table 分支不该白跑一遍
/// —— 那会重复产出「图片缺失」之类的警告。
fn render_entries(block: &Block, person: &Person, warnings: &mut Vec<String>) -> String {
    let mut out = String::new();
    for entry in &block.items {
        out.push_str(&render_entry(entry, person, warnings));
    }
    out
}

fn section_title(block: &Block) -> String {
    if block.title.trim().is_empty() {
        return String::new();
    }
    format!(r#"<h2 class="t">{}</h2>"#, escape(&block.title))
}

/// 本页字段优先；为空表示继承库里那一条。
///
/// 四种渲染方式都要先回答「这一条到底有哪些字段」，判定只留这一处：
/// 页面自己存的字段与库里那条经常一半一半，各写一遍必然对不齐。
fn entry_fields<'a>(entry: &'a SectionEntry, person: &'a Person) -> &'a [Field] {
    if !entry.fields.is_empty() {
        return &entry.fields;
    }
    entry
        .ref_id
        .as_deref()
        .and_then(|id| person.items.iter().find(|item| item.id == id))
        .map(|item| item.fields.as_slice())
        .unwrap_or(&[])
}

/// 条目头：名称 / 中间字段 / 时间，**一行三段**。
///
/// 原件里一个条目就是一行（`公司  职位  时间`）。拆成一个字段一行不只是多占
/// 两行 —— 五条项目经历加一条工作、一条教育就是十几行，观感也完全不像原件。
pub(crate) struct EntryHead {
    pub lead: String,
    pub roles: Vec<String>,
    pub time: Option<String>,
}

fn render_entry(entry: &SectionEntry, person: &Person, warnings: &mut Vec<String>) -> String {
    let fields = entry_fields(entry, person);
    let (_, images) = split_fields(fields);
    let has_bullets = entry.bullets.iter().any(|line| !line.trim().is_empty());

    let mut inner = String::new();
    if let Some(head) = entry_head(fields, has_bullets) {
        if head.roles.is_empty() && head.time.is_none() {
            // 只有名称：不套 flex 层，`.entry > b` 的块级规则继续生效
            inner.push_str(&format!("<b>{}</b>", escape(&head.lead)));
        } else {
            let mut row = format!("<b>{}</b>", escape(&head.lead));
            for role in &head.roles {
                row.push_str(&format!(r#"<span class="role">{}</span>"#, escape(role)));
            }
            if let Some(time) = &head.time {
                row.push_str(&format!(r#"<span class="time">{}</span>"#, escape(time)));
            }
            inner.push_str(&format!(r#"<div class="erow">{row}</div>"#));
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

/// 把一组字段折成「名称 + 中间字段 + 时间」。
///
/// `has_bullets` 参与判断是必要的：「技能与兴趣」这类条目只有一个空值字段加
/// 一串要点，分区标题已经印了那四个字，头行再印一遍就是在纸上重复。
pub(crate) fn entry_head(fields: &[Field], has_bullets: bool) -> Option<EntryHead> {
    // 图片字段的「值」是图注，进头行会印出半句话；它走 render_field_images
    let texts: Vec<&Field> = fields.iter().filter(|f| f.image_id().is_none()).collect();
    let lead = *texts.first()?;
    if texts.len() == 1 && lead.value.trim().is_empty() && has_bullets {
        return None;
    }

    let rest = &texts[1..];
    // 只认最后一个：认不出来就当普通中间字段，硬塞到右边比不右对齐更难看
    let time_at = rest
        .len()
        .checked_sub(1)
        .filter(|&index| is_time_field(rest[index]));

    let mut roles = Vec::new();
    for (index, field) in rest.iter().enumerate() {
        if Some(index) == time_at {
            continue;
        }
        if let Some(text) = head_field_text(field) {
            roles.push(text);
        }
    }
    let time = time_at.and_then(|index| head_field_text(rest[index]));

    Some(EntryHead {
        lead: lead_title(lead).trim().to_string(),
        roles,
        time,
    })
}

/// 头行上一个字段印什么：有值印值，没值退回标签（原件就是这么印的：
/// `教育部直属 211` / `行政管理` / `产品助理`）。两个都空就整段省掉 ——
/// 留一个空 span 只会在行里撑出一段看不见的间距。
fn head_field_text(field: &Field) -> Option<String> {
    let value = field.value.trim();
    if !value.is_empty() {
        return Some(value.to_string());
    }
    let label = field.label.trim();
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

/// 时间字段只靠两件事认：标签，或值的形态。
///
/// 不写日期解析器 —— `2025年10月-至今` / `2025.9-2026.6` / `2025/09` 之外还有
/// 大量写法，真正的解析只会把「2025 届」这类也吞进去。
fn is_time_field(field: &Field) -> bool {
    if matches!(
        field.label.trim(),
        "时间" | "日期" | "起止时间" | "起止" | "年份"
    ) {
        return true;
    }
    has_date_shape(field.value.trim())
}

/// 值里出现「四位数字后跟 年 / . / - / /」就当它是日期区间
fn has_date_shape(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut digits = 0;
    for (index, ch) in chars.iter().enumerate() {
        if ch.is_ascii_digit() {
            digits += 1;
            continue;
        }
        if digits >= 4 {
            let next = chars[index..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('年') | Some('.') | Some('-') | Some('/')) {
                return true;
            }
        }
        digits = 0;
    }
    false
}

/// 标签云：一条只落一个 `<span class="tag">`。
///
/// 取第一个**文本**字段（图片字段的值是图注，落成标签会印出半句话）。
/// 值空着时退回标签名 —— 一个「公司」标签也比一片空白有信息量。
fn render_tags(block: &Block, person: &Person) -> String {
    let mut inner = String::new();
    for entry in &block.items {
        let (texts, _) = split_fields(entry_fields(entry, person));
        let Some(first) = texts.first() else {
            continue;
        };
        let text = lead_title(first);
        if text.trim().is_empty() {
            continue;
        }
        inner.push_str(&format!(r#"<span class="tag">{}</span>"#, escape(&text)));
    }
    if inner.is_empty() {
        return String::new();
    }
    format!(r#"<div class="tags">{inner}</div>"#)
}

/// 键值：每个文本字段一对 `dt/dd`，条目只是分组用的，不再单独包一层。
fn render_kv(block: &Block, person: &Person) -> String {
    let mut inner = String::new();
    for entry in &block.items {
        let (texts, _) = split_fields(entry_fields(entry, person));
        for field in texts {
            let label = field.label.trim();
            let value = field.value.trim();
            // 两个都空时输出 <dt></dt><dd></dd> 只会在纸上留一道空隙
            if label.is_empty() && value.is_empty() {
                continue;
            }
            inner.push_str(&format!(
                "<dt>{}</dt><dd>{}</dd>",
                escape(label),
                escape(value)
            ));
        }
    }
    if inner.is_empty() {
        return String::new();
    }
    format!(r#"<dl class="kv">{inner}</dl>"#)
}

/// 表格：一条一行，字段值按顺序成列，第一列加粗当行首。
///
/// 这里刻意不用 `field_text`（标签：值）：表格的列宽已经暗示了「标签」这一维，
/// 再把标签塞进单元格，一张技能表会挤成两行。
/// 也为空格保留 `<td>`：列数一旦错位，整张表就串行了。
fn render_table(block: &Block, person: &Person) -> String {
    let mut rows = String::new();
    for entry in &block.items {
        let (texts, _) = split_fields(entry_fields(entry, person));
        if texts.is_empty() {
            continue;
        }
        let mut row = String::from("<tr>");
        for (index, field) in texts.iter().enumerate() {
            let text = escape(field.value.trim());
            if index == 0 {
                row.push_str(&format!("<td><b>{text}</b></td>"));
            } else {
                row.push_str(&format!("<td>{text}</td>"));
            }
        }
        row.push_str("</tr>");
        rows.push_str(&row);
    }
    if rows.is_empty() {
        return String::new();
    }
    format!("<table><tbody>{rows}</tbody></table>")
}

/// tags / kv / table 只排字段，要点和多出来的字段会消失。
///
/// GUIDE 里承诺「内容不会因为排版而丢」，所以这里必须吭声：
/// agent 换了 role 之后以为排好了，实际用户少了两行字，是更糟的结果。
fn warn_if_lossy(block: &Block, person: &Person, role: &str, warnings: &mut Vec<String>) {
    // tags 一条只取第一个字段，其余算丢；kv / table 字段全排，只有要点会丢
    let keep = if role == "tags" { 1 } else { usize::MAX };
    let mut bullets = 0;
    let mut fields = 0;
    for entry in &block.items {
        bullets += entry.bullets.iter().filter(|line| !line.trim().is_empty()).count();
        let (texts, _) = split_fields(entry_fields(entry, person));
        fields += texts.len().saturating_sub(keep);
    }
    if bullets == 0 && fields == 0 {
        return;
    }
    let title = if block.title.trim().is_empty() {
        "未命名分区"
    } else {
        block.title.trim()
    };
    warnings.push(format!(
        "「{title}」用 {role} 排：{bullets} 条要点、{fields} 个字段没有出现在纸上，确认这是你要的"
    ));
}

fn render_media(block: &Block, person: &Person, warnings: &mut Vec<String>) -> String {
    let Some(asset_id) = block.asset_id.as_deref().filter(|id| !id.trim().is_empty()) else {
        warnings.push("图片块没有引用任何资产".into());
        return String::new();
    };

    if find_asset(person, asset_id).is_none() {
        warnings.push(format!("图片块引用了找不到的资产：{asset_id}"));
        // 缺失时也要留锚点：批注要贴在「这一块」上，而不是飘在页边
        return format!(
            r#"<figure class="fig missing"{}>图片缺失</figure>"#,
            block_attr(&block.id)
        );
    }

    let mut out = format!(
        r#"<figure class="fig"{}><img src="asset:{}" alt="">"#,
        block_attr(&block.id),
        escape(asset_id)
    );
    if let Some(caption) = block.caption.as_deref().filter(|text| !text.trim().is_empty()) {
        out.push_str(&format!("<figcaption>{}</figcaption>", escape(caption)));
    }
    out.push_str("</figure>");
    out
}

/* ---------- 小工具 ---------- */

/// 页头照片：人身上第一张图片。
///
/// 跳过 `role=="candidate"`：PDF 抽出来的图全部登记进 person.assets，不跳的话
/// 第一张 logo/水印会立刻变成页头证件照。老数据没有这个键，行为不变。
fn photo_of(person: &Person) -> Option<String> {
    let is_candidate = |asset: &Asset| {
        asset.extra.get("role").and_then(Value::as_str) == Some("candidate")
    };
    person
        .assets
        .iter()
        .find(|asset| asset.kind == "image" && !is_candidate(asset))
        // 没有 role 键 = 老数据（升级前就在的证件照），照旧取第一张
        .or_else(|| {
            person
                .assets
                .iter()
                .find(|asset| asset.kind == "image" && !asset.extra.contains_key("role"))
        })
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

/// `style` 实际生效的属性集合：静态清单 + `transform` + 开关打开时的定位属性。
///
/// 为什么不在常量里一次写全：模型手写的 `position` 能不能用是一道单独的能力边界，
/// 两个状态必须只有一处来源 —— 常量 `ALLOW_ABSOLUTE`（当前 false，与页头照片的
/// `HEAD_ASIDE_ABSOLUTE` 无关）。
fn style_props() -> HashSet<&'static str> {
    let mut set: HashSet<&'static str> = CSS_PROPS.iter().copied().collect();
    // transform 的名字要留在白名单里，值另行把关（见 strip_bad_transform）
    set.insert("transform");
    if ALLOW_ABSOLUTE {
        set.extend(ABSOLUTE_PROPS.iter().copied());
    }
    set
}

/// `transform` 只放行平移。
///
/// 缩放 / 旋转 / 斜切在 Word 里表达不了：预览看着对，导出就是另一张纸。
/// 平移能落到 OOXML 的绝对偏移上，所以留它 —— 照片微调时用得到。
/// 大小写不敏感（`Translate(...)` 同样合法），函数之间允许空格或逗号分隔。
pub(crate) fn transform_allowed(value: &str) -> bool {
    let chars: Vec<char> = value.trim().chars().collect();
    let mut index = 0;
    let mut seen = false;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_alphabetic() {
            let start = index;
            while index < chars.len() && (chars[index].is_alphabetic() || chars[index] == '-') {
                index += 1;
            }
            let name: String = chars[start..index].iter().collect::<String>().to_ascii_lowercase();
            // 函数名后面必须紧跟左括号，否则是裸关键字（`none`、`inherit` 也算）
            if index >= chars.len() || chars[index] != '(' {
                return false;
            }
            if !matches!(name.as_str(), "translate" | "translatex" | "translatey") {
                return false;
            }
            seen = true;
            let mut depth = 0usize;
            while index < chars.len() {
                match chars[index] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            index += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                index += 1;
            }
            // 括号没闭合：这句声明本身是坏的，留着只会让后面的属性一起失效
            if depth != 0 {
                return false;
            }
            continue;
        }
        if ch.is_whitespace() || ch == ',' {
            index += 1;
            continue;
        }
        // 函数外面出现别的字符 = 语法垃圾
        return false;
    }
    seen
}

/// 干掉 `style` 里不被接受的 `transform` 声明，其余声明原样保留。
///
/// 不能只靠属性名白名单：白名单只认名字，`transform: scale(0.9)` 会整条放行。
/// 而拒绝整条 style 又太狠 —— 同一行里可能还有一个合法的 `margin`。
fn strip_bad_transform(style: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut dropped = false;
    for decl in style.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        let (name, value) = match decl.split_once(':') {
            Some(pair) => pair,
            // 不完整的声明交给 ammonia 自己处理
            None => {
                kept.push(decl);
                continue;
            }
        };
        if name.trim().eq_ignore_ascii_case("transform") && !transform_allowed(value) {
            dropped = true;
            continue;
        }
        kept.push(decl);
    }
    if !dropped {
        return style.to_string();
    }
    kept.join("; ")
}

fn sanitizer() -> &'static ammonia::Builder<'static> {
    static BUILDER: OnceLock<ammonia::Builder<'static>> = OnceLock::new();
    BUILDER.get_or_init(|| {
        let mut builder = ammonia::Builder::default();
        builder
            .tags(TAGS.iter().copied().collect::<HashSet<_>>())
            // generic_attributes 就是属性白名单。ammonia 默认丢掉所有 `data-*`，
            // `data-block` 必须写在这里才会被留下 —— 写在 attribute_filter 里没用，
            // 那个回调只对「已经通过白名单」的属性执行。
            .generic_attributes(GENERIC_ATTRS.iter().copied().collect::<HashSet<_>>())
            // 只允许排版类属性；非法声明与 @ 规则会被这里剔掉
            .filter_style_properties(style_props())
            // 图片走 asset: 协议，由前端换成真实地址；data: 一律不放行，
            // 否则模型可以塞一段 data:text/html 进来
            .url_schemes(["asset", "http", "https", "mailto"].into_iter().collect())
            .attribute_filter(|_element, attribute, value| {
                if attribute != "style" {
                    return Some(Cow::Borrowed(value));
                }
                // style 里带 url(...) 等于绕过协议白名单（背景图可以指向任意地址）
                if value.contains("url(") {
                    return None;
                }
                // transform 的值单独把关：只放行 translate，缩放在 Word 里没有对应
                let cleaned = strip_bad_transform(value);
                if cleaned == value {
                    Some(Cow::Borrowed(value))
                } else {
                    Some(Cow::Owned(cleaned))
                }
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

    /// F-004：PDF 抽出来的候选图不能顶替页头照片；老数据没有 role 键，行为不变
    #[test]
    fn head_photo_never_picks_a_candidate() {
        let candidate = || {
            serde_json::from_value::<Asset>(json!({
                "id": "a-cand",
                "kind": "image",
                "name": "候选.jpg",
                "stored": "cand.jpg",
                "role": "candidate",
                "source": "src.pdf",
                "pageIndex": 2
            }))
            .expect("候选资产")
        };

        // 候选图排在真照片前面 → 仍然取真照片
        let mut with_candidate = person();
        with_candidate.assets.insert(0, candidate());
        assert_eq!(photo_of(&with_candidate).as_deref(), Some("a-photo"));

        // 全是候选 → 没有照片（页头不出 <img>），不 panic
        let mut only_candidates = person();
        only_candidates.assets.clear();
        only_candidates.assets.push(candidate());
        assert_eq!(photo_of(&only_candidates), None);

        // 老数据：两张图都没有 role → 还是第一张（升级后原有证件照不会消失）
        assert_eq!(photo_of(&person()).as_deref(), Some("a-photo"));
    }

    #[test]
    fn entry_inherits_library_fields_when_page_has_none() {
        let out = compile(&page(), &person());
        assert!(out.html.contains("<b>中远海运</b>"), "{}", out.html);
        assert!(out.html.contains(r#"<span class="role">单证员</span>"#), "{}", out.html);
        assert!(out.html.contains("<li>负责报关单证</li>"), "{}", out.html);
    }

    /// 条目头是一行三段：原件里的教育经历就是「学校  院校属性  专业  时间」一行
    #[test]
    fn entry_head_is_one_line_of_three_parts() {
        let page = solo_section(
            "plain",
            json!([{
                "id": "e1",
                "fields": [
                    { "id": "f1", "label": "学校", "value": "中山大学" },
                    { "id": "f2", "label": "院校属性", "value": "教育部直属 211" },
                    { "id": "f3", "label": "专业", "value": "行政管理" },
                    { "id": "f4", "label": "时间", "value": "2016年9月-2020年6月" }
                ],
                "bullets": []
            }]),
        );
        let out = compile(&page, &person());

        // 一个条目只有一行头：<b> + 2 个中间字段 + 1 个时间
        assert_eq!(out.html.matches(r#"class="erow""#).count(), 1, "{}", out.html);
        assert_eq!(out.html.matches(r#"class="role""#).count(), 2, "{}", out.html);
        assert_eq!(out.html.matches(r#"class="time""#).count(), 1, "{}", out.html);
        assert!(out.html.contains("<b>中山大学</b>"), "{}", out.html);
        assert!(
            out.html.contains(r#"<span class="role">教育部直属 211</span>"#),
            "{}",
            out.html
        );
        assert!(out.html.contains(r#"<span class="role">行政管理</span>"#), "{}", out.html);
        assert!(
            out.html.contains(r#"<span class="time">2016年9月-2020年6月</span>"#),
            "{}",
            out.html
        );
        // 不再是「一个字段一行、还带标签」
        assert!(!out.html.contains(r#"class="meta""#), "{}", out.html);
        assert!(!out.html.contains("时间："), "{}", out.html);
    }

    /// 第二个字段不是时间（项目 / 角色）：它留在中间，不硬塞到右边
    #[test]
    fn a_non_time_second_field_stays_in_the_middle() {
        let page = solo_section(
            "plain",
            json!([{
                "id": "e1",
                "fields": [
                    { "id": "f1", "label": "项目", "value": "调度系统" },
                    { "id": "f2", "label": "角色", "value": "产品助理" }
                ],
                "bullets": []
            }]),
        );
        let out = compile(&page, &person());

        assert_eq!(out.html.matches(r#"class="erow""#).count(), 1, "{}", out.html);
        assert!(out.html.contains(r#"<span class="role">产品助理</span>"#), "{}", out.html);
        assert!(!out.html.contains(r#"class="time""#), "{}", out.html);
    }

    /// 单字段空值 + 要点：分区标题已经印了「技能与兴趣」，头行不能再印一遍
    #[test]
    fn a_lone_empty_label_with_bullets_prints_no_head() {
        let page = solo_section(
            "plain",
            json!([{
                "id": "e1",
                "fields": [{ "id": "f1", "label": "技能与兴趣", "value": "" }],
                "bullets": ["报关单证", "船务对接"]
            }]),
        );
        let out = compile(&page, &person());

        assert!(!out.html.contains(r#"class="erow""#), "{}", out.html);
        assert!(!out.html.contains("技能与兴趣"), "{}", out.html);
        assert!(out.html.contains("<li>报关单证</li>"), "{}", out.html);
    }

    /// 值里出现「四位数字 + 年/./-/」也当时间，认不出标签的老数据同样能右对齐
    #[test]
    fn a_date_shaped_value_counts_as_time() {
        let mut field = Field {
            id: "f1".into(),
            label: "在岗".into(),
            value: "2025.10 - 至今".into(),
            asset_id: None,
            extra: Default::default(),
        };
        assert!(is_time_field(&field));
        field.value = "2025".into();
        assert!(!is_time_field(&field), "单个年份没有区间形态，不当时间");
        field.value = "三年".into();
        assert!(!is_time_field(&field));
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


    /// transform 只认平移：缩放/旋转在 Word 里没有对应，放开了就是「屏幕对、导出错」
    #[test]
    fn transform_allows_translate_only() {
        assert!(transform_allowed("translate(0, -2px)"));
        assert!(transform_allowed("Translate(-4px,2px)"));
        assert!(transform_allowed("translateX(3px) translateY(0)"));
        assert!(!transform_allowed("scale(0.9)"));
        assert!(!transform_allowed("rotate(3deg)"));
        assert!(!transform_allowed("skewX(4deg)"));
        assert!(!transform_allowed("translate(1px, 0) scale(2)"));
        assert!(!transform_allowed("translate(1px, 0) rotate(1deg)"));
        assert!(!transform_allowed("none"));
        assert!(!transform_allowed(""));
    }

    /// 清洗后：translate 留着，scale 整条剔掉，同行别的声明不受牵连
    #[test]
    fn sanitize_keeps_translate_and_drops_scale() {
        let out = sanitize(
            r#"<p style="transform: translate(1px, 2px); font-size: 12px">甲</p><p style="transform: scale(0.8); margin-top: 2px">乙</p>"#,
        );
        assert!(out.contains("translate(1px, 2px)"), "{out}");
        assert!(!out.contains("scale"), "{out}");
        // ammonia 会把 style 重新序列化成没有空格的紧凑形式
        assert!(out.contains("font-size:12px"), "{out}");
        assert!(out.contains("margin-top:2px"), "{out}");
    }

    /// 模型手写的 position 仍被清洗：页头照片开了绝对定位，不等于模型也能写定位
    #[test]
    fn model_written_position_is_still_stripped() {
        let out = sanitize(r#"<div style="position: absolute; top: 0; z-index: 3">甲</div>"#);
        assert!(!out.contains("position"), "{out}");
        assert!(!out.contains("z-index"), "{out}");

        let props = style_props();
        assert!(!props.contains("position"), "position 不该进白名单");
        for prop in ABSOLUTE_PROPS {
            assert!(!props.contains(prop), "{prop} 不该在白名单里");
        }
    }

    /// 两个开关是两件事：页头照片开了，模型手写这条仍关着
    #[test]
    fn head_aside_switch_is_not_the_model_style_switch() {
        assert!(HEAD_ASIDE_ABSOLUTE, "页头 aside 已可用");
        assert!(!ALLOW_ABSOLUTE, "模型手写 position 仍不放行");
    }

    /// head: aside 现在真的出绝对定位的照片（原件那种「姓名居中、照片贴右上角」）
    #[test]
    fn head_aside_pins_the_photo_to_the_top_right() {
        let mut page = page();
        page.layout = Some(PageLayout {
            head: "aside".into(),
            ..Default::default()
        });
        let out = compile(&page, &person());
        assert!(out.html.contains(r#"<header class="head aside""#), "{}", out.html);
        assert!(!out.html.contains(r#"<div class="top">"#), "不该再走两栏降级：{}", out.html);
        assert!(out.html.contains(r#"<img class="photo abs" style="top:0;right:0""#), "{}", out.html);
        assert!(out.html.contains(r#"style="text-align:center""#), "{}", out.html);
    }

    /// 没有照片时 aside 不留空 img，文字照样居中
    #[test]
    fn head_aside_without_a_photo_leaves_no_empty_image() {
        let mut page = page();
        page.layout = Some(PageLayout {
            head: "aside".into(),
            ..Default::default()
        });
        // page() 的页头没有图片字段，照片会退回人身上第一张；把人身上的也去掉，
        // 才是真的「没有照片」
        let mut bare_person = person();
        bare_person.assets.clear();
        let out = compile(&page, &bare_person);
        assert!(out.html.contains(r#"<header class="head aside""#), "{}", out.html);
        assert!(!out.html.contains(r#"class="photo abs""#), "不该输出空 img：{}", out.html);
        assert!(out.html.contains(r#"<div class="txt">"#), "{}", out.html);
    }

    /// aside 里的照片不参与文字居中：照片在 `<div class="txt">` 之后、`<header>` 之内
    #[test]
    fn head_aside_photo_sits_outside_the_centered_text() {
        let mut page = page();
        page.layout = Some(PageLayout {
            head: "aside".into(),
            ..Default::default()
        });
        let html = compile(&page, &person()).html;
        let header_at = html.find("<header").expect("有页头");
        let txt_at = html.find(r#"<div class="txt">"#).expect("有文字层");
        let img_at = html.find(r#"class="photo abs""#).expect("有照片");
        let header_end = html.find("</header>").expect("有页头结束");
        assert!(header_at < txt_at, "{}", html);
        assert!(txt_at < img_at, "照片必须排在文字之后（否则会挤掉居中）：{}", html);
        assert!(img_at < header_end, "照片必须留在 header 里：{}", html);
    }

    /// head: center 这类纸面 class 走选项时也要生效
    #[test]
    fn head_option_picks_the_paper_class() {
        let mut page = page();
        page.layout = Some(PageLayout {
            head: "center".into(),
            ..Default::default()
        });
        let out = compile(&page, &person());
        assert!(out.html.contains(r#"<header class="head center""#), "{}", out.html);
    }

    /// aside 真正出到纸上的形状：文字在流里、照片脱出文档流并钉在右上角。
    /// 直接测内层，不依赖开关（开关翻成 true 之前这条分支跑不到）
    #[test]
    fn aside_inner_pins_the_photo_to_the_top_right() {
        let inner = aside_inner(r#"<h1 class="name">张三</h1>"#, Some("a-photo"));
        assert!(inner.contains(r#"<div class="txt"><h1 class="name">张三</h1></div>"#), "{inner}");
        assert!(inner.contains(r#"class="photo abs""#), "{inner}");
        assert!(inner.contains("style=\"top:0;right:0\""), "{inner}");
        // 没有照片时只留文字，不要一个空的 img
        let bare = aside_inner("张三", None);
        assert!(!bare.contains("<img"), "{bare}");
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

    /* ---------- role → 容器 class（契约第 5 节） ---------- */

    /// 只有一页、一个分区的小页：把注意力集中在 role 的产出上
    fn solo_section(role: &str, items: Value) -> ResumePage {
        serde_json::from_value(json!({
            "id": "pg1",
            "kind": "native",
            "label": "简历",
            "blocks": [{
                "id": "b2",
                "kind": "section",
                "title": "技能",
                "role": role,
                "items": items
            }]
        }))
        .expect("section page")
    }

    /// 每条一个文本字段，值依次取传入的字符串
    fn one_field_entries(values: &[&str]) -> Value {
        let items: Vec<Value> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                json!({
                    "id": format!("e{index}"),
                    "fields": [{ "id": format!("f{index}"), "label": "技能", "value": value }],
                    "bullets": []
                })
            })
            .collect();
        Value::Array(items)
    }

    /// tags：一条一个 `<span class="tag">`，取的是第一个字段的**值**而不是「标签：值」
    #[test]
    fn tags_role_turns_each_entry_into_one_tag() {
        let page = solo_section("tags", one_field_entries(&["报关", "船务", "Excel"]));
        let out = compile(&page, &person());

        assert!(out.html.contains(r#"<div class="tags">"#), "{}", out.html);
        assert!(out.html.contains(r#"<span class="tag">报关</span>"#), "{}", out.html);
        assert!(out.html.contains(r#"<span class="tag">Excel</span>"#), "{}", out.html);
        // 标签是值，不该印成「技能：报关」
        assert!(!out.html.contains("技能：报关"), "{}", out.html);
        // 容器还是分区，标题照常生成
        assert!(out.html.contains(r#"<section class="sec""#), "{}", out.html);
        assert!(out.html.contains(r#"<h2 class="t">技能</h2>"#), "{}", out.html);
        // 一条字段全用上了，没有内容被丢，不该刷警告
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    }

    /// kv：每个字段一对 dt/dd，不再套 entry 那层 div（多一层会把两列挤歪）
    #[test]
    fn kv_role_renders_each_field_as_a_pair() {
        let page = solo_section(
            "kv",
            json!([{
                "id": "e1",
                "fields": [
                    { "id": "f1", "label": "英语", "value": "CET-6" },
                    { "id": "f2", "label": "普通话", "value": "二级甲等" }
                ],
                "bullets": []
            }]),
        );
        let out = compile(&page, &person());

        assert!(out.html.contains(r#"<dl class="kv">"#), "{}", out.html);
        assert!(out.html.contains("<dt>英语</dt><dd>CET-6</dd>"), "{}", out.html);
        assert!(out.html.contains("<dt>普通话</dt><dd>二级甲等</dd>"), "{}", out.html);
        assert!(!out.html.contains(r#"class="entry""#), "{}", out.html);
    }

    /// table：一条一行，字段值按顺序成列，第一列加粗当行首
    #[test]
    fn table_role_renders_one_row_per_entry_with_a_bold_first_cell() {
        let page = solo_section(
            "table",
            json!([
                {
                    "id": "e1",
                    "fields": [
                        { "id": "f1", "label": "年度", "value": "2023" },
                        { "id": "f2", "label": "单量", "value": "1200" }
                    ],
                    "bullets": []
                },
                {
                    "id": "e2",
                    "fields": [
                        { "id": "f3", "label": "年度", "value": "2024" },
                        { "id": "f4", "label": "单量", "value": "1500" }
                    ],
                    "bullets": []
                }
            ]),
        );
        let out = compile(&page, &person());

        assert!(out.html.contains("<table><tbody>"), "{}", out.html);
        assert!(
            out.html.contains("<tr><td><b>2023</b></td><td>1200</td></tr>"),
            "{}",
            out.html
        );
        assert!(
            out.html.contains("<tr><td><b>2024</b></td><td>1500</td></tr>"),
            "{}",
            out.html
        );
        // 单元格里只放值：标签已经在列语义里，再写进去一张表会挤成两行
        assert!(!out.html.contains("年度："), "{}", out.html);
    }

    /// tight / band 只是换 class，条目仍然照常渲染
    #[test]
    fn tight_and_band_only_add_a_class() {
        let out = compile(&solo_section("tight", one_field_entries(&["报关"])), &person());
        assert!(out.html.contains(r#"<section class="sec tight""#), "{}", out.html);
        assert!(out.html.contains(r#"<div class="entry""#), "{}", out.html);

        let out = compile(&solo_section("band", one_field_entries(&["报关"])), &person());
        assert!(out.html.contains(r#"<section class="sec band""#), "{}", out.html);
        assert!(out.html.contains(r#"<div class="entry""#), "{}", out.html);
    }

    /// grid-2 这类布局档同样保留常规条目，只是换容器
    #[test]
    fn layout_role_keeps_regular_entries() {
        let out = compile(&solo_section("grid-2", one_field_entries(&["甲", "乙"])), &person());
        assert!(out.html.contains(r#"<section class="sec grid-2""#), "{}", out.html);
        assert_eq!(out.html.matches(r#"class="entry""#).count(), 2, "{}", out.html);
    }

    /// row 仍然只包条目、标题留在 section 直下；样式改成逐条生效
    #[test]
    fn row_role_wraps_entries_and_targets_each_one() {
        let out = compile(&solo_section("row", one_field_entries(&["甲", "乙"])), &person());
        // 结构不变：标题在 section 直下，条目容器是它的兄弟
        assert!(
            out.html.contains(r#"<h2 class="t">技能</h2><div class="row">"#),
            "{}",
            out.html
        );
        assert!(!out.html.contains(r#"class="sec row""#), "{}", out.html);
        assert_eq!(out.html.matches(r#"class="entry""#).count(), 2, "{}", out.html);
        // 容器级 flex 会把两条挤成一行，样式必须落到 .entry 上
        assert!(PRESET_CSS.contains(".row > .entry"), "样式表缺 .row > .entry");
        // 头行在 row 容器里要占满整行，时间才贴得到右缘
        assert!(
            PRESET_CSS.contains(".row > .entry > .erow"),
            "样式表缺 .row > .entry > .erow"
        );
    }

    /// split 只适合手写片段：结构化条目给不出「左栏 + 右栏」，必须回落并吭声
    #[test]
    fn split_role_is_rejected_for_structured_pages() {
        let out = compile(&solo_section("split", one_field_entries(&["报关", "船务"])), &person());
        assert!(
            !out.html.contains(r#"class="split""#),
            "结构化档不该产出 split 容器：{}",
            out.html
        );
        assert!(out.html.contains(r#"<section class="sec""#), "{}", out.html);
        assert_eq!(out.html.matches(r#"class="entry""#).count(), 2, "{}", out.html);
        assert!(
            out.warnings.iter().any(|w| w.contains("split")),
            "{:?}",
            out.warnings
        );
        // 手写片段还在用 .split，样式表里不能删
        assert!(PRESET_CSS.contains(".split {"), "样式表缺 .split");
    }

    /// 不认识的 role 回落 plain 并且必须吭声：静默回落会让 agent 以为排好了
    #[test]
    fn unknown_role_falls_back_to_plain_with_a_warning() {
        let out = compile(
            &solo_section("masonry", one_field_entries(&["报关"])),
            &person(),
        );
        assert!(out.html.contains(r#"<section class="sec""#), "{}", out.html);
        assert!(!out.html.contains("masonry"), "{}", out.html);
        assert!(
            out.warnings.iter().any(|w| w.contains("masonry")),
            "{:?}",
            out.warnings
        );
    }

    /// tags 会丢掉要点和多余字段：必须警告，不能让「内容不会丢」变成空话
    #[test]
    fn tags_role_warns_when_it_drops_bullets() {
        let page = solo_section(
            "tags",
            json!([{
                "id": "e1",
                "fields": [{ "id": "f1", "label": "技能", "value": "报关" }],
                "bullets": ["负责单证"]
            }]),
        );
        let out = compile(&page, &person());
        assert!(out.html.contains(r#"<span class="tag">报关</span>"#), "{}", out.html);
        assert!(
            out.warnings.iter().any(|w| w.contains("要点")),
            "{:?}",
            out.warnings
        );
    }

    /// 每个结构化块的根元素都要带 data-block，否则批注无从贴起
    #[test]
    fn generated_blocks_carry_their_id() {
        let out = compile(&page(), &person());
        assert!(
            out.html.contains(r#"data-block="b1""#),
            "页头要带锚点：{}",
            out.html
        );
        assert!(
            out.html.contains(r#"data-block="b2""#),
            "分区要带锚点：{}",
            out.html
        );
    }

    /// ammonia 默认丢弃所有 `data-*`。这条专门盯清洗器：
    /// 生成器写进去、又恰好没走清洗，不能算「通过了 ammonia」。
    #[test]
    fn data_block_survives_the_sanitizer() {
        let cleaned = sanitize(r#"<section data-block="b7"><p>正文</p></section>"#);
        assert!(cleaned.contains(r#"data-block="b7""#), "{cleaned}");

        // 白名单只开了这一个：别的 data-* 仍然要被丢掉
        let cleaned = sanitize(r#"<p data-track="x" data-block="b8">正文</p>"#);
        assert!(cleaned.contains(r#"data-block="b8""#), "{cleaned}");
        assert!(!cleaned.contains("data-track"), "{cleaned}");
    }

    /// 页头之后插一个补充字段组，role 决定它是什么角色
    fn page_with_group(role: &str) -> ResumePage {
        let mut page = page();
        page.blocks.insert(
            1,
            serde_json::from_value(json!({
                "id": "b9",
                "kind": "fields",
                "role": role,
                "fields": [
                    { "id": "g1", "label": "电话", "value": "13800000000" },
                    { "id": "g2", "label": "城市", "value": "上海" }
                ]
            }))
            .expect("group block"),
        );
        page
    }

    /// 补充字段组支持两栏；其余 role 对字段组没意义，要说出来而不是静默吞掉
    #[test]
    fn a_fields_block_can_be_a_two_column_group() {
        let out = compile(&page_with_group("grid-2"), &person());
        assert!(
            out.html
                .contains(r#"<div class="group grid-2" data-block="b9">"#),
            "{}",
            out.html
        );
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);

        let out = compile(&page_with_group("table"), &person());
        assert!(out.html.contains(r#"<div class="group" data-block="b9">"#), "{}", out.html);
        assert!(
            out.warnings.iter().any(|w| w.contains("只对分区生效")),
            "{:?}",
            out.warnings
        );
    }
}
