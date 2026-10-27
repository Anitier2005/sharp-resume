//! 工作区的强类型模型（v5）。
//!
//! 为什么在 Rust 再定义一遍（前端 types 里也有一份）：
//!   前端那份是为了渲染，这份是为了**校验和落盘**。后端权威的前提是后端能判断
//!   「这份数据合不合法」，只靠前端自己说「我写的是对的」不算权威。
//!
//! v5 的关键变化：**页 = 导出文件夹里的一个文件**
//!   - `native` 页由我们构建，内容是一串 block（字段 / 分区 / 媒体）
//!   - `file` 页是用户原件（证书、视频、PDF），只引用资产池里的一份文件，
//!     导出时逐字节拷贝，永不转码
//!   一页不是「PDF 的一页」，所以一页可以只是一张图。
//!
//! 关键约束：**未知字段必须原样保留**。
//!   每个结构体都带 `#[serde(flatten)] extra`，把不认识的键收进来再写回去。
//!   否则 Rust 一旦接管写入，前端将来新增的字段会在一次往返后被静默抹掉 ——
//!   这是最难查的一类数据丢失（文件还在，内容却少了）。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// 当前数据模型版本。低于它的在 `adopt` 里逐级升级
pub const VERSION: u32 = 5;

/// 未知字段的容器。空的时候不输出，避免把 `"extra": {}` 写进文件
fn is_empty(map: &Map<String, Value>) -> bool {
    map.is_empty()
}

fn is_empty_vec<T>(list: &[T]) -> bool {
    list.is_empty()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub value: String,
    /// 这一项是图片时，指向资产池里的那份文件。
    ///
    /// 有它时 `value` 当图注用（可以空着）。字段能装图片这件事是必要的：
    /// 证件照、作品截图、证书扫描件在用户脑子里就是「页面上的一项内容」，
    /// 让他先去材料区传一张、再回来指望我们猜哪张是头像，是把结构问题推给人。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "is_empty")]
    pub extra: Map<String, Value>,
}

impl Field {
    /// 这一项是不是图片（有引用且不是空串）
    pub fn image_id(&self) -> Option<&str> {
        self.asset_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
    }
}

/// 材料 / 资产。
///
/// `stored` 是资产池里的文件名（内容哈希 + 扩展名），不是路径。
/// `sha256` 用来证明「导出的就是用户当初给的那份」，不转码、不重压缩。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    /// image / link / file
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    /// 桌面端是池内存储名；浏览器调试模式是 data URL；link 类型则是网址
    #[serde(default)]
    pub stored: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: String,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub notes: String,
    #[serde(flatten, default, skip_serializing_if = "is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionEntry {
    pub id: String,
    /// 引用库条目 id；为空表示这一页临时写的
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_id: Option<String>,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub bullets: Vec<String>,
    #[serde(flatten, default, skip_serializing_if = "is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub items: Vec<SectionEntry>,
    #[serde(flatten, default, skip_serializing_if = "is_empty")]
    pub extra: Map<String, Value>,
}

/// 页里的一个块。
///
/// 为什么不用 enum：`#[serde(tag)]` 的内部标记 enum 不支持 `flatten`，
/// 会丢掉未知字段。用一个宽容结构体 + `kind` 判别，前端照样按 union 读。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: String,
    /// fields / section / media
    pub kind: String,
    #[serde(default, skip_serializing_if = "is_empty_vec")]
    pub fields: Vec<Field>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "is_empty_vec")]
    pub items: Vec<SectionEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// contain / cover；只是排版提示，缺失就是 contain
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "is_empty")]
    pub extra: Map<String, Value>,
}

fn default_page_kind() -> String {
    "native".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumePage {
    pub id: String,
    /// native（我们构建）/ file（用户原件）
    #[serde(default = "default_page_kind")]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    /// file 页引用的资产 id
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// 排版参数由「页排版 agent」写，这里不拆开校验，保持前向兼容
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Value>,
    /// 成品层：版式预设 + 手写覆盖。结构与语义见 `render::PageLayout`。
    ///
    /// 与 `style` 并存而不是替换：`style` 是旧的三档疏密（读旧数据时映射成预设），
    /// 这里是真正的成品 —— 可以整页手写，也可以只改某几个块。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<crate::render::PageLayout>,
    #[serde(default)]
    pub blocks: Vec<Block>,
    /// v4 遗留：页头字段。只在读旧数据时出现，`normalize` 会把它搬进 blocks
    #[serde(default, skip_serializing_if = "is_empty_vec")]
    pub header: Vec<Field>,
    /// v4 遗留：分区。同上
    #[serde(default, skip_serializing_if = "is_empty_vec")]
    pub sections: Vec<Section>,
    #[serde(flatten, default, skip_serializing_if = "is_empty")]
    pub extra: Map<String, Value>,
}

impl ResumePage {
    /// v4 → v5：页头变成第一个 fields 块，分区依次变成 section 块。
    ///
    /// 幂等：blocks 已经非空时不再搬，否则每次读都会多插一份页头。
    fn normalize(&mut self) {
        if self.kind.trim().is_empty() {
            self.kind = "native".into();
        }
        // 页名不在这里动。
        //
        // 试过把「第 12 页」这类自动名清成空、界面显示「未命名页」，结果四五个页签
        // 全叫「未命名页」，比原来的数字更难区分 —— 已经退回去了。
        // 页名怎么显示是界面的事，别在数据层动手。
        // 只要旧的 header / sections 还在就搬；搬完它们会清空，所以反复调用不会重复插入
        if self.header.is_empty() && self.sections.is_empty() {
            return;
        }
        if !self.header.is_empty() {
            let fields = std::mem::take(&mut self.header);
            self.blocks.push(Block {
                id: format!("{}-head", self.id),
                kind: "fields".into(),
                fields,
                title: String::new(),
                items: Vec::new(),
                asset_id: None,
                caption: None,
                fit: None,
                extra: Map::new(),
            });
        }
        for section in std::mem::take(&mut self.sections) {
            self.blocks.push(Block {
                id: section.id,
                kind: "section".into(),
                fields: Vec::new(),
                title: section.title,
                items: section.items,
                asset_id: None,
                caption: None,
                fit: None,
                extra: section.extra,
            });
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resume {
    pub id: String,
    #[serde(default)]
    pub person_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub pages: Vec<ResumePage>,
    #[serde(flatten, default, skip_serializing_if = "is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: String,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub items: Vec<LibraryItem>,
    /// 会话真相源在 SQLite 的 chat 表；这个字段只是旧数据兼容，不再写入
    #[serde(default, skip_serializing_if = "is_empty_vec")]
    pub messages: Vec<Value>,
    #[serde(default)]
    pub resumes: Vec<Resume>,
    #[serde(flatten, default, skip_serializing_if = "is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ui {
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_hex: Option<String>,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub active_person_id: Option<String>,
    #[serde(default)]
    pub active_resume_id: Option<String>,
    #[serde(default)]
    pub expanded: Vec<String>,
    #[serde(default)]
    pub text_scale: Option<f64>,
    #[serde(default)]
    pub side_w: Option<f64>,
    #[serde(default)]
    pub doc_w: Option<f64>,
    #[serde(default)]
    pub show_side: Option<bool>,
    #[serde(default)]
    pub show_doc: Option<bool>,
    /// edit / preview
    #[serde(default)]
    pub view: Option<String>,
    /// 页签栏的 MRU 顺序（最近的在前）。
    /// 空的时候不输出 —— 前端见到 undefined 才会用「当前简历全部页」兜底。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_pages: Vec<String>,
    /// 上次导出到的目录（用户可以在设置里改）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_dir: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "is_empty")]
    pub extra: Map<String, Value>,
}

fn default_accent() -> String {
    "white".into()
}

fn default_mode() -> String {
    "light".into()
}

/// 手写 Default 而不是 derive：accent / mode 的默认值不是 `String::default()`（空串），
/// 空 accent 会让前端的主题选择器匹配不上，界面直接失去强调色
impl Default for Ui {
    fn default() -> Self {
        Self {
            accent: default_accent(),
            accent_hex: None,
            mode: default_mode(),
            active_person_id: None,
            active_resume_id: None,
            expanded: Vec::new(),
            text_scale: None,
            side_w: None,
            doc_w: None,
            show_side: None,
            show_doc: None,
            view: None,
            open_pages: Vec::new(),
            export_dir: None,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub version: u32,
    /// 资产池的元数据。文件本体在 `<数据目录>/pool/`，这里只记「它是什么」。
    /// `file` 页与 `media` 块通过 id 引用这里的条目。
    #[serde(default, skip_serializing_if = "is_empty_vec")]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub people: Vec<Person>,
    #[serde(default)]
    pub ui: Ui,
    #[serde(flatten, default, skip_serializing_if = "is_empty")]
    pub extra: Map<String, Value>,
}

impl Workspace {
    /// 把任意历史形态补齐成 v5。只做结构补齐，不改内容语义。
    pub fn normalize(&mut self) {
        self.version = VERSION;
        for person in &mut self.people {
            for resume in &mut person.resumes {
                for page in &mut resume.pages {
                    page.normalize();
                }
            }
        }
    }
}

/* ---------- v1 / v2 / v3 → v4 ---------- */

/// 旧版本迁移。搬自前端 `migrate.ts`：数据处理属于后端，前端只拿结果渲染。
///
/// 单向、幂等：v4 及以上原样返回。迁移必须无损 —— 原有内容全部变成字段值，
/// 一句不丢（只是键名由用户接管）。
mod legacy {
    use super::*;

    fn text(value: Option<&Value>) -> String {
        value.and_then(Value::as_str).unwrap_or("").trim().to_string()
    }

    fn uid(prefix: &str) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_millis())
            .unwrap_or(0);
        format!("{prefix}-{stamp:x}{seq:x}")
    }

    fn field(label: &str, value: Option<&Value>) -> Option<Value> {
        let content = text(value);
        if content.is_empty() {
            return None;
        }
        Some(serde_json::json!({ "id": uid("f"), "label": label, "value": content }))
    }

    fn compact(list: Vec<Option<Value>>) -> Vec<Value> {
        list.into_iter().flatten().collect()
    }

    fn array_of<'a>(item: &'a Value, key: &str) -> &'a [Value] {
        item.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
    }

    fn item_with(fields: Vec<Value>, notes: String) -> Value {
        serde_json::json!({ "id": uid("it"), "fields": fields, "notes": notes })
    }

    fn migrate_experience(item: &Value) -> Value {
        let mut fields = compact(vec![
            field("公司", item.get("company")),
            field("职位", item.get("role")),
            field("时间", item.get("period")),
        ]);
        for fact in array_of(item, "facts") {
            let label = text(fact.get("label"));
            fields.extend(field(if label.is_empty() { "细节" } else { &label }, fact.get("text")));
        }
        let mut notes = text(item.get("notes"));
        if notes.is_empty() {
            // v1 把要点叫 bullets，v2 改成 notes；这里统一进「详实记录」
            notes = array_of(item, "bullets")
                .iter()
                .filter_map(|line| line.as_str())
                .collect::<Vec<_>>()
                .join("\n");
        }
        let mut out = item_with(fields, notes);
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            out["id"] = Value::from(id);
        }
        out
    }

    fn migrate_project(item: &Value) -> Value {
        let tags = array_of(item, "tags")
            .iter()
            .filter_map(|tag| tag.as_str())
            .collect::<Vec<_>>()
            .join("、");
        // desc 在 v3 是**会印在简历上的**描述，所以迁成字段而不是 notes ——
        // notes 是只给自己看的素材，落到那里等于从纸上消失
        let fields = compact(vec![
            field("项目", item.get("title")),
            field("时间", item.get("year")),
            field("简介", item.get("desc")),
            field("标签", Some(&Value::from(tags))),
        ]);
        let mut out = item_with(fields, String::new());
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            out["id"] = Value::from(id);
        }
        out
    }

    fn migrate_education(item: &Value) -> Value {
        let fields = compact(vec![
            field("学校", item.get("school")),
            field("学位", item.get("degree")),
            field("时间", item.get("period")),
        ]);
        item_with(fields, String::new())
    }

    fn migrate_skill(item: &Value) -> Value {
        let fields = compact(vec![
            field("类别", item.get("label")),
            field("内容", item.get("value")),
        ]);
        item_with(fields, String::new())
    }

    /// 旧页面的 entries（引用经历 + 本页措辞）→ 分区块
    fn migrate_entry(entry: &Value) -> Value {
        let mut bullets: Vec<String> = Vec::new();
        for key in ["bullets", "bulletsEn"] {
            for line in array_of(entry, key) {
                let line = text(Some(line));
                if !line.is_empty() && !bullets.contains(&line) {
                    bullets.push(line);
                }
            }
        }
        let ref_id = text(entry.get("experienceId"));
        let mut out = serde_json::json!({
            "id": uid("se"),
            "fields": [],
            "bullets": bullets,
        });
        if let Some(id) = entry.get("id").and_then(Value::as_str) {
            out["id"] = Value::from(id);
        }
        if !ref_id.is_empty() {
            out["refId"] = Value::from(ref_id);
        }
        out
    }

    fn section(title: &str, items: Vec<Value>) -> Value {
        serde_json::json!({ "id": uid("sc"), "title": title, "items": items })
    }

    fn migrate_page(page: &Value, order: usize) -> Value {
        let mut header = compact(vec![
            field("姓名", page.get("headline")),
            field("职位", page.get("role")),
        ]);
        for meta in array_of(page, "meta") {
            header.extend(field("", Some(meta)));
        }
        header.extend(field("摘要", page.get("summary")));

        let mut sections: Vec<Value> = Vec::new();
        let entries = array_of(page, "entries");
        if !entries.is_empty() {
            sections.push(section(
                "工作经历",
                entries.iter().map(migrate_entry).collect(),
            ));
        }
        for (key, title) in [("experienceIds", "工作经历"), ("projectIds", "作品")] {
            let ids = array_of(page, key);
            if ids.is_empty() {
                continue;
            }
            // 极早期数据：只存了 id，没有措辞
            let items: Vec<Value> = ids
                .iter()
                .filter_map(|id| id.as_str())
                .map(|id| {
                    serde_json::json!({ "id": uid("se"), "refId": id, "fields": [], "bullets": [] })
                })
                .collect();
            sections.push(section(title, items));
        }

        let label = text(page.get("label"));
        serde_json::json!({
            "id": if text(page.get("id")).is_empty() { uid("pg") } else { text(page.get("id")) },
            "label": if label.is_empty() { format!("第 {} 页", order + 1) } else { label },
            "header": header,
            "sections": sections,
        })
    }

    fn migrate_person(person: &Value) -> Value {
        let fields = if person.get("fields").map(Value::is_array).unwrap_or(false) {
            person.get("fields").cloned().unwrap_or_else(|| Value::Array(Vec::new()))
        } else {
            // 年限在 v1 里是**数字**，直接丢给 text() 会拿到空串、整个字段消失。
            // 前端原来是拼成「3 年」，这里保持一致，否则迁移一次少一个字段。
            let years = match person.get("years") {
                Some(Value::Number(value)) => format!("{value} 年"),
                Some(Value::String(value)) if !value.trim().is_empty() => {
                    let value = value.trim().trim_end_matches('年').trim();
                    if value.is_empty() { String::new() } else { format!("{value} 年") }
                }
                _ => String::new(),
            };
            let years = Value::String(years);
            Value::Array(compact(vec![
                field("姓名", person.get("name")),
                field("职位", person.get("role")),
                field("邮箱", person.get("email")),
                field("城市", person.get("location")),
                field("年限", Some(&years)),
            ]))
        };

        let items = if person.get("items").map(Value::is_array).unwrap_or(false) {
            person.get("items").cloned().unwrap_or_else(|| Value::Array(Vec::new()))
        } else {
            let mut list: Vec<Value> = Vec::new();
            for item in array_of(person, "experiences") {
                list.push(migrate_experience(item));
            }
            for item in array_of(person, "projects") {
                list.push(migrate_project(item));
            }
            for item in array_of(person, "education") {
                list.push(migrate_education(item));
            }
            for item in array_of(person, "skills") {
                list.push(migrate_skill(item));
            }
            Value::Array(list)
        };

        let person_id = {
            let id = text(person.get("id"));
            if id.is_empty() { uid("p") } else { id }
        };

        let resumes: Vec<Value> = array_of(person, "resumes")
            .iter()
            .map(|resume| {
                let pages: Vec<Value> = array_of(resume, "pages")
                    .iter()
                    .enumerate()
                    .map(|(order, page)| migrate_page(page, order))
                    .collect();
                let id = {
                    let value = text(resume.get("id"));
                    if value.is_empty() { uid("r") } else { value }
                };
                let title = {
                    let value = text(resume.get("title"));
                    if value.is_empty() { "未命名简历".to_string() } else { value }
                };
                let person_ref = {
                    let value = text(resume.get("personId"));
                    if value.is_empty() { person_id.clone() } else { value }
                };
                serde_json::json!({
                    "id": id,
                    "personId": person_ref,
                    "title": title,
                    "updatedAt": resume.get("updatedAt").and_then(Value::as_u64).unwrap_or(0),
                    "pages": pages,
                })
            })
            .collect();

        serde_json::json!({
            "id": person_id,
            "fields": fields,
            "items": items,
            "messages": person.get("messages").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
            "resumes": resumes,
        })
    }

    /// 老数据没有 openPages：先按「当前简历的全部页」铺开。
    /// 之后打开任意一页都会把它顶到最前，顺序由使用习惯自然接管。
    fn initial_open_pages(ui: Option<&Value>, people: &[Value]) -> Vec<String> {
        let active_person = ui.and_then(|value| value.get("activePersonId")).and_then(Value::as_str);
        let active_resume = ui.and_then(|value| value.get("activeResumeId")).and_then(Value::as_str);
        let person = people
            .iter()
            .find(|item| item.get("id").and_then(Value::as_str) == active_person)
            .or_else(|| people.first());
        let Some(person) = person else { return Vec::new() };
        let resumes = array_of(person, "resumes");
        let resume = resumes
            .iter()
            .find(|item| item.get("id").and_then(Value::as_str) == active_resume)
            .or_else(|| resumes.first());
        let Some(resume) = resume else { return Vec::new() };
        array_of(resume, "pages")
            .iter()
            .filter_map(|page| page.get("id").and_then(Value::as_str).map(str::to_string))
            .collect()
    }

    pub fn to_v4(raw: Value) -> Result<Value, String> {
        let source = raw
            .as_object()
            .ok_or_else(|| "工作区数据格式不对".to_string())?;
        let people_raw = source
            .get("people")
            .and_then(Value::as_array)
            .ok_or_else(|| "工作区数据格式不对".to_string())?;
        let people: Vec<Value> = people_raw.iter().map(migrate_person).collect();
        let ui_raw = source.get("ui");

        let pick = |key: &str| -> Option<Value> { ui_raw.and_then(|value| value.get(key)).cloned() };
        let mut ui = Map::new();
        ui.insert("accent".into(), pick("accent").unwrap_or_else(|| Value::from("white")));
        if let Some(value) = pick("accentHex") {
            ui.insert("accentHex".into(), value);
        }
        ui.insert("mode".into(), pick("mode").unwrap_or_else(|| Value::from("light")));
        ui.insert(
            "activePersonId".into(),
            pick("activePersonId").unwrap_or(Value::Null),
        );
        ui.insert(
            "activeResumeId".into(),
            pick("activeResumeId").unwrap_or(Value::Null),
        );
        ui.insert("expanded".into(), pick("expanded").unwrap_or_else(|| Value::Array(Vec::new())));
        for key in ["textScale", "sideW", "docW", "showSide", "showDoc", "view", "exportDir"] {
            if let Some(value) = pick(key) {
                ui.insert(key.into(), value);
            }
        }
        ui.insert(
            "openPages".into(),
            Value::Array(
                initial_open_pages(ui_raw, &people)
                    .into_iter()
                    .map(Value::from)
                    .collect(),
            ),
        );

        Ok(serde_json::json!({ "version": 4, "people": people, "ui": ui }))
    }
}

/* ---------- 入口 ---------- */

/// 把任意版本的原生 JSON 接管成 v5 强类型。
///
/// 顺序：v1/v2/v3 → v4（Value 层，结构完全不同）→ 反序列化 → v4→v5（结构层搬运）。
pub fn adopt(raw: Value) -> Result<Workspace, String> {
    let version = raw.get("version").and_then(Value::as_u64).unwrap_or(1);
    let upgraded = if version < 4 { legacy::to_v4(raw)? } else { raw };
    let mut workspace: Workspace = serde_json::from_value(upgraded)
        .map_err(|error| format!("工作区结构无法解析：{error}"))?;
    workspace.normalize();
    Ok(workspace)
}

/* ---------- 校验 ---------- */

/// 结构层面的基本完整性：id 不能空、简历必须挂在存在的候选人下、页 id 不重复、
/// file 页必须有资产、block 的 kind 必须认得。
///
/// 只查「一定不对」的东西，不查业务规则（比如「一页至少要有一个分区」）——
/// 那是编辑期的约束，不该拦一次保存。
pub fn validate(workspace: &Workspace) -> Result<(), String> {
    if workspace.version > VERSION {
        return Err(format!(
            "这份数据是更新版本（v{}）写的，本程序只到 v{VERSION}",
            workspace.version
        ));
    }
    let mut person_ids = std::collections::HashSet::new();
    for person in &workspace.people {
        if person.id.trim().is_empty() {
            return Err("候选人的 id 不能为空".into());
        }
        if !person_ids.insert(person.id.as_str()) {
            return Err(format!("候选人 id 重复：{}", person.id));
        }

        let mut item_ids = std::collections::HashSet::new();
        for item in &person.items {
            if item.id.trim().is_empty() {
                return Err(format!("候选人 {} 名下有经历条目缺 id", person.id));
            }
            if !item_ids.insert(item.id.as_str()) {
                return Err(format!("经历条目 id 重复：{}", item.id));
            }
        }

        for resume in &person.resumes {
            if resume.id.trim().is_empty() {
                return Err(format!("候选人 {} 名下有简历缺 id", person.id));
            }
            if !resume.person_id.is_empty() && resume.person_id != person.id {
                return Err(format!(
                    "简历 {} 的 personId（{}）与所属候选人（{}）不一致",
                    resume.id, resume.person_id, person.id
                ));
            }
            let mut page_ids = std::collections::HashSet::new();
            for page in &resume.pages {
                if page.id.trim().is_empty() {
                    return Err(format!("简历 {} 下有页面缺 id", resume.id));
                }
                if !page_ids.insert(page.id.as_str()) {
                    return Err(format!("页面 id 重复：{}", page.id));
                }
                match page.kind.as_str() {
                    "native" => {
                        let mut block_ids = std::collections::HashSet::new();
                        for block in &page.blocks {
                            if block.id.trim().is_empty() {
                                return Err(format!("页 {} 里有块缺 id", page.id));
                            }
                            if !block_ids.insert(block.id.as_str()) {
                                return Err(format!("页 {} 里块 id 重复：{}", page.id, block.id));
                            }
                            if !matches!(block.kind.as_str(), "fields" | "section" | "media") {
                                return Err(format!(
                                    "页 {} 里有不认识的块类型：{}",
                                    page.id, block.kind
                                ));
                            }
                            if block.kind == "media"
                                && block.asset_id.as_deref().unwrap_or("").trim().is_empty()
                            {
                                return Err(format!("页 {} 的媒体块没有指向任何资产", page.id));
                            }
                        }
                    }
                    "file" => {
                        if page.asset_id.as_deref().unwrap_or("").trim().is_empty() {
                            return Err(format!("文件页 {} 没有指向任何资产", page.id));
                        }
                    }
                    other => {
                        return Err(format!("页 {} 的类型不认识：{other}", page.id));
                    }
                }
            }
        }
    }
    Ok(())
}

/// 查找一个人名下的全部页面 id —— 导出/资产引用统计都要用
pub fn page_ids(workspace: &Workspace) -> Vec<String> {
    workspace
        .people
        .iter()
        .flat_map(|person| &person.resumes)
        .flat_map(|resume| &resume.pages)
        .map(|page| page.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_fields_survive_a_roundtrip() {
        let raw = json!({
            "version": 5,
            "people": [{
                "id": "p1",
                "fields": [{ "id": "f1", "label": "姓名", "value": "张三", "pinned": true }],
                "items": [],
                "resumes": [{
                    "id": "r1",
                    "title": "简历",
                    "pages": [{
                        "id": "pg1",
                        "kind": "native",
                        "label": "一",
                        "blocks": [{ "id": "b1", "kind": "fields", "fields": [] }],
                        "futureLayout": { "columns": 2 }
                    }]
                }],
                "futureField": 42
            }],
            "ui": { "accent": "white", "mode": "light", "openPages": ["pg1"] },
            "futureTop": "keep me"
        });

        let workspace = adopt(raw).expect("应能解析");
        let back = serde_json::to_value(&workspace).expect("应能序列化");

        assert_eq!(back["futureTop"], json!("keep me"));
        assert_eq!(back["people"][0]["futureField"], json!(42));
        assert_eq!(back["people"][0]["fields"][0]["pinned"], json!(true));
        assert_eq!(
            back["people"][0]["resumes"][0]["pages"][0]["futureLayout"]["columns"],
            json!(2)
        );
        assert_eq!(back["people"][0]["fields"][0]["value"], json!("张三"));
        assert_eq!(back["ui"]["openPages"][0], json!("pg1"));
    }

    #[test]
    fn empty_extra_is_not_written() {
        let workspace = adopt(json!({
            "version": 5,
            "people": [],
            "ui": { "accent": "white", "mode": "light" }
        }))
        .unwrap();
        let text = serde_json::to_string(&workspace).unwrap();
        assert!(!text.contains("\"extra\""), "{text}");
    }

    #[test]
    fn v4_header_and_sections_become_blocks() {
        let workspace = adopt(json!({
            "version": 4,
            "people": [{
                "id": "p1",
                "resumes": [{
                    "id": "r1",
                    "personId": "p1",
                    "pages": [{
                        "id": "pg1",
                        "label": "第 1 页",
                        "header": [{ "id": "h1", "label": "姓名", "value": "张三" }],
                        "sections": [{
                            "id": "s1",
                            "title": "工作经历",
                            "items": [{ "id": "e1", "bullets": ["做了事"] }]
                        }]
                    }]
                }]
            }],
            "ui": {}
        }))
        .unwrap();

        let page = &workspace.people[0].resumes[0].pages[0];
        assert_eq!(workspace.version, 5);
        assert_eq!(page.kind, "native");
        assert_eq!(page.blocks.len(), 2);
        assert_eq!(page.blocks[0].kind, "fields");
        assert_eq!(page.blocks[0].fields[0].value, "张三");
        assert_eq!(page.blocks[1].kind, "section");
        assert_eq!(page.blocks[1].title, "工作经历");
        assert_eq!(page.blocks[1].items[0].bullets, vec!["做了事".to_string()]);
        // 旧字段不再写回
        let back = serde_json::to_value(&workspace).unwrap();
        assert!(back["people"][0]["resumes"][0]["pages"][0].get("header").is_none());
        assert!(back["people"][0]["resumes"][0]["pages"][0].get("sections").is_none());
        // 幂等
        let mut again = workspace.clone();
        again.normalize();
        assert_eq!(again.people[0].resumes[0].pages[0].blocks.len(), 2);
    }

    #[test]
    fn v1_tree_is_migrated_losslessly() {
        // 极早期：人身上挂 experiences/projects/education/skills，页面只存 id
        let workspace = adopt(json!({
            "version": 1,
            "people": [{
                "id": "p1",
                "name": "李四",
                "role": "单证员",
                "experiences": [{
                    "id": "x1",
                    "company": "中远",
                    "role": "单证",
                    "period": "2020-2024",
                    "bullets": ["A", "B"],
                    "facts": [{ "label": "规模", "text": "日 240 票" }]
                }],
                "projects": [{ "id": "j1", "title": "系统", "year": "2024", "desc": "做了个系统" }],
                "education": [{ "id": "e1", "school": "海事大学", "degree": "本科" }],
                "skills": [{ "id": "k1", "label": "语言", "value": "英语六级" }],
                "resumes": [{
                    "id": "r1",
                    "title": "外贸单证",
                    "pages": [{ "id": "pg1", "label": "中文", "experienceIds": ["x1"], "projectIds": ["j1"] }]
                }]
            }],
            "ui": { "activePersonId": "p1", "activeResumeId": "r1" }
        }))
        .unwrap();

        let person = &workspace.people[0];
        assert_eq!(person.fields[0].value, "李四");
        assert_eq!(person.items.len(), 4, "四类旧数据都该变成库条目");
        // 要点不能丢：v1 的 bullets 进 notes
        let experience = &person.items[0];
        assert!(experience.notes.contains('A'), "{}", experience.notes);
        assert!(experience.fields.iter().any(|f| f.value == "日 240 票"));
        // 项目描述必须进字段（落 notes 就不会印在纸上）
        let project = person.items.iter().find(|i| i.fields.iter().any(|f| f.label == "项目")).unwrap();
        assert!(project.fields.iter().any(|f| f.label == "简介" && f.value == "做了个系统"));

        let page = &person.resumes[0].pages[0];
        assert_eq!(page.blocks.len(), 2, "两个 id 列表各成一个分区");
        assert_eq!(page.blocks[0].items[0].ref_id.as_deref(), Some("x1"));
        // 老数据补出 openPages，页签栏才不会空
        assert_eq!(workspace.ui.open_pages, vec!["pg1".to_string()]);
    }

    #[test]
    fn validate_catches_structural_broken_data() {
        let mut workspace = adopt(json!({
            "version": 5,
            "people": [{ "id": "p1", "resumes": [{ "id": "r1", "personId": "p1", "pages": [] }] }],
            "ui": {}
        }))
        .unwrap();
        assert!(validate(&workspace).is_ok());

        workspace.people[0].resumes[0].person_id = "p2".into();
        assert!(validate(&workspace).unwrap_err().contains("不一致"));

        workspace.people[0].resumes[0].person_id = "p1".into();
        workspace.people[0].resumes[0].pages = vec![
            serde_json::from_value(json!({ "id": "pg1", "kind": "native" })).unwrap(),
            serde_json::from_value(json!({ "id": "pg1", "kind": "native" })).unwrap(),
        ];
        assert!(validate(&workspace).unwrap_err().contains("页面 id 重复"));

        // 文件页必须指向资产
        workspace.people[0].resumes[0].pages = vec![
            serde_json::from_value(json!({ "id": "pg2", "kind": "file" })).unwrap(),
        ];
        assert!(validate(&workspace).unwrap_err().contains("没有指向任何资产"));

        // 媒体块同理
        workspace.people[0].resumes[0].pages = vec![serde_json::from_value(json!({
            "id": "pg3",
            "kind": "native",
            "blocks": [{ "id": "b1", "kind": "media" }]
        }))
        .unwrap()];
        assert!(validate(&workspace).unwrap_err().contains("媒体块"));
    }

    #[test]
    fn ui_defaults_when_missing() {
        let workspace = adopt(json!({ "version": 5, "people": [] })).unwrap();
        assert_eq!(workspace.ui.accent, "white");
        assert_eq!(workspace.ui.mode, "light");
        assert!(workspace.ui.open_pages.is_empty());
    }

    #[test]
    fn v1_workspace_upgrades_without_losing_anything() {
        let raw = json!({
            "version": 1,
            "people": [{
                "id": "p1",
                "name": "张三",
                "role": "工程师",
                "email": "z@example.com",
                "location": "上海",
                "years": 3,
                "experiences": [{
                    "id": "e1",
                    "company": "某公司",
                    "role": "开发",
                    "period": "2020—2023",
                    "bullets": ["做了 A", "做了 B"]
                }],
                "projects": [],
                "education": [],
                "skills": [],
                "resumes": [{
                    "id": "r1",
                    "title": "简历",
                    "pages": [{
                        "id": "pg1",
                        "label": "中文",
                        "headline": "张三",
                        "experienceIds": ["e1"]
                    }]
                }]
            }]
        });

        let ws = adopt(raw).expect("v1 要能升级");
        let person = &ws.people[0];

        // 年限在 v1 是数字：必须变成「3 年」，不能被当成空串丢掉
        let years = person.fields.iter().find(|f| f.label == "年限").expect("年限字段");
        assert_eq!(years.value, "3 年");
        assert_eq!(person.fields.len(), 5, "姓名/职位/邮箱/城市/年限 一个都不能少");

        // 经历进库，v1 的 bullets 是「记录」而不是页面措辞
        assert_eq!(person.items.len(), 1);
        assert_eq!(person.items[0].notes, "做了 A\n做了 B");

        // 页面的 experienceIds 变成「工作经历」分区，条目引用库里那一条
        let page = &person.resumes[0].pages[0];
        let section = page
            .blocks
            .iter()
            .find(|block| block.kind == "section")
            .expect("应当有分区块");
        assert_eq!(section.title, "工作经历");
        assert_eq!(section.items[0].ref_id.as_deref(), Some("e1"));
        // 页头块在最前
        assert_eq!(page.blocks[0].kind, "fields");
    }

    #[test]
    fn adopting_twice_is_stable() {
        let raw = json!({
            "version": 4,
            "people": [{
                "id": "p1",
                "fields": [{ "id": "f1", "label": "姓名", "value": "张三" }],
                "items": [],
                "messages": [],
                "resumes": [{
                    "id": "r1",
                    "personId": "p1",
                    "title": "简历",
                    "updatedAt": 0,
                    "pages": [{
                        "id": "pg1",
                        "label": "中文",
                        "header": [{ "id": "f2", "label": "姓名", "value": "张三" }],
                        "sections": []
                    }]
                }]
            }],
            "ui": { "accent": "white" }
        });

        let once = adopt(raw.clone()).expect("v4 升级");
        let twice = adopt(serde_json::to_value(&once).unwrap()).expect("再升一次");
        assert_eq!(once.version, VERSION);
        assert_eq!(
            serde_json::to_string(&once).unwrap(),
            serde_json::to_string(&twice).unwrap(),
            "连读两次结果必须一致，否则每次启动都会多插块"
        );
    }
    #[test]
    fn normalize_keeps_page_labels_as_written() {
        // 页名是用户数据：normalize 只补结构，不碰内容。
        // 曾经想在这里把「第 7 页」这类自动名清成空，结果几个页签全叫「未命名页」，已回退。
        let mut page: ResumePage = serde_json::from_value(json!({
            "id": "pg1",
            "label": "第 7 页",
            "blocks": []
        }))
        .unwrap();
        page.normalize();
        assert_eq!(page.label, "第 7 页");
    }
}
