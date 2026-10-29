//! 内容写入：把会话 agent 的编辑意图变成 reducer action。
//!
//! 为什么不让工具直接产出「容器全量替换」的 action：
//! 同一轮里模型常连续改同一个容器（先改标题、再加一条），每次都基于同一份
//! `workspace` 快照算新数组，后一次会把前一次冲掉。`EditState` 就是那个
//! 「本轮内的可写副本」：工具改它，`flush()` 在轮末一次产出**一条** `mutate` 信封。
//!
//! 形状约定见 `docs/CONTRACT.md` 第 1 节与第 3 节。
//!
//! ## 惰性 copy-on-write
//!
//! `EditState` 不复制整份工作区，只复制**被改到的那个容器**（一组字段、一个分区的
//! 条目、整份经历库）。理由很实际：一轮对话改的通常只有两三个容器，而整份工作区
//! 可能有几十页。没被改到的容器在 flush 时根本不出现在 ops 里，前端也就不会重渲染。
//!
//! ## 暂存区存在哪
//!
//! 按契约，`EditState` 属于 `Outbox`：`Outbox { actions, assets, edits: Mutex<EditState> }`。
//! 工具只通过 `ToolCtx.out` 拿到出站口，所以「同一轮的所有工具改同一份暂存区」
//! 这件事天然由 `out.edits` 保证，不需要任何全局变量。
//!
//! 生命周期：`turn::run` 开头 `begin(ctx.out)` 清空，`drain_actions` 末尾
//! `flush_all(ctx.out)` 结算。中途取消 / 报错时不清也安全 —— 下一轮的 `begin`
//! 会把半成品扔掉。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde_json::{json, Map, Value};

use crate::tools::{push_action, Outbox, ToolCtx};

/* ---------- 常量 ---------- */

/// `Block.role` 的全部合法值（契约第 5 节）。
///
/// 校验放在这一层而不是数据层：`set_block_role` 是唯一的写入口，模型现编一个
/// role 名没有任何好处，早点报错它才知道该往这几个里选；而**读旧数据**时
/// 遇到不认识的 role 必须放行，否则一次保存就把人家的简历锁死了。
///
/// 为什么没有 `split`：它要求条目内部恰好是「左栏元素 + 右栏元素」，
/// 结构化生成器产出的条目塞进窄栏只会把公司名挤成三行。它只对手写 parts 有意义，
/// 所以从可写枚举里撤掉；`paper.css` 的 `.split` 类保留给手写片段。
pub const ROLES: &[&str] = &[
    "plain", "tight", "band", "tags", "kv", "table", "grid-2", "grid-3", "cols-2", "cols-3",
    "row",
];

/// `apply_patch` 里允许出现的工具名。只放内容工具：
/// 读工具没有意义（回滚它也不会撤销任何东西），界面工具会把动作直接推给前端、
/// 回滚不掉，混进来只会让「失败即整批回退」变成谎话。
const PATCHABLE: &[&str] = &[
    "set_field",
    "add_field",
    "remove_field",
    "add_entry",
    "update_entry",
    "remove_entry",
    "move_entry",
    "add_section",
    "set_section_title",
    "remove_section",
    "set_block_role",
    "add_page",
    "delete_page",
    "rename_page",
    "add_resume",
    "rename_resume",
    "delete_resume",
    "save_library_item",
    "remove_library_item",
];

const PATCH_LIMIT: usize = 40;

/// 一轮最多回传多少条 op。理论上限远不止，但真到几十条说明模型在乱改，
/// 与其把半成品丢给用户，不如让它下一轮说清要干什么。
const OP_LIMIT: usize = 200;

fn uid(prefix: &str) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    format!("{prefix}-{stamp:x}{seq:x}")
}

/* ---------- 小工具 ---------- */

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn list(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn need_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("缺参数 {key}（要传 id，不是名字）"))
}

fn has(args: &Value, key: &str) -> bool {
    args.get(key).map(|value| !value.is_null()).unwrap_or(false)
}

fn person_of<'a>(ctx: &ToolCtx<'a>) -> Result<&'a Value, String> {
    ctx.workspace
        .get("people")
        .and_then(Value::as_array)
        .and_then(|people| {
            people
                .iter()
                .find(|item| text(item, "id") == ctx.person_id)
        })
        .ok_or_else(|| format!("工作区快照里没有候选人 {}", ctx.person_id))
}

/// 页的定位信息。所有 op 都要带齐三个 id，缺一个前端就不知道该改哪份简历的哪一页。
#[derive(Clone, Debug)]
struct Spot {
    person: String,
    resume: String,
    page: String,
}

type PageRef<'a> = (Spot, usize, &'a Value);

fn pages_of<'a>(ctx: &ToolCtx<'a>) -> Result<Vec<PageRef<'a>>, String> {
    let person = person_of(ctx)?;
    let mut out = Vec::new();
    let resumes = person
        .get("resumes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    for resume in resumes {
        let resume_id = text(resume, "id");
        let pages = resume
            .get("pages")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for (index, page) in pages.iter().enumerate() {
            out.push((
                Spot {
                    person: ctx.person_id.to_string(),
                    resume: resume_id.clone(),
                    page: text(page, "id"),
                },
                index,
                page,
            ));
        }
    }
    Ok(out)
}

fn page_name(index: usize, page: &Value) -> String {
    let label = text(page, "label");
    format!(
        "第 {} 页{}",
        index + 1,
        if label.is_empty() {
            String::new()
        } else {
            format!("「{label}」")
        }
    )
}

/// 一个字段的归属容器。`field` 是全局唯一的，所以工具只收 `fieldId`，
/// 归属由这里扫出来 —— 让模型去记「这个字段在哪个块上」是强人所难，
/// 记错了就是一次无谓的失败往返。
#[derive(Clone, Debug)]
enum Owner {
    Person {
        spot: Spot,
        label: String,
    },
    Item {
        spot: Spot,
        item: String,
        label: String,
    },
    Block {
        spot: Spot,
        block: String,
        label: String,
    },
    Entry {
        spot: Spot,
        entry: String,
        label: String,
    },
    /// 页头：简历最上面那块字段组建不出来时用它。
    ///
    /// 页头 = 这一页的第一个 fields 块（前端 `withHeader` 认这个规则）。
    /// 但新建的页是**零预设**的，一个块都没有 —— 于是模型看到的页头上没有 id，
    /// 也就没有任何工具能把姓名写进简历。这个变体就是那个缺口：按「页 id」认领页头，
    /// 落库时下发一条 `pageHeader`（前端会自己插入字段块）。
    Header {
        spot: Spot,
        label: String,
    },
}

impl Owner {
    fn key(&self) -> String {
        match self {
            Owner::Person { spot, .. } => format!("fields:p:{}", spot.person),
            Owner::Item { spot, .. } => format!("lib:{}", spot.person),
            Owner::Block { spot, block, .. } => {
                format!("fields:b:{}:{}:{}", spot.resume, spot.page, block)
            }
            Owner::Entry { entry, .. } => format!("entry:{entry}"),
            Owner::Header { spot, .. } => format!("fields:h:{}:{}", spot.resume, spot.page),
        }
    }

    fn label(&self) -> &str {
        match self {
            Owner::Person { label, .. }
            | Owner::Item { label, .. }
            | Owner::Block { label, .. }
            | Owner::Entry { label, .. }
            | Owner::Header { label, .. } => label,
        }
    }

    fn spot(&self) -> &Spot {
        match self {
            Owner::Person { spot, .. }
            | Owner::Item { spot, .. }
            | Owner::Block { spot, .. }
            | Owner::Entry { spot, .. }
            | Owner::Header { spot, .. } => spot,
        }
    }
}

/// 一个分区块（条目容器）
#[derive(Clone, Debug)]
struct Section {
    spot: Spot,
    block: String,
    label: String,
}

impl Section {
    fn items_key(&self) -> String {
        format!("items:{}:{}:{}", self.spot.resume, self.spot.page, self.block)
    }
}

/// 待下发的一条 op。容器类的只记「改哪儿」，值在 flush 时从副本里现取 ——
/// 这样同一容器被改十次，最终只发一条，且发的是最终状态。
#[derive(Clone, Debug)]
enum Dirty {
    /// 字段数组：候选人 / 字段块 / 库条目
    Fields(Owner),
    /// 分区条目数组（增删改顺序）
    Items(Section),
    /// 单个条目的措辞或字段（entryPatch）
    Entry { entry: String, section: Section },
    /// 参数已经算好的 op（页、简历、role、分区增删）
    Op(Value),
}

impl Dirty {
    fn key(&self) -> String {
        match self {
            Dirty::Fields(owner) => owner.key(),
            Dirty::Items(section) => section.items_key(),
            Dirty::Entry { entry, .. } => format!("entry:{entry}"),
            Dirty::Op(_) => String::new(),
        }
    }
}

/* ---------- 暂存区 ---------- */

/// 本轮编辑的暂存区。
///
/// `slots` 里只放**被改过**的容器，键名见 `Owner::key` / `Section::items_key`。
/// `Clone` 是给 `apply_patch` 做整批回滚用的。
#[derive(Clone, Default)]
pub struct EditState {
    slots: HashMap<String, Vec<Value>>,
    order: Vec<Dirty>,
    seen: HashSet<String>,
    seq: u64,
    /// 本轮新建的分区：workspace 里还查不到，但同轮的 add_entry 要能找到它
    new_sections: HashMap<String, Section>,
    /// 本轮新建的简历：同上，允许同轮再改它的名字
    new_resumes: HashSet<String>,
    /// 本轮已经删掉的 id：再改它就是模型在自说自话，直接报错比静默丢弃好
    removed: HashSet<String>,
}

impl EditState {
    /// 轮末把改动结算成一条 `mutate` 信封；没有改动返回 None。
    pub fn flush(&mut self) -> Option<Value> {
        if self.order.is_empty() {
            return None;
        }
        let order = std::mem::take(&mut self.order);
        self.seen.clear();
        let mut ops: Vec<Value> = Vec::new();
        for dirty in order {
            if ops.len() >= OP_LIMIT {
                break;
            }
            if let Some(op) = self.op_of(&dirty) {
                ops.push(op);
            }
        }
        // 结算完就清空：下一轮从干净状态开始，也避免快照的旧值被带过去
        self.slots.clear();
        self.new_sections.clear();
        self.new_resumes.clear();
        self.removed.clear();
        if ops.is_empty() {
            return None;
        }
        Some(json!({ "type": "mutate", "ops": ops }))
    }

    fn mark(&mut self, dirty: Dirty) {
        let key = dirty.key();
        if key.is_empty() {
            return;
        }
        // 同一容器只发一条（发最终状态），位置保留第一次触碰时的顺序 ——
        // 顺序即语义：先 addSection 再往这个分区里加条目
        if self.seen.insert(key) {
            self.order.push(dirty);
        }
    }

    fn mark_op(&mut self, op: Value) {
        self.seq += 1;
        // Op 没有容器可以合并，用序号造一个唯一键，保证不会被去重
        self.seen.insert(format!("op:{}", self.seq));
        self.order.push(Dirty::Op(op));
    }

    /// 条目被删掉后，之前排队等它的 entryPatch 必须一起作废，
    /// 否则前端会收到一条「改一个不存在的条目」的指令
    fn forget_entry(&mut self, entry_id: &str) {
        let key = format!("entry:{entry_id}");
        if self.seen.remove(&key) {
            self.order.retain(|dirty| dirty.key() != key);
        }
    }

    fn op_of(&self, dirty: &Dirty) -> Option<Value> {
        match dirty {
            Dirty::Op(op) => Some(op.clone()),
            Dirty::Fields(owner) => {
                let fields = self.slots.get(&owner.key())?.clone();
                let spot = owner.spot();
                Some(match owner {
                    // 纯人级 op 只带 personId（契约 1.2 的表），塞上页码反而让人以为
                    // 这次改动只影响某一页
                    Owner::Person { .. } => json!({
                        "type": "personFields",
                        "personId": spot.person,
                        "fields": fields,
                    }),
                    Owner::Block { block, .. } => json!({
                        "type": "blockFields",
                        "personId": spot.person,
                        "resumeId": spot.resume,
                        "pageId": spot.page,
                        "blockId": block,
                        "fields": fields,
                    }),
                    // 库条目改字段 = 整份经历库替换（前端只有这一个 op）
                    Owner::Item { .. } => json!({
                        "type": "libraryItems",
                        "personId": spot.person,
                        "items": fields,
                    }),
                    // Entry 的字段改动永远走 entryPatch，不会走到这里
                    Owner::Entry { .. } => return None,
                    // 前端 `withHeader` 认「第一个 fields 块」：整组替换，没有就插一个
                    Owner::Header { .. } => json!({
                        "type": "pageHeader",
                        "personId": spot.person,
                        "resumeId": spot.resume,
                        "pageId": spot.page,
                        "header": fields,
                    }),
                })
            }
            Dirty::Items(section) => {
                let items = self.slots.get(&section.items_key())?.clone();
                Some(section_items_op(section, items))
            }
            Dirty::Entry { entry, section } => {
                // 字段和措辞都从「这个分区当前的条目数组」里读：
                // 同一轮里先 add_entry 再 update_entry 也能读到刚加的那条
                let items = self.slots.get(&section.items_key())?;
                let item = items.iter().find(|item| text(item, "id") == *entry)?;
                // 前端 reducer 把 `patch` 直接铺在条目上（Partial<SectionEntry>），
                // 所以字段与措辞必须装进 patch 里，放在顶层会被形状校验丢掉
                Some(json!({
                    "type": "entryPatch",
                    "personId": section.spot.person,
                    "resumeId": section.spot.resume,
                    "pageId": section.spot.page,
                    "entryId": entry,
                    "patch": {
                        "fields": item.get("fields").cloned().unwrap_or_else(|| json!([])),
                        "bullets": item.get("bullets").cloned().unwrap_or_else(|| json!([])),
                    },
                }))
            }
        }
    }
}

fn section_items_op(section: &Section, items: Vec<Value>) -> Value {
    json!({
        "type": "sectionItems",
        "personId": section.spot.person,
        "resumeId": section.spot.resume,
        "pageId": section.spot.page,
        "sectionId": section.block,
        "items": items,
    })
}

/* ---------- 暂存区的挂载点 ---------- */

/// 每轮开始前调用一次：清掉上一轮可能残留的暂存区。
///
/// 为什么需要它：取消、报错、网关 400 都可能让一轮在半路结束，
/// 那时 `flush_all` 没被调用，暂存区里的半成品会活到下一轮 ——
/// 下一轮用户问的是别的事，却会收到上一轮没提交的改动。
pub fn begin(out: &Outbox) {
    let mut state = out.edits.lock().unwrap_or_else(|error| error.into_inner());
    *state = EditState::default();
}

/// 轮末结算：把本轮所有内容改动并成一条 `mutate` 信封。
pub fn flush_all(out: &Outbox) -> Option<Value> {
    flush_state(&out.edits)
}

/// 从暂存区取一条信封（`flush_all` 与测试都走这里）
pub fn flush_state(store: &Mutex<EditState>) -> Option<Value> {
    match store.lock() {
        Ok(mut state) => state.flush(),
        Err(error) => error.into_inner().flush(),
    }
}

/// 工具的统一入口：借出本轮暂存区，跑完自动放锁。
fn with_state<T>(out: &Outbox, run: impl FnOnce(&mut EditState) -> T) -> T {
    // 中毒也要继续：一次断言失败不该让后续所有工具永久报错
    let mut state = out.edits.lock().unwrap_or_else(|error| error.into_inner());
    run(&mut state)
}

/* ---------- 扫描：按 id 找容器 ---------- */

fn field_ids(value: &Value) -> Vec<String> {
    list(value, "fields")
        .iter()
        .map(|field| text(field, "id"))
        .filter(|id| !id.is_empty())
        .collect()
}

/// 报错时把「能用哪些 id」直接摆出来。模型看到一屏 id 会自己挑，
/// 只看到「找不到」就会退化成乱猜。
fn available(ctx: &ToolCtx) -> String {
    let Ok(person) = person_of(ctx) else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    let own = field_ids(person);
    if !own.is_empty() {
        parts.push(format!("候选人字段 {}", own.join(" ")));
    }
    let items: Vec<String> = list(person, "items")
        .iter()
        .map(|item| text(item, "id"))
        .filter(|id| !id.is_empty())
        .collect();
    if !items.is_empty() {
        parts.push(format!("库条目 {}", items.join(" ")));
    }
    if let Ok(pages) = pages_of(ctx) {
        for (_spot, index, page) in pages {
            let name = page_name(index, page);
            let mut blocks: Vec<String> = Vec::new();
            for block in list(page, "blocks") {
                let id = text(&block, "id");
                if id.is_empty() {
                    continue;
                }
                let mut inner: Vec<String> = field_ids(&block);
                for entry in list(&block, "items") {
                    let entry_id = text(&entry, "id");
                    if !entry_id.is_empty() {
                        inner.push(entry_id);
                    }
                }
                blocks.push(if inner.is_empty() {
                    id
                } else {
                    format!("{id}({})", inner.join(" "))
                });
            }
            if !blocks.is_empty() {
                parts.push(format!("{name} {}", blocks.join(" ")));
            }
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("。可用的 id：{}", parts.join("；"))
    }
}

fn find_field_owner(ctx: &ToolCtx, field_id: &str) -> Result<Owner, String> {
    let person = person_of(ctx)?;
    let header = Spot {
        person: ctx.person_id.to_string(),
        resume: ctx.resume_id.to_string(),
        page: ctx.page_id.to_string(),
    };

    if field_ids(person).iter().any(|id| id == field_id) {
        return Ok(Owner::Person {
            spot: header,
            label: "候选人字段".into(),
        });
    }

    for item in list(person, "items") {
        if field_ids(&item).iter().any(|id| id == field_id) {
            let name = text(&item, "id");
            return Ok(Owner::Item {
                spot: header,
                item: name.clone(),
                label: format!("经历库条目 {name}"),
            });
        }
    }

    for (spot, index, page) in pages_of(ctx)? {
        let name = page_name(index, page);
        for block in list(page, "blocks") {
            if field_ids(&block).iter().any(|id| id == field_id) {
                let block_id = text(&block, "id");
                return Ok(Owner::Block {
                    spot,
                    block: block_id.clone(),
                    label: format!("{name} 的字段块 {block_id}"),
                });
            }
            for entry in list(&block, "items") {
                if field_ids(&entry).iter().any(|id| id == field_id) {
                    let entry_id = text(&entry, "id");
                    return Ok(Owner::Entry {
                        spot,
                        entry: entry_id.clone(),
                        label: format!("{name}「{}」的条目 {entry_id}", text(&block, "title")),
                    });
                }
            }
        }
    }

    Err(format!("找不到字段 {field_id}{}", available(ctx)))
}

/// `add_field` 的 ownerId：候选人 / 字段块 / 条目 / 库条目，全靠扫描判定。
/// 刻意不做「pageHeader」这类语义别名 —— 多一个模型要记的词，就多一种记错的方式。
fn find_owner_by_id(ctx: &ToolCtx, owner_id: &str) -> Result<Owner, String> {
    let person = person_of(ctx)?;
    let header = Spot {
        person: ctx.person_id.to_string(),
        resume: ctx.resume_id.to_string(),
        page: ctx.page_id.to_string(),
    };
    if owner_id == ctx.person_id {
        return Ok(Owner::Person {
            spot: header,
            label: "候选人字段".into(),
        });
    }
    for item in list(person, "items") {
        if text(&item, "id") == owner_id {
            return Ok(Owner::Item {
                spot: header,
                item: owner_id.to_string(),
                label: format!("经历库条目 {owner_id}"),
            });
        }
    }
    for (spot, index, page) in pages_of(ctx)? {
        let name = page_name(index, page);
        // 页 id 就是页头的「入口」：新建的页零预设，没有块也就没有块 id ——
        // 不认页 id 的话，模型没有任何办法把姓名写进简历最上面那一行
        if text(page, "id") == owner_id {
            return Ok(Owner::Header {
                spot,
                label: format!("{name}的页头"),
            });
        }
        for block in list(page, "blocks") {
            let block_id = text(&block, "id");
            let is_block = block_id == owner_id;
            let has_entry = list(&block, "items")
                .iter()
                .any(|entry| text(entry, "id") == owner_id);
            if is_block || has_entry {
                if has_entry {
                    return Ok(Owner::Entry {
                        spot,
                        entry: owner_id.to_string(),
                        label: format!(
                            "{name}「{}」的条目 {owner_id}",
                            text(&block, "title")
                        ),
                    });
                }
                let kind = text(&block, "kind");
                if kind != "fields" {
                    return Err(format!(
                        "{owner_id} 是{}块，不能直接加字段。字段只能加到字段块、条目或库条目上",
                        if kind == "section" { "分区" } else { "媒体" }
                    ));
                }
                return Ok(Owner::Block {
                    spot,
                    block: block_id.clone(),
                    label: format!("{name} 的字段块 {block_id}"),
                });
            }
        }
    }
    Err(format!(
        "ownerId {owner_id} 既不是候选人、字段块、条目，也不是库条目{}",
        available(ctx)
    ))
}

/// 找分区：本轮新建的优先（workspace 里还查不到），否则扫快照。
///
/// 少了这一步，「add_section 之后同轮 add_entry」会报「找不到分区」——
/// 而这恰恰是模型最自然会连着做的两件事。
fn resolve_section(st: &EditState, ctx: &ToolCtx, section_id: &str) -> Result<Section, String> {
    if let Some(section) = st.new_sections.get(section_id) {
        return Ok(section.clone());
    }
    find_section(ctx, section_id)
}

fn find_section(ctx: &ToolCtx, section_id: &str) -> Result<Section, String> {
    for (spot, index, page) in pages_of(ctx)? {
        for block in list(page, "blocks") {
            if text(&block, "id") != section_id {
                continue;
            }
            if text(&block, "kind") != "section" {
                return Err(format!(
                    "{section_id} 是{}块，不是分区。分区 id 从 read_page 里标着「分区」的块取",
                    text(&block, "kind")
                ));
            }
            return Ok(Section {
                spot: spot.clone(),
                block: section_id.to_string(),
                label: format!(
                    "{}「{}」",
                    page_name(index, page),
                    text(&block, "title")
                ),
            });
        }
    }
    Err(format!("找不到分区 {section_id}{}", available(ctx)))
}

fn find_block<'a>(ctx: &ToolCtx<'a>, block_id: &str) -> Result<(Spot, Value), String> {
    for (spot, _index, page) in pages_of(ctx)? {
        if let Some(block) = list(page, "blocks")
            .into_iter()
            .find(|block| text(block, "id") == block_id)
        {
            return Ok((spot, block));
        }
    }
    Err(format!("找不到块 {block_id}{}", available(ctx)))
}

/// 条目所在的 section。先看本轮改过的副本，再看 workspace ——
/// 否则「刚 add_entry 出来的条目」在同轮 update_entry 里就找不到了。
fn section_of_entry(st: &EditState, ctx: &ToolCtx, entry_id: &str) -> Result<Section, String> {
    for (spot, index, page) in pages_of(ctx)? {
        for block in list(page, "blocks") {
            if text(&block, "kind") != "section" {
                continue;
            }
            let section = Section {
                spot: spot.clone(),
                block: text(&block, "id"),
                label: format!(
                    "{}「{}」",
                    page_name(index, page),
                    text(&block, "title")
                ),
            };
            let items = match st.slots.get(&section.items_key()) {
                Some(items) => items.clone(),
                None => list(&block, "items"),
            };
            if items.iter().any(|item| text(item, "id") == entry_id) {
                return Ok(section);
            }
        }
    }
    for section in st.new_sections.values() {
        if let Some(items) = st.slots.get(&section.items_key()) {
            if items.iter().any(|item| text(item, "id") == entry_id) {
                return Ok(section.clone());
            }
        }
    }
    Err(format!(
        "找不到条目 {entry_id}（可能这一轮刚被删掉）{}",
        available(ctx)
    ))
}

fn seed_items(st: &EditState, ctx: &ToolCtx, section: &Section) -> Result<Vec<Value>, String> {
    if st.new_sections.contains_key(&section.block) {
        return Ok(Vec::new());
    }
    for (_spot, _index, page) in pages_of(ctx)? {
        if let Some(block) = list(page, "blocks")
            .into_iter()
            .find(|block| text(block, "id") == section.block)
        {
            return Ok(list(&block, "items"));
        }
    }
    Err(format!(
        "分区 {} 已经不在了（可能这一轮刚被删掉）",
        section.block
    ))
}

/// 拿到某个分区的条目数组副本（没有就按 workspace 克隆一份）
fn items_slot<'a>(
    st: &'a mut EditState,
    ctx: &ToolCtx,
    section: &Section,
) -> Result<&'a mut Vec<Value>, String> {
    let key = section.items_key();
    if !st.slots.contains_key(&key) {
        let seed = seed_items(st, ctx, section)?;
        st.slots.insert(key.clone(), seed);
    }
    Ok(st.slots.get_mut(&key).expect("刚插入的容器"))
}

/// 页头字段：这一页第一个 `fields` 块的字段；还没有那个块就是空数组。
///
/// 与前端 `headerOf` 同一条规则 —— 两边各认一套「哪个才叫页头」的时候，
/// 模型写进去的字就会出现在用户看不见的地方。
fn header_fields(page: &Value) -> Vec<Value> {
    for block in list(page, "blocks") {
        if text(&block, "kind") == "fields" {
            return list(&block, "fields");
        }
    }
    Vec::new()
}

fn seed_fields(st: &EditState, ctx: &ToolCtx, owner: &Owner) -> Result<Vec<Value>, String> {
    match owner {
        Owner::Person { .. } => Ok(list(person_of(ctx)?, "fields")),
        Owner::Block { block, .. } => {
            let (_spot, block) = find_block(ctx, block)?;
            Ok(list(&block, "fields"))
        }
        Owner::Header { .. } => {
            let page_id = owner.spot().page.clone();
            let page = pages_of(ctx)?
                .into_iter()
                .find(|(spot, _, _)| spot.page == page_id)
                .map(|(_, _, page)| page.clone())
                .ok_or_else(|| format!("找不到页面 {page_id}"))?;
            Ok(header_fields(&page))
        }
        Owner::Entry { entry, .. } => {
            let section = section_of_entry(st, ctx, entry)?;
            let items = match st.slots.get(&section.items_key()) {
                Some(items) => items.clone(),
                None => seed_items(st, ctx, &section)?,
            };
            let item = items
                .iter()
                .find(|item| text(item, "id") == *entry)
                .ok_or_else(|| format!("找不到条目 {entry}"))?;
            Ok(list(item, "fields"))
        }
        Owner::Item { .. } => Err("库条目字段走整份经历库替换，不该走到这里".into()),
    }
}

/// 拿到字段数组的可写副本。库条目和条目是「容器里的容器」，
/// 所以要穿过 items 数组拿到那一项的 `fields`。
fn fields_mut<'a>(
    st: &'a mut EditState,
    ctx: &ToolCtx,
    owner: &Owner,
) -> Result<&'a mut Vec<Value>, String> {
    match owner {
        Owner::Item { spot, item, .. } => {
            let key = format!("lib:{}", spot.person);
            if !st.slots.contains_key(&key) {
                let seed = list(person_of(ctx)?, "items");
                st.slots.insert(key.clone(), seed);
            }
            let items = st.slots.get_mut(&key).expect("刚插入的容器");
            let index = items
                .iter()
                .position(|entry| text(entry, "id") == *item)
                .ok_or_else(|| format!("经历库里没有 {item}"))?;
            let value = &mut items[index];
            if !value.get("fields").map(Value::is_array).unwrap_or(false) {
                value["fields"] = json!([]);
            }
            Ok(value["fields"].as_array_mut().expect("刚补过数组"))
        }
        Owner::Entry { entry, .. } => {
            // 条目永远活在一个分区的 items 数组里，改它要穿过那一层
            // 这样 add_entry 之后紧接 set_field 也能落在刚建的那条上
            let section = section_of_entry(st, ctx, entry)?;
            let items = items_slot(st, ctx, &section)?;
            let index = items
                .iter()
                .position(|item| text(item, "id") == *entry)
                .ok_or_else(|| format!("找不到条目 {entry}"))?;
            let value = &mut items[index];
            if !value.get("fields").map(Value::is_array).unwrap_or(false) {
                value["fields"] = json!([]);
            }
            Ok(value["fields"].as_array_mut().expect("刚补过数组"))
        }
        _ => {
            let key = owner.key();
            if !st.slots.contains_key(&key) {
                let seed = seed_fields(st, ctx, owner)?;
                st.slots.insert(key.clone(), seed);
            }
            Ok(st.slots.get_mut(&key).expect("刚插入的容器"))
        }
    }
}

/* ---------- 字段值的搬运 ---------- */

fn build_fields(input: &Value) -> Result<Vec<Value>, String> {
    let raw = input
        .as_array()
        .ok_or_else(|| "fields 必须是数组，每项形如 {label, value}".to_string())?;
    let mut out = Vec::new();
    for (index, item) in raw.iter().enumerate() {
        let Some(object) = item.as_object() else {
            return Err(format!(
                "fields 第 {} 项必须是对象，形如 {{\"label\":\"公司\",\"value\":\"中远\"}}",
                index + 1
            ));
        };
        let label = object
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let value = object
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let asset = object
            .get("assetId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if label.is_empty() && value.is_empty() && asset.is_empty() {
            return Err(format!(
                "fields 第 {} 项是空的（label 与 value 都为空），要么填内容，要么去掉这一项",
                index + 1
            ));
        }
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| uid("f"));
        let mut field = Map::new();
        field.insert("id".into(), json!(id));
        field.insert("label".into(), json!(label));
        field.insert("value".into(), json!(value));
        if !asset.is_empty() {
            field.insert("assetId".into(), json!(asset));
        }
        out.push(Value::Object(field));
    }
    Ok(out)
}

fn build_bullets(input: &Value) -> Result<Vec<String>, String> {
    let raw = input
        .as_array()
        .ok_or_else(|| "bullets 必须是字符串数组".to_string())?;
    let mut out = Vec::new();
    for item in raw {
        let line = item
            .as_str()
            .ok_or_else(|| "bullets 里每一项都要是字符串".to_string())?
            .trim();
        if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    Ok(out)
}

/// 字段数组改完之后的「一句话总结」不需要额外函数：
/// 各工具的返回文案里直接写容器名与条数，模型更容易对上号。

/* ---------- 参数（给模型看的形状） ---------- */

pub fn set_field_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "fieldId": { "type": "string", "description": "要改的字段 id（read_person / read_page 里方括号里的那个）" },
            "label": { "type": "string", "description": "新的字段名，不改就别传" },
            "value": { "type": "string", "description": "新的字段值，不改就别传" },
            "assetId": { "type": "string", "description": "把这一项换成图片：传材料 id；传空串表示清空图片位" }
        },
        "required": ["fieldId"],
        "additionalProperties": false
    })
}

pub fn add_field_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ownerId": {
                "type": "string",
                "description": "加到哪儿：候选人 id（资料，不进简历）、页 id（= 页头，会自动建在最前面）、字段块 id、条目 id、库条目 id。read_person / read_page 里都能看到。简历上的姓名/电话要加到页 id 或页头的字段块上"
            },
            "label": { "type": "string", "description": "字段名（如「公司」）" },
            "value": { "type": "string", "description": "字段值" },
            "assetId": { "type": "string", "description": "这一项是图片时传材料 id" },
            "afterFieldId": { "type": "string", "description": "插到这个字段后面；不传就加在末尾" }
        },
        "required": ["ownerId", "label"],
        "additionalProperties": false
    })
}

pub fn remove_field_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "fieldId": { "type": "string", "description": "要删的字段 id" }
        },
        "required": ["fieldId"],
        "additionalProperties": false
    })
}

pub fn add_entry_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "sectionId": { "type": "string", "description": "分区块 id（read_page 里标着「分区」的块的 id）" },
            "fields": {
                "type": "array",
                "description": "这一条的字段，按显示顺序",
                "items": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" },
                        "value": { "type": "string" },
                        "assetId": { "type": "string" }
                    },
                    "required": ["label"]
                }
            },
            "bullets": {
                "type": "array",
                "items": { "type": "string" },
                "description": "这一条要印在纸上的措辞，一行一条"
            },
            "refId": { "type": "string", "description": "引用经历库里的某条；本页现写就不传" }
        },
        "required": ["sectionId", "fields"],
        "additionalProperties": false
    })
}

pub fn update_entry_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "entryId": { "type": "string", "description": "条目 id" },
            "fields": {
                "type": "array",
                "description": "整组替换这个条目的字段（不是增量）",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "label": { "type": "string" },
                        "value": { "type": "string" },
                        "assetId": { "type": "string" }
                    },
                    "required": ["label"]
                }
            },
            "bullets": {
                "type": "array",
                "items": { "type": "string" },
                "description": "整组替换措辞"
            }
        },
        "required": ["entryId"],
        "additionalProperties": false
    })
}

pub fn remove_entry_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "entryId": { "type": "string", "description": "要删的条目 id" }
        },
        "required": ["entryId"],
        "additionalProperties": false
    })
}

pub fn move_entry_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "entryId": { "type": "string", "description": "要挪的条目 id" },
            "to": { "type": "integer", "description": "挪到分区里的第几位，从 0 开始" }
        },
        "required": ["entryId", "to"],
        "additionalProperties": false
    })
}

pub fn add_section_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "分区标题，如「工作经历」" },
            "afterBlockId": { "type": "string", "description": "插到这个块后面；不传就加在页尾" }
        },
        "required": ["title"],
        "additionalProperties": false
    })
}

pub fn set_section_title_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "sectionId": { "type": "string", "description": "分区 id" },
            "title": { "type": "string", "description": "新标题" }
        },
        "required": ["sectionId", "title"],
        "additionalProperties": false
    })
}

pub fn remove_section_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "sectionId": { "type": "string", "description": "要删的分区 id" }
        },
        "required": ["sectionId"],
        "additionalProperties": false
    })
}

pub fn set_block_role_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "blockId": { "type": "string", "description": "分区块或字段块的 id" },
            "role": {
                "type": "string",
                "enum": ROLES,
                "description": "容器形状。tags = 一行标签、kv = 名值两列、table = 表格、grid/cols/split/row = 分栏。不传表示恢复 plain"
            }
        },
        "required": ["blockId"],
        "additionalProperties": false
    })
}

pub fn add_page_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "label": { "type": "string", "description": "页名，如「英文版」。不传就按序号叫「第 N 页」" }
        },
        "additionalProperties": false
    })
}

pub fn delete_page_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pageId": { "type": "string", "description": "要删的页面 id" }
        },
        "required": ["pageId"],
        "additionalProperties": false
    })
}

pub fn rename_page_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pageId": { "type": "string", "description": "页面 id" },
            "label": { "type": "string", "description": "新页名" }
        },
        "required": ["pageId", "label"],
        "additionalProperties": false
    })
}

pub fn add_resume_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "简历名字，如「外贸单证 · 2024」" }
        },
        "required": ["title"],
        "additionalProperties": false
    })
}

pub fn rename_resume_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "resumeId": { "type": "string", "description": "简历 id" },
            "title": { "type": "string", "description": "新名字" }
        },
        "required": ["resumeId", "title"],
        "additionalProperties": false
    })
}

pub fn delete_resume_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "resumeId": { "type": "string", "description": "要删的简历 id" }
        },
        "required": ["resumeId"],
        "additionalProperties": false
    })
}

pub fn save_library_item_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "itemId": { "type": "string", "description": "改已有条目时传 id；新建就省略" },
            "fields": {
                "type": "array",
                "description": "整组字段",
                "items": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" },
                        "value": { "type": "string" },
                        "assetId": { "type": "string" }
                    },
                    "required": ["label"]
                }
            },
            "notes": { "type": "string", "description": "详实记录（只给自己看，不印在纸上）" }
        },
        "required": ["fields"],
        "additionalProperties": false
    })
}

pub fn remove_library_item_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "itemId": { "type": "string", "description": "要删的库条目 id" }
        },
        "required": ["itemId"],
        "additionalProperties": false
    })
}

pub fn apply_patch_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ops": {
                "type": "array",
                "minItems": 1,
                "maxItems": PATCH_LIMIT,
                "description": "按顺序执行的内容工具调用。任何一条失败整批回滚",
                "items": {
                    "type": "object",
                    "properties": {
                        "tool": { "type": "string", "enum": PATCHABLE },
                        "args": { "type": "object", "description": "该工具的参数" }
                    },
                    "required": ["tool", "args"]
                }
            }
        },
        "required": ["ops"],
        "additionalProperties": false
    })
}

/* ---------- 核心实现（apply_patch 复用这里，不重写一遍） ---------- */

fn core_set_field(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let field_id = need_str(args, "fieldId")?;
    let label = args.get("label").and_then(Value::as_str);
    let value = args.get("value").and_then(Value::as_str);
    let asset = args.get("assetId").and_then(Value::as_str);
    if label.is_none() && value.is_none() && asset.is_none() {
        return Err("没有要改的内容：label / value / assetId 至少传一个".into());
    }

    let owner = find_field_owner(ctx, &field_id)?;
    let (before, after, len) = {
        let fields = fields_mut(st, ctx, &owner)?;
        let index = fields
            .iter()
            .position(|field| text(field, "id") == field_id)
            .ok_or_else(|| format!("容器里没有字段 {field_id}"))?;
        let before = fields[index].clone();
        let field = &mut fields[index];
        if let Some(label) = label {
            field["label"] = json!(label.trim());
        }
        if let Some(value) = value {
            field["value"] = json!(value);
        }
        if let Some(asset) = asset {
            let asset = asset.trim();
            if asset.is_empty() {
                // 空串是「清掉这个图片位」，删键而不是写空串：
                // 前端按 `assetId` 有没有值判断这一项是不是图片
                if let Some(object) = field.as_object_mut() {
                    object.remove("assetId");
                }
            } else {
                field["assetId"] = json!(asset);
            }
        }
        (before, fields[index].clone(), fields.len())
    };

    // 条目和库条目的 op 类型不一样，这里统一在 mark_owner 里按归属挑
    mark_owner(st, ctx, &owner);

    let name = {
        let label = text(&after, "label");
        if label.is_empty() {
            field_id.clone()
        } else {
            label
        }
    };
    let now = text(&after, "value");
    let mut out = format!(
        "已改「{name}」：{} → {}（位置：{}，该处共 {len} 项）",
        summarize(&before),
        if now.is_empty() {
            "（空）".to_string()
        } else {
            now
        },
        owner.label(),
    );
    if !library_note(&owner).is_empty() {
        out.push('\n');
        out.push_str(library_note(&owner));
    }
    Ok(out)
}

/// 命中库条目时必须补的一句：否则模型会以为改的是「这一页的这段」，
/// 转头对用户说「第二页的公司名改好了」—— 实际上所有引用这条经历的页都变了。
/// 工具对模型说的每句话最后都会变成它对用户说的话，范围说错就是谎报。
const LIBRARY_EFFECT: &str = "注意：这条内容属于经历库（不是本页私有），改动会影响所有引用它的页；只想改这一页，用 add_entry 在本页新建一条带自有字段的条目。";

fn library_note(owner: &Owner) -> &'static str {
    if matches!(owner, Owner::Item { .. }) {
        LIBRARY_EFFECT
    } else {
        ""
    }
}

fn summarize(field: &Value) -> String {
    let label = text(field, "label");
    let value = text(field, "value");
    let asset = text(field, "assetId");
    if !asset.is_empty() {
        return format!("（图片 {asset}）");
    }
    match (label.is_empty(), value.is_empty()) {
        (_, true) => "（空）".to_string(),
        (true, false) => value,
        (false, false) => format!("{label}：{value}"),
    }
}

fn core_add_field(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let owner_id = need_str(args, "ownerId")?;
    let label = need_str(args, "label")?;
    let value = args.get("value").and_then(Value::as_str).unwrap_or("");
    let asset = args
        .get("assetId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let after = args
        .get("afterFieldId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);

    let owner = find_owner_by_id(ctx, &owner_id)?;
    let len = {
        let fields = fields_mut(st, ctx, &owner)?;
        let mut field = Map::new();
        field.insert("id".into(), json!(uid("f")));
        field.insert("label".into(), json!(label));
        field.insert("value".into(), json!(value));
        if !asset.is_empty() {
            field.insert("assetId".into(), json!(asset));
        }
        let field = Value::Object(field);
        match after {
            Some(after) => {
                let index = fields
                    .iter()
                    .position(|item| text(item, "id") == after)
                    .ok_or_else(|| {
                        format!(
                            "afterFieldId {after} 不在 {} 里。这一处现有字段：{}",
                            owner.label(),
                            field_ids_of(fields).join(" ")
                        )
                    })?;
                fields.insert(index + 1, field);
            }
            None => fields.push(field),
        }
        fields.len()
    };
    mark_owner(st, ctx, &owner);
    let mut out = format!(
        "已加字段「{label}」（位置：{}，该处现有 {len} 项）",
        owner.label()
    );
    if !library_note(&owner).is_empty() {
        out.push('\n');
        out.push_str(library_note(&owner));
    }
    Ok(out)
}

fn field_ids_of(fields: &[Value]) -> Vec<String> {
    fields
        .iter()
        .map(|field| text(field, "id"))
        .filter(|id| !id.is_empty())
        .collect()
}

fn mark_owner(st: &mut EditState, ctx: &ToolCtx, owner: &Owner) {
    match owner {
        Owner::Entry { entry, .. } => {
            if let Ok(section) = section_of_entry(st, ctx, entry) {
                st.mark(Dirty::Entry {
                    entry: entry.clone(),
                    section,
                });
            }
        }
        _ => st.mark(Dirty::Fields(owner.clone())),
    }
}

fn core_remove_field(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let field_id = need_str(args, "fieldId")?;
    let owner = find_field_owner(ctx, &field_id)?;
    let (removed, len) = {
        let fields = fields_mut(st, ctx, &owner)?;
        let index = fields
            .iter()
            .position(|field| text(field, "id") == field_id)
            .ok_or_else(|| format!("容器里没有字段 {field_id}"))?;
        let removed = fields.remove(index);
        (summarize(&removed), fields.len())
    };
    mark_owner(st, ctx, &owner);
    // 这里说「已删除」而不说「待确认」：它产出的是 blockFields / personFields
    // 这类**整组替换**的 op，前端看不出其中少了一项，也就没法把它拦进确认门。
    // 文案必须和真实行为一致，否则模型会对用户说「等你点确认」而实际上早就生效了。
    let mut out = format!(
        "已删除字段 {field_id}（{removed}）。位置：{}，剩下 {len} 项，立即生效",
        owner.label()
    );
    if !library_note(&owner).is_empty() {
        out.push('\n');
        out.push_str(library_note(&owner));
    }
    Ok(out)
}

fn core_add_entry(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let section_id = need_str(args, "sectionId")?;
    let section = resolve_section(st, ctx, &section_id)?;
    if st.removed.contains(&section_id) {
        return Err(format!("分区 {section_id} 这一轮已经删掉了"));
    }
    let mut fields = build_fields(args.get("fields").unwrap_or(&json!([])))?;
    let bullets = match args.get("bullets") {
        Some(value) => build_bullets(value)?,
        None => Vec::new(),
    };
    if fields.is_empty() && bullets.is_empty() {
        return Err("新条目至少要有字段或措辞，否则纸上什么都不会出现".into());
    }
    let ref_id = args
        .get("refId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);

    // 前端靠 refId 找回库条目：id 不存在时纸上会空白，所以先验一下
    if let Some(ref_id) = &ref_id {
        let person = person_of(ctx)?;
        if !list(person, "items")
            .iter()
            .any(|item| text(item, "id") == *ref_id)
        {
            return Err(format!(
                "refId {ref_id} 不是经历库里的条目。不想引用就省略 refId，让这一页现写"
            ));
        }
    }

    let entry_id = uid("se");
    let mut entry = Map::new();
    entry.insert("id".into(), json!(entry_id));
    if let Some(ref_id) = ref_id {
        entry.insert("refId".into(), json!(ref_id));
    }
    // 条目的字段 id 留空也无所谓，但补上好处是 set_field 能直接定位到它
    for field in &mut fields {
        if text(field, "id").is_empty() {
            field["id"] = json!(uid("f"));
        }
    }
    entry.insert("fields".into(), Value::Array(fields));
    entry.insert("bullets".into(), json!(bullets));

    let total = {
        let items = items_slot(st, ctx, &section)?;
        items.push(Value::Object(entry));
        items.len()
    };
    st.mark(Dirty::Items(section.clone()));
    Ok(format!(
        "已加 1 条到{}（id: {entry_id}，该分区现在 {total} 条）",
        section.label
    ))
}

fn core_update_entry(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let entry_id = need_str(args, "entryId")?;
    if st.removed.contains(&entry_id) {
        return Err(format!("条目 {entry_id} 这一轮已经删掉了"));
    }
    let has_fields = has(args, "fields");
    let has_bullets = has(args, "bullets");
    if !has_fields && !has_bullets {
        return Err("没有要改的内容：fields / bullets 至少传一个".into());
    }
    let section = section_of_entry(st, ctx, &entry_id)?;

    let total = {
        let items = items_slot(st, ctx, &section)?;
        let index = items
            .iter()
            .position(|item| text(item, "id") == entry_id)
            .ok_or_else(|| format!("找不到条目 {entry_id}"))?;
        let item = &mut items[index];
        if has_fields {
            let fields = build_fields(args.get("fields").unwrap_or(&json!([])))?;
            item["fields"] = Value::Array(fields);
        }
        if has_bullets {
            let bullets = build_bullets(args.get("bullets").unwrap_or(&json!([])))?;
            item["bullets"] = json!(bullets);
        }
        items.len()
    };

    st.mark(Dirty::Entry {
        entry: entry_id.clone(),
        section: section.clone(),
    });
    Ok(format!(
        "已改条目 {entry_id}（{}，该分区共 {total} 条）",
        section.label
    ))
}

fn core_remove_entry(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let entry_id = need_str(args, "entryId")?;
    let section = section_of_entry(st, ctx, &entry_id)?;
    let total = {
        let items = items_slot(st, ctx, &section)?;
        let index = items
            .iter()
            .position(|item| text(item, "id") == entry_id)
            .ok_or_else(|| format!("找不到条目 {entry_id}"))?;
        items.remove(index);
        items.len()
    };
    st.forget_entry(&entry_id);
    st.mark(Dirty::Items(section.clone()));
    st.removed.insert(entry_id.clone());
    // 同样说「已删除」：条目删除走的是 sectionItems（整组替换），
    // op 上和「改了顺序 / 改了某条」长得一样，前端无法据此判断「有东西被删」。
    // 想让它进确认门就得新增 entryRemove 类型 —— 为一句文案加一种 reducer 类型不划算。
    Ok(format!(
        "已删除条目 {entry_id}（{}，该分区还剩 {total} 条），立即生效",
        section.label
    ))
}

fn core_move_entry(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let entry_id = need_str(args, "entryId")?;
    let to = args
        .get("to")
        .and_then(Value::as_u64)
        .ok_or_else(|| "to 必须是整数（0 起的位置）".to_string())? as usize;
    let section = section_of_entry(st, ctx, &entry_id)?;
    let total = {
        let items = items_slot(st, ctx, &section)?;
        let from = items
            .iter()
            .position(|item| text(item, "id") == entry_id)
            .ok_or_else(|| format!("找不到条目 {entry_id}"))?;
        if to >= items.len() {
            return Err(format!(
                "to = {to} 超范围：{}现在 {} 条，可用位置 0–{}",
                section.label,
                items.len(),
                items.len() as i64 - 1
            ));
        }
        let value = items.remove(from);
        items.insert(to, value);
        items.len()
    };
    st.mark(Dirty::Items(section.clone()));
    Ok(format!(
        "已把 {entry_id} 挪到{}第 {} 位（共 {total} 条）",
        section.label,
        to + 1
    ))
}

fn core_add_section(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let title = need_str(args, "title")?;
    let after = args
        .get("afterBlockId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);

    // 只改当前页：分区是页里的块，跨页加会让「你刚加的那条去哪了」变成谜
    let page = pages_of(ctx)?
        .into_iter()
        .find(|(spot, _, _)| spot.resume == ctx.resume_id && spot.page == ctx.page_id)
        .ok_or_else(|| format!("找不到当前页 {}", ctx.page_id))?;
    let blocks = list(page.2, "blocks");
    if let Some(after) = &after {
        if !blocks.iter().any(|block| text(block, "id") == *after) {
            return Err(format!(
                "afterBlockId {after} 不是这一页的块。这一页有：{}",
                blocks
                    .iter()
                    .map(|block| text(block, "id"))
                    .filter(|id| !id.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
    }

    let section_id = uid("sc");
    let spot = Spot {
        person: ctx.person_id.to_string(),
        resume: ctx.resume_id.to_string(),
        page: ctx.page_id.to_string(),
    };
    let section = Section {
        spot: spot.clone(),
        block: section_id.clone(),
        label: format!("第 {} 页「{title}」", page.1 + 1),
    };
    st.new_sections.insert(section_id.clone(), section.clone());
    // 新分区的条目容器现在就建好：同轮 add_entry 直接往里写
    st.slots.insert(section.items_key(), Vec::new());

    // `id` 与 `afterBlockId` 是这一层提出、正在和前端对齐的两个字段：
    // 有了预生成 id，同一轮里才能接着往这个新分区加条目；
    // 没有它，前端会自己生成一个 id，后端算出来的那个就永远对不上。
    let mut op = json!({
        "type": "addSection",
        "personId": spot.person,
        "resumeId": spot.resume,
        "pageId": spot.page,
        "id": section_id,
        "title": title,
    });
    if let Some(after) = after {
        op["afterBlockId"] = json!(after);
    }
    st.mark_op(op);
    Ok(format!(
        "已新建分区「{title}」（id: {section_id}），位置：第 {} 页，现在是空的",
        page.1 + 1
    ))
}

fn core_set_section_title(
    st: &mut EditState,
    args: &Value,
    ctx: &ToolCtx,
) -> Result<String, String> {
    let section_id = need_str(args, "sectionId")?;
    let title = need_str(args, "title")?;
    let section = resolve_section(st, ctx, &section_id)?;
    if st.removed.contains(&section_id) {
        return Err(format!("分区 {section_id} 这一轮已经删掉了"));
    }
    st.mark_op(json!({
        "type": "sectionTitle",
        "personId": section.spot.person,
        "resumeId": section.spot.resume,
        "pageId": section.spot.page,
        "sectionId": section_id,
        "title": title,
    }));
    Ok(format!(
        "已把{}的标题改成「{title}」",
        section.label
    ))
}

fn core_remove_section(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let section_id = need_str(args, "sectionId")?;
    let section = resolve_section(st, ctx, &section_id)?;
    let count = st
        .slots
        .get(&section.items_key())
        .map(|items| items.len())
        .unwrap_or_else(|| {
            // 没被改过：数一下 workspace 里的条目
            pages_of(ctx)
                .ok()
                .and_then(|pages| {
                    pages.into_iter().find_map(|(_, _, page)| {
                        list(page, "blocks")
                            .into_iter()
                            .find(|block| text(block, "id") == section_id)
                            .map(|block| list(&block, "items").len())
                    })
                })
                .unwrap_or(0)
        });
    st.removed.insert(section_id.clone());
    st.new_sections.remove(&section_id);
    st.mark_op(json!({
        "type": "removeSection",
        "personId": section.spot.person,
        "resumeId": section.spot.resume,
        "pageId": section.spot.page,
        "sectionId": section_id,
    }));
    Ok(format!(
        "已提交删除{}（含 {count} 条）。界面会向用户确认",
        section.label
    ))
}

fn core_set_block_role(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let block_id = need_str(args, "blockId")?;
    let role = args
        .get("role")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(str::to_string);
    if let Some(role) = &role {
        if !ROLES.contains(&role.as_str()) {
            return Err(format!(
                "role「{role}」不认识。可用的有：{}。想恢复常规排法就传 null 或不传",
                ROLES.join(" / ")
            ));
        }
    }

    let (spot, block) = find_block(ctx, &block_id)?;
    let kind = text(&block, "kind");
    if kind != "section" && kind != "fields" {
        return Err(format!(
            "{block_id} 是{kind}块。role 只对分区块与字段块生效"
        ));
    }

    let mut op = json!({
        "type": "blockRole",
        "personId": spot.person,
        "resumeId": spot.resume,
        "pageId": spot.page,
        "blockId": block_id,
    });
    // 清掉 = 省略 role（前端把「没传」当清掉；传 null 会被形状校验当未知值丢掉）
    if let Some(role) = &role {
        op["role"] = json!(role);
    }
    st.mark_op(op);

    let name = if kind == "section" {
        format!("「{}」", text(&block, "title"))
    } else {
        "页头字段组".to_string()
    };
    Ok(match &role {
        Some(role) => format!("已把块 {block_id}（{name}）改成 {role} 排法"),
        None => format!("已把块 {block_id}（{name}）恢复成常规排法（plain）"),
    })
}

fn current_resume<'a>(ctx: &ToolCtx<'a>) -> Result<&'a Value, String> {
    person_of(ctx)?
        .get("resumes")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter()
                .find(|item| text(item, "id") == ctx.resume_id)
        })
        .ok_or_else(|| format!("找不到简历 {}", ctx.resume_id))
}

fn core_add_page(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let resume = current_resume(ctx)?;
    let pages = list(resume, "pages");
    let label = args
        .get("label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("第 {} 页", pages.len() + 1));
    let page_id = uid("pg");
    // 键名是 `id` 不是 `pageId`：reducer 用 `action.id ?? uid()` 定页 id，
    // 传成 pageId 的话它会另生成一个，本轮后续指向这一页的 op 就全落空了
    st.mark_op(json!({
        "type": "addPage",
        "personId": ctx.person_id,
        "resumeId": ctx.resume_id,
        "id": page_id,
        "label": label,
        // 不抢焦点：新建页不等于用户想切过去，切页是 open_page 的事
        "open": false,
    }));
    Ok(format!(
        "已新建「{label}」（id: {page_id}），这一页现在是空的。\
         要往这一页里加内容得先 open_page 切过去，add_section 只改当前页"
    ))
}

fn core_delete_page(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let page_id = need_str(args, "pageId")?;
    let resume = current_resume(ctx)?;
    let pages = list(resume, "pages");
    let index = pages.iter().position(|page| text(page, "id") == page_id).ok_or_else(|| {
        let names = pages
            .iter()
            .enumerate()
            .map(|(index, page)| {
                format!("第 {} 页「{}」({})", index + 1, text(page, "label"), text(page, "id"))
            })
            .collect::<Vec<_>>()
            .join(" ");
        format!("这份简历没有页面 {page_id}。现有：{names}")
    })?;
    let label = text(&pages[index], "label");
    let blocks = list(&pages[index], "blocks").len();

    // 删的是用户正看着的那一页：必须先把他挪到邻页。
    // 顺序上没问题 —— 信封是轮末才追加进 actions 的，openPage 一定排在它前面。
    if page_id == ctx.page_id {
        let neighbor = pages
            .get(index + 1)
            .or_else(|| {
                if index == 0 {
                    None
                } else {
                    pages.get(index - 1)
                }
            })
            .map(|page| text(page, "id"))
            .filter(|id| !id.is_empty());
        if let Some(neighbor) = neighbor {
            push_action(ctx, json!({ "type": "openPage", "pageId": neighbor }))?;
        }
    }

    st.mark_op(json!({
        "type": "removePage",
        "personId": ctx.person_id,
        "resumeId": ctx.resume_id,
        "pageId": page_id,
    }));
    Ok(format!(
        "已提交删除第 {} 页「{label}」（含 {blocks} 个块）。界面会向用户确认",
        index + 1
    ))
}

fn core_rename_page(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let page_id = need_str(args, "pageId")?;
    let label = need_str(args, "label")?;
    let resume = current_resume(ctx)?;
    let pages = list(resume, "pages");
    let old = pages
        .iter()
        .find(|page| text(page, "id") == page_id)
        .map(|page| text(page, "label"))
        .ok_or_else(|| format!("这份简历没有页面 {page_id}"))?;
    st.mark_op(json!({
        "type": "pageLabel",
        "personId": ctx.person_id,
        "resumeId": ctx.resume_id,
        "pageId": page_id,
        "label": label,
    }));
    Ok(format!(
        "已把页名「{}」改成「{label}」",
        if old.is_empty() { page_id.as_str() } else { old.as_str() }
    ))
}

fn core_add_resume(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let title = need_str(args, "title")?;
    let resume_id = uid("r");
    st.new_resumes.insert(resume_id.clone());
    st.mark_op(json!({
        "type": "createResume",
        "personId": ctx.person_id,
        "resumeId": resume_id,
        "title": title,
    }));
    Ok(format!(
        "已新建简历《{title}》（id: {resume_id}）。新简历还没有页面，加页用 add_page"
    ))
}

fn resume_exists(st: &EditState, ctx: &ToolCtx, resume_id: &str) -> bool {
    st.new_resumes.contains(resume_id)
        || person_of(ctx)
            .map(|person| {
                list(person, "resumes")
                    .iter()
                    .any(|resume| text(resume, "id") == resume_id)
            })
            .unwrap_or(false)
}

fn core_rename_resume(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let resume_id = need_str(args, "resumeId")?;
    let title = need_str(args, "title")?;
    if !resume_exists(st, ctx, &resume_id) {
        return Err(format!("没有简历 {resume_id}{}", available(ctx)));
    }
    st.mark_op(json!({
        "type": "renameResume",
        "personId": ctx.person_id,
        "resumeId": resume_id,
        "title": title,
    }));
    Ok(format!("已把简历名改成《{title}》"))
}

fn core_delete_resume(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let resume_id = need_str(args, "resumeId")?;
    if !resume_exists(st, ctx, &resume_id) {
        return Err(format!("没有简历 {resume_id}{}", available(ctx)));
    }
    st.mark_op(json!({
        "type": "deleteResume",
        "personId": ctx.person_id,
        "resumeId": resume_id,
    }));
    Ok(format!(
        "已提交删除简历 {resume_id}。界面会向用户确认（删掉后这份简历的页面内容一起消失）"
    ))
}

fn lib_slot<'a>(st: &'a mut EditState, ctx: &ToolCtx) -> Result<&'a mut Vec<Value>, String> {
    let key = format!("lib:{}", ctx.person_id);
    if !st.slots.contains_key(&key) {
        let seed = list(person_of(ctx)?, "items");
        st.slots.insert(key.clone(), seed);
    }
    Ok(st.slots.get_mut(&key).expect("刚插入的容器"))
}

fn core_save_library_item(
    st: &mut EditState,
    args: &Value,
    ctx: &ToolCtx,
) -> Result<String, String> {
    let fields = build_fields(args.get("fields").unwrap_or(&json!([])))?;
    let notes = args.get("notes").and_then(Value::as_str);
    let item_id = args
        .get("itemId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);

    let (id, total, created) = {
        let items = lib_slot(st, ctx)?;
        match &item_id {
            Some(id) => {
                let index = items
                    .iter()
                    .position(|item| text(item, "id") == *id)
                    .ok_or_else(|| format!("经历库里没有条目 {id}{}", available(ctx)))?;
                let item = &mut items[index];
                item["fields"] = Value::Array(fields);
                if let Some(notes) = notes {
                    item["notes"] = json!(notes);
                }
                (id.clone(), items.len(), false)
            }
            None => {
                let id = uid("it");
                let mut item = Map::new();
                item.insert("id".into(), json!(id));
                item.insert("fields".into(), Value::Array(fields));
                item.insert("assets".into(), json!([]));
                item.insert("notes".into(), json!(notes.unwrap_or("")));
                items.push(Value::Object(item));
                (id.clone(), items.len(), true)
            }
        }
    };

    st.mark(Dirty::Fields(Owner::Item {
        spot: Spot {
            person: ctx.person_id.to_string(),
            resume: ctx.resume_id.to_string(),
            page: ctx.page_id.to_string(),
        },
        item: id.clone(),
        label: format!("经历库条目 {id}"),
    }));

    Ok(if created {
        format!("已新建经历库条目 {id}（经历库共 {total} 条）")
    } else {
        format!("已更新经历库条目 {id}（经历库共 {total} 条）")
    })
}

fn core_remove_library_item(
    st: &mut EditState,
    args: &Value,
    ctx: &ToolCtx,
) -> Result<String, String> {
    let item_id = need_str(args, "itemId")?;
    let total = {
        let items = lib_slot(st, ctx)?;
        let index = items
            .iter()
            .position(|item| text(item, "id") == item_id)
            .ok_or_else(|| format!("经历库里没有条目 {item_id}{}", available(ctx)))?;
        items.remove(index);
        items.len()
    };
    st.mark(Dirty::Fields(Owner::Item {
        spot: Spot {
            person: ctx.person_id.to_string(),
            resume: ctx.resume_id.to_string(),
            page: ctx.page_id.to_string(),
        },
        item: item_id.clone(),
        label: "经历库".into(),
    }));
    // 整份 libraryItems 替换，op 上看不出「少了一条」，因此不在确认门里
    Ok(format!(
        "已删除库条目 {item_id}（经历库还剩 {total} 条），立即生效"
    ))
}

fn core_apply_patch(st: &mut EditState, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let raw = args
        .get("ops")
        .and_then(Value::as_array)
        .ok_or_else(|| "ops 必须是数组，每项形如 {tool, args}".to_string())?;
    if raw.is_empty() {
        return Err("ops 是空的".into());
    }
    if raw.len() > PATCH_LIMIT {
        return Err(format!(
            "一次最多 {PATCH_LIMIT} 条，这次给了 {}。拆成几轮，用户也好看清改了什么",
            raw.len()
        ));
    }

    // 快照 = 整批回滚的凭据。EditState 是纯数据，clone 一次很便宜
    let snapshot = st.clone();
    let actions_before = ctx
        .out
        .actions
        .lock()
        .map(|guard| guard.len())
        .unwrap_or(0);
    let mut done: Vec<String> = Vec::new();

    for (index, item) in raw.iter().enumerate() {
        let tool = item
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let tool_args = item.get("args").cloned().unwrap_or_else(|| json!({}));
        let failed = |reason: String| -> String {
            format!(
                "第 {} 条（{}）失败：{reason}。整批已回滚，前面 {} 条都没生效",
                index + 1,
                if tool.is_empty() { "缺 tool" } else { &tool },
                index
            )
        };

        if !PATCHABLE.contains(&tool.as_str()) {
            *st = snapshot.clone();
            return Err(failed(format!(
                "{tool} 不是内容工具。apply_patch 只能放：{}",
                PATCHABLE.join(" / ")
            )));
        }
        match run_one(st, &tool, &tool_args, ctx) {
            Ok(text) => done.push(text),
            Err(error) => {
                *st = snapshot.clone();
                // 界面指令也要退回去：delete_page 会顺手推一条 openPage
                if let Ok(mut guard) = ctx.out.actions.lock() {
                    guard.truncate(actions_before);
                }
                return Err(failed(error));
            }
        }
    }

    Ok(format!(
        "已提交 {} 项改动（按顺序执行）：\n{}",
        done.len(),
        done.iter()
            .enumerate()
            .map(|(index, line)| format!("{}. {line}", index + 1))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

/// apply_patch 的分发。只认内容工具，不认自己（防止无限递归）。
fn run_one(
    st: &mut EditState,
    tool: &str,
    args: &Value,
    ctx: &ToolCtx,
) -> Result<String, String> {
    match tool {
        "set_field" => core_set_field(st, args, ctx),
        "add_field" => core_add_field(st, args, ctx),
        "remove_field" => core_remove_field(st, args, ctx),
        "add_entry" => core_add_entry(st, args, ctx),
        "update_entry" => core_update_entry(st, args, ctx),
        "remove_entry" => core_remove_entry(st, args, ctx),
        "move_entry" => core_move_entry(st, args, ctx),
        "add_section" => core_add_section(st, args, ctx),
        "set_section_title" => core_set_section_title(st, args, ctx),
        "remove_section" => core_remove_section(st, args, ctx),
        "set_block_role" => core_set_block_role(st, args, ctx),
        "add_page" => core_add_page(st, args, ctx),
        "delete_page" => core_delete_page(st, args, ctx),
        "rename_page" => core_rename_page(st, args, ctx),
        "add_resume" => core_add_resume(st, args, ctx),
        "rename_resume" => core_rename_resume(st, args, ctx),
        "delete_resume" => core_delete_resume(st, args, ctx),
        "save_library_item" => core_save_library_item(st, args, ctx),
        "remove_library_item" => core_remove_library_item(st, args, ctx),
        other => Err(format!("apply_patch 里不能放 {other}")),
    }
}

/* ---------- 工具入口 ---------- */

macro_rules! tool {
    ($name:ident, $core:ident) => {
        pub fn $name(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
            with_state(ctx.out, |state| $core(state, args, ctx))
        }
    };
}

tool!(set_field, core_set_field);
tool!(add_field, core_add_field);
tool!(remove_field, core_remove_field);
tool!(add_entry, core_add_entry);
tool!(update_entry, core_update_entry);
tool!(remove_entry, core_remove_entry);
tool!(move_entry, core_move_entry);
tool!(add_section, core_add_section);
tool!(set_section_title, core_set_section_title);
tool!(remove_section, core_remove_section);
tool!(set_block_role, core_set_block_role);
tool!(add_page, core_add_page);
tool!(delete_page, core_delete_page);
tool!(rename_page, core_rename_page);
tool!(add_resume, core_add_resume);
tool!(rename_resume, core_rename_resume);
tool!(delete_resume, core_delete_resume);
tool!(save_library_item, core_save_library_item);
tool!(remove_library_item, core_remove_library_item);
tool!(apply_patch, core_apply_patch);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn workspace() -> Value {
        json!({
            "version": 5,
            "people": [{
                "id": "p1",
                "fields": [
                    { "id": "f1", "label": "姓名", "value": "张三" },
                    { "id": "f2", "label": "职位", "value": "单证" }
                ],
                "assets": [
                    { "id": "a1", "kind": "image", "name": "证件照.jpg", "stored": "a.jpg" }
                ],
                "items": [
                    { "id": "it1", "fields": [{ "id": "f10", "label": "公司", "value": "中远" }], "assets": [], "notes": "记录" },
                    { "id": "it2", "fields": [{ "id": "f11", "label": "项目", "value": "系统" }], "assets": [], "notes": "" }
                ],
                "resumes": [
                    {
                        "id": "r1",
                        "personId": "p1",
                        "title": "中文简历",
                        "pages": [
                            {
                                "id": "pg1",
                                "kind": "native",
                                "label": "通用版",
                                "blocks": [
                                    { "id": "b1", "kind": "fields", "fields": [{ "id": "f3", "label": "城市", "value": "上海" }] },
                                    {
                                        "id": "s1",
                                        "kind": "section",
                                        "title": "工作经历",
                                        "items": [
                                            { "id": "e1", "refId": "it1", "fields": [], "bullets": ["日处理 240 票"] },
                                            { "id": "e2", "fields": [{ "id": "f4", "label": "公司", "value": "某公司" }], "bullets": ["做了一件事"] }
                                        ]
                                    },
                                    { "id": "m1", "kind": "media", "assetId": "a1", "caption": "照片" }
                                ]
                            },
                            { "id": "pg2", "kind": "native", "label": "英文版", "blocks": [] }
                        ]
                    },
                    { "id": "r2", "personId": "p1", "title": "旧版", "pages": [{ "id": "pg9", "kind": "native", "label": "旧", "blocks": [] }] }
                ]
            }],
            "ui": {}
        })
    }

    struct Harness {
        ws: Value,
        out: Outbox,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                ws: workspace(),
                out: Outbox::default(),
            }
        }

        fn ctx(&self) -> ToolCtx<'_> {
            ToolCtx {
                workspace: &self.ws,
                person_id: "p1",
                resume_id: "r1",
                page_id: "pg1",
                out: &self.out,
                root: None,
                facts: None,
            }
        }

        fn run(&self, tool: &str, args: Value) -> Result<String, String> {
            let ctx = self.ctx();
            if tool == "apply_patch" {
                return with_state(&self.out, |state| core_apply_patch(state, &args, &ctx));
            }
            with_state(&self.out, |state| run_one(state, tool, &args, &ctx))
        }

        fn flush(&self) -> Option<Value> {
            flush_all(&self.out)
        }

        fn envelope(&self) -> Value {
            flush_all(&self.out).expect("应当有改动要提交")
        }

        fn ops(&self) -> Vec<Value> {
            self.envelope()["ops"].as_array().cloned().unwrap_or_default()
        }

        fn actions(&self) -> Vec<Value> {
            self.out.actions.lock().unwrap().clone()
        }
    }

    #[test]
    fn flush_without_changes_is_none() {
        let h = Harness::new();
        assert!(h.flush().is_none());
    }

    #[test]
    fn set_field_locates_every_kind_of_container() {
        let h = Harness::new();
        h.run("set_field", json!({ "fieldId": "f1", "value": "李四" })).unwrap();
        h.run("set_field", json!({ "fieldId": "f3", "value": "北京" })).unwrap();
        h.run("set_field", json!({ "fieldId": "f10", "value": "中远海运" })).unwrap();
        h.run("set_field", json!({ "fieldId": "f4", "value": "新公司" })).unwrap();

        let ops = h.ops();
        let kinds: Vec<&str> = ops.iter().map(|op| op["type"].as_str().unwrap()).collect();
        assert_eq!(
            kinds,
            vec!["personFields", "blockFields", "libraryItems", "entryPatch"]
        );
        assert_eq!(ops[0]["fields"][0]["value"], json!("李四"));
        assert_eq!(ops[1]["blockId"], json!("b1"));
        assert_eq!(ops[2]["items"][0]["fields"][0]["value"], json!("中远海运"));
        assert_eq!(ops[3]["entryId"], json!("e2"));
        assert_eq!(ops[3]["patch"]["fields"][0]["value"], json!("新公司"));
        // 条目的措辞不能因为改字段而丢
        assert_eq!(ops[3]["patch"]["bullets"][0], json!("做了一件事"));
    }

    #[test]
    fn set_field_reports_available_ids_when_it_cannot_find_one() {
        let h = Harness::new();
        let err = h
            .run("set_field", json!({ "fieldId": "f-nope", "value": "x" }))
            .unwrap_err();
        assert!(err.contains("找不到字段 f-nope"), "{err}");
        // 只报「找不到」会让模型下一轮瞎猜，必须把可用的 id 摆出来
        assert!(err.contains("可用的 id"), "{err}");
        assert!(err.contains("f1"), "{err}");
        assert!(err.contains("s1"), "{err}");

        let err = h.run("set_field", json!({ "fieldId": "f1" })).unwrap_err();
        assert!(err.contains("至少传一个"), "{err}");
    }

    /// 改到库条目会波及所有引用它的页：必须把后果说给模型，
    /// 否则它会向用户谎报范围（「只改了第二页」）
    #[test]
    fn editing_a_library_field_warns_about_the_blast_radius() {
        let h = Harness::new();
        let text = h.run("set_field", json!({ "fieldId": "f10", "value": "中远海运" })).unwrap();
        assert!(text.contains("属于经历库"), "{text}");
        assert!(text.contains("所有引用它的页"), "{text}");

        let text = h.run("set_field", json!({ "fieldId": "f3", "value": "北京" })).unwrap();
        assert!(!text.contains("经历库"), "本页自有字段不该报警告：{text}");

        let text = h.run("remove_field", json!({ "fieldId": "f11" })).unwrap();
        assert!(text.contains("属于经历库"), "{text}");
    }

    /// 同一轮改两次同一容器 → 只发一条 op，发的是最终值
    #[test]
    fn edits_in_one_turn_collapse_into_one_op() {
        let h = Harness::new();
        h.run("set_field", json!({ "fieldId": "f1", "assetId": "a1" })).unwrap();
        h.run("add_field", json!({ "ownerId": "p1", "label": "邮箱", "value": "a@b.c" }))
            .unwrap();
        h.run("set_field", json!({ "fieldId": "f1", "assetId": "" })).unwrap();

        let ops = h.ops();
        assert_eq!(ops.len(), 1, "同一个容器只该发一条：{ops:?}");
        let fields = ops[0]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 3);
        // 空串表示清空图片位：键要真的消失，前端才不会把它当图片渲染
        assert!(fields[0].get("assetId").is_none(), "{}", fields[0]);
        assert_eq!(fields[2]["label"], json!("邮箱"));
    }

    #[test]
    fn add_field_inserts_after_the_named_field() {
        let h = Harness::new();
        let text = h
            .run(
                "add_field",
                json!({ "ownerId": "b1", "label": "邮箱", "value": "a@b.c", "afterFieldId": "f3" }),
            )
            .unwrap();
        assert!(text.contains("该处现有 2 项"), "{text}");

        let ops = h.ops();
        assert_eq!(ops[0]["type"], json!("blockFields"));
        let fields = ops[0]["fields"].as_array().unwrap();
        assert_eq!(fields[1]["label"], json!("邮箱"));

        // afterFieldId 不在这个容器里：要报错，不能默默加到末尾
        let err = h
            .run("add_field", json!({ "ownerId": "b1", "label": "x", "afterFieldId": "f1" }))
            .unwrap_err();
        assert!(err.contains("不在"), "{err}");
        assert!(err.contains("f3"), "{err}");
    }

    #[test]
    fn add_field_refuses_section_blocks_and_unknown_owners() {
        let h = Harness::new();
        let err = h
            .run("add_field", json!({ "ownerId": "s1", "label": "x" }))
            .unwrap_err();
        assert!(err.contains("分区块"), "{err}");

        let err = h
            .run("add_field", json!({ "ownerId": "nope", "label": "x" }))
            .unwrap_err();
        assert!(err.contains("既不是候选人"), "{err}");
    }

    /// 新建的页是零预设的：没有块，也就没有块 id 可给。
    /// 页 id 必须能当页头的入口，否则模型没有任何办法把姓名写进简历最上面那一行。
    #[test]
    fn add_field_with_a_page_id_writes_the_page_header() {
        let h = Harness::new();
        let text = h
            .run("add_field", json!({ "ownerId": "pg2", "label": "姓名", "value": "李四" }))
            .unwrap();
        assert!(text.contains("页头"), "{text}");

        let ops = h.ops();
        assert_eq!(ops[0]["type"], json!("pageHeader"));
        assert_eq!(ops[0]["pageId"], json!("pg2"));
        let header = ops[0]["header"].as_array().unwrap();
        assert_eq!(header.len(), 1, "{header:?}");
        assert_eq!(header[0]["label"], json!("姓名"));
    }

    /// 已经有页头字段块时，页 id 落到同一份数据上 —— 不能长出第二个页头
    #[test]
    fn page_id_targets_the_existing_header_block() {
        let h = Harness::new();
        h.run("add_field", json!({ "ownerId": "pg1", "label": "邮箱", "value": "a@b.c" }))
            .unwrap();

        let ops = h.ops();
        assert_eq!(ops[0]["type"], json!("pageHeader"));
        let header = ops[0]["header"].as_array().unwrap();
        assert_eq!(header.len(), 2, "原有字段要保留：{header:?}");
        assert_eq!(header[0]["label"], json!("城市"));
        assert_eq!(header[1]["label"], json!("邮箱"));
    }

    #[test]
    fn remove_field_reports_what_is_left() {
        let h = Harness::new();
        let text = h.run("remove_field", json!({ "fieldId": "f3" })).unwrap();
        assert!(text.contains("剩下 0 项"), "{text}");
        // 这条不进确认门（op 是整组替换），文案必须说「已删除、立即生效」
        assert!(text.contains("立即生效"), "{text}");
        assert!(!text.contains("确认"), "{text}");
        let ops = h.ops();
        assert_eq!(ops[0]["fields"], json!([]));

        let err = h.run("remove_field", json!({ "fieldId": "nope" })).unwrap_err();
        assert!(err.contains("找不到字段"), "{err}");
    }

    #[test]
    fn add_entry_appends_to_the_section() {
        let h = Harness::new();
        let text = h
            .run(
                "add_entry",
                json!({
                    "sectionId": "s1",
                    "fields": [{ "label": "公司", "value": "新公司" }],
                    "bullets": ["做了 A", "  ", "做了 B"]
                }),
            )
            .unwrap();
        assert!(text.contains("该分区现在 3 条"), "{text}");

        let ops = h.ops();
        assert_eq!(ops[0]["type"], json!("sectionItems"));
        let items = ops[0]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[2]["fields"][0]["value"], json!("新公司"));
        // 空行不该占一条措辞
        assert_eq!(items[2]["bullets"], json!(["做了 A", "做了 B"]));
        assert!(items[2]["id"].as_str().unwrap().starts_with("se-"));
    }

    #[test]
    fn add_entry_rejects_bad_targets() {
        let h = Harness::new();
        let err = h
            .run("add_entry", json!({ "sectionId": "b1", "fields": [{ "label": "x" }] }))
            .unwrap_err();
        assert!(err.contains("不是分区"), "{err}");

        let err = h
            .run(
                "add_entry",
                json!({ "sectionId": "s1", "fields": [{ "label": "x" }], "refId": "nope" }),
            )
            .unwrap_err();
        assert!(err.contains("不是经历库里的条目"), "{err}");
    }

    #[test]
    fn update_entry_patches_fields_and_bullets() {
        let h = Harness::new();
        h.run(
            "update_entry",
            json!({
                "entryId": "e2",
                "fields": [{ "label": "公司", "value": "新公司" }, { "label": "职位", "value": "主管" }],
                "bullets": ["新的措辞"]
            }),
        )
        .unwrap();

        let ops = h.ops();
        assert_eq!(ops[0]["type"], json!("entryPatch"));
        assert_eq!(ops[0]["entryId"], json!("e2"));
        assert_eq!(ops[0]["patch"]["fields"].as_array().unwrap().len(), 2);
        assert_eq!(ops[0]["patch"]["bullets"], json!(["新的措辞"]));

        let err = h
            .run("update_entry", json!({ "entryId": "e9", "bullets": ["x"] }))
            .unwrap_err();
        assert!(err.contains("找不到条目"), "{err}");
    }

    /// 改完又删：排队等它的 entryPatch 必须作废，
    /// 否则前端会收到「改一个已经不存在的条目」
    #[test]
    fn removing_an_entry_drops_its_pending_patch() {
        let h = Harness::new();
        h.run("set_field", json!({ "fieldId": "f4", "value": "改过的公司" })).unwrap();
        h.run("remove_entry", json!({ "entryId": "e2" })).unwrap();

        // 删掉的条目在同一轮里再改：应当拒绝，而不是静默无效
        let err = h.run("update_entry", json!({ "entryId": "e2", "bullets": ["x"] })).unwrap_err();
        assert!(err.contains("已经删掉"), "{err}");

        let ops = h.ops();
        assert_eq!(ops.len(), 1, "{ops:?}");
        assert_eq!(ops[0]["type"], json!("sectionItems"));
        assert_eq!(ops[0]["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn move_entry_reorders_within_the_section() {
        let h = Harness::new();
        h.run("move_entry", json!({ "entryId": "e2", "to": 0 })).unwrap();
        let ops = h.ops();
        let items = ops[0]["items"].as_array().unwrap();
        assert_eq!(items[0]["id"], json!("e2"));
        assert_eq!(items[1]["id"], json!("e1"));

        let err = h.run("move_entry", json!({ "entryId": "e1", "to": 9 })).unwrap_err();
        assert!(err.contains("超范围"), "{err}");
    }

    /// 新建分区后同轮往里加条目：顺序必须先是 addSection，再是 sectionItems
    #[test]
    fn new_section_can_be_filled_in_the_same_turn() {
        let h = Harness::new();
        h.run("add_section", json!({ "title": "技能", "afterBlockId": "s1" })).unwrap();
        let ops = h.ops();
        assert_eq!(ops[0]["type"], json!("addSection"));
        assert_eq!(ops[0]["afterBlockId"], json!("s1"));
        assert!(ops[0]["id"].as_str().unwrap().starts_with("sc-"));

        // 新 id 在 workspace 里查不到，但同轮的 add_entry 必须认得它
        let h2 = Harness::new();
        let envelope = with_state(&h2.out, |state| {
            let ctx = h2.ctx();
            core_add_section(state, &json!({ "title": "技能" }), &ctx).unwrap();
            let id = state
                .new_sections
                .keys()
                .next()
                .cloned()
                .expect("新分区应当登记在案");
            core_add_entry(
                state,
                &json!({ "sectionId": id, "fields": [{ "label": "语言", "value": "英语" }] }),
                &ctx,
            )
            .unwrap();
            state.flush().unwrap()
        });
        let ops = envelope["ops"].as_array().cloned().unwrap();
        assert_eq!(ops[0]["type"], json!("addSection"));
        assert_eq!(ops[1]["type"], json!("sectionItems"));
        assert_eq!(ops[1]["items"].as_array().unwrap().len(), 1);
        assert_eq!(ops[1]["items"][0]["fields"][0]["value"], json!("英语"));
    }

    #[test]
    fn add_section_rejects_unknown_anchor() {
        let h = Harness::new();
        let err = h
            .run("add_section", json!({ "title": "技能", "afterBlockId": "nope" }))
            .unwrap_err();
        assert!(err.contains("这一页有"), "{err}");
        assert!(err.contains("b1"), "{err}");
    }

    #[test]
    fn section_title_and_removal_emit_dedicated_ops() {
        let h = Harness::new();
        h.run("set_section_title", json!({ "sectionId": "s1", "title": "经历" })).unwrap();
        h.run("remove_section", json!({ "sectionId": "s1" })).unwrap();
        let ops = h.ops();
        assert_eq!(ops[0]["type"], json!("sectionTitle"));
        assert_eq!(ops[0]["title"], json!("经历"));
        assert_eq!(ops[1]["type"], json!("removeSection"));
        assert_eq!(ops[1]["sectionId"], json!("s1"));

        let err = h.run("set_section_title", json!({ "sectionId": "nope", "title": "x" })).unwrap_err();
        assert!(err.contains("找不到分区"), "{err}");
    }

    #[test]
    fn block_role_sets_and_clears() {
        let h = Harness::new();
        h.run("set_block_role", json!({ "blockId": "s1", "role": "tags" })).unwrap();
        h.run("set_block_role", json!({ "blockId": "b1" })).unwrap();
        let ops = h.ops();
        assert_eq!(ops[0]["type"], json!("blockRole"));
        assert_eq!(ops[0]["role"], json!("tags"));
        // 不传 role = 清掉：要**省略**这个键（前端的形状校验看到 null 会整条丢掉）
        assert!(ops[1].get("role").is_none(), "{}", ops[1]);

        let err = h.run("set_block_role", json!({ "blockId": "s1", "role": "随便" })).unwrap_err();
        assert!(err.contains("不认识"), "{err}");
        assert!(err.contains("tags"), "{err}");
        // split 已从可写枚举撤出（render 把它当「只适合手写片段」处理）
        let err = h.run("set_block_role", json!({ "blockId": "s1", "role": "split" })).unwrap_err();
        assert!(err.contains("不认识"), "{err}");

        let err = h.run("set_block_role", json!({ "blockId": "m1", "role": "tags" })).unwrap_err();
        assert!(err.contains("只对分区块与字段块生效"), "{err}");
    }

    /// 删掉用户正看着的那一页：必须先切到邻页，否则中间会闪一下空面板
    #[test]
    fn deleting_the_current_page_opens_a_neighbour_first() {
        let h = Harness::new();
        let text = h.run("delete_page", json!({ "pageId": "pg1" })).unwrap();
        assert!(text.contains("界面会向用户确认"), "{text}");

        let actions = h.actions();
        assert_eq!(actions[0], json!({ "type": "openPage", "pageId": "pg2" }));
        let ops = h.ops();
        assert_eq!(ops[0]["type"], json!("removePage"));
        assert_eq!(ops[0]["pageId"], json!("pg1"));

        let err = h.run("delete_page", json!({ "pageId": "nope" })).unwrap_err();
        assert!(err.contains("现有"), "{err}");
    }

    /// 删的不是当前页就不该动界面焦点
    #[test]
    fn deleting_another_page_does_not_move_the_user() {
        let h = Harness::new();
        h.run("delete_page", json!({ "pageId": "pg2" })).unwrap();
        assert!(h.actions().is_empty());
    }

    #[test]
    fn pages_and_resumes_emit_their_own_ops() {
        let h = Harness::new();
        let text = h.run("add_page", json!({ "label": "作品集" })).unwrap();
        assert!(text.contains("作品集"), "{text}");
        h.run("rename_page", json!({ "pageId": "pg2", "label": "EN" })).unwrap();
        let ops = h.ops();
        assert_eq!(ops[0]["type"], json!("addPage"));
        assert_eq!(ops[0]["label"], json!("作品集"));
        assert_eq!(ops[1]["type"], json!("pageLabel"));
        assert_eq!(ops[1]["label"], json!("EN"));

        let err = h.run("rename_page", json!({ "pageId": "nope", "label": "x" })).unwrap_err();
        assert!(err.contains("没有页面"), "{err}");
    }

    /// 新建的简历在 workspace 里还没有，但同轮改名必须能找到它
    #[test]
    fn new_resume_can_be_renamed_in_the_same_turn() {
        let h = Harness::new();
        with_state(&h.out, |state| {
            let ctx = h.ctx();
            core_add_resume(state, &json!({ "title": "英文版" }), &ctx).unwrap();
            let id = state.new_resumes.iter().next().cloned().expect("新简历要登记");
            core_rename_resume(state, &json!({ "resumeId": id, "title": "English" }), &ctx).unwrap();
            core_delete_resume(state, &json!({ "resumeId": id }), &ctx).unwrap();
        });
        let ops = h.ops();
        let kinds: Vec<&str> = ops.iter().map(|op| op["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, vec!["createResume", "renameResume", "deleteResume"]);
    }

    #[test]
    fn delete_resume_rejects_unknown_and_warns() {
        let h = Harness::new();
        let err = h.run("delete_resume", json!({ "resumeId": "nope" })).unwrap_err();
        assert!(err.contains("没有简历"), "{err}");

        let text = h.run("delete_resume", json!({ "resumeId": "r2" })).unwrap();
        assert!(text.contains("界面会向用户确认"), "{text}");
        assert_eq!(h.ops()[0]["type"], json!("deleteResume"));
    }

    #[test]
    fn library_items_can_be_created_updated_and_removed() {
        let h = Harness::new();
        let text = h
            .run(
                "save_library_item",
                json!({ "fields": [{ "label": "学校", "value": "海事大学" }], "notes": "本科" }),
            )
            .unwrap();
        assert!(text.contains("新建"), "{text}");
        assert!(text.contains("共 3 条"), "{text}");
        let ops = h.ops();
        let items = ops[0]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[2]["notes"], json!("本科"));
        let new_id = items[2]["id"].as_str().unwrap().to_string();

        // 改已有条目：assets 等别的键不能丢
        let h2 = Harness::new();
        h2.run(
            "save_library_item",
            json!({ "itemId": "it1", "fields": [{ "label": "公司", "value": "中远海运" }] }),
        )
        .unwrap();
        let ops = h2.ops();
        let items = ops[0]["items"].as_array().unwrap();
        assert_eq!(items[0]["id"], json!("it1"));
        assert_eq!(items[0]["notes"], json!("记录"), "notes 没传就该留着");

        let h3 = Harness::new();
        let text = h3.run("remove_library_item", json!({ "itemId": "it2" })).unwrap();
        assert!(text.contains("还剩 1 条"), "{text}");
        assert!(text.contains("立即生效"), "{text}");
        assert_eq!(h3.ops()[0]["items"].as_array().unwrap().len(), 1);

        let h4 = Harness::new();
        let err = h4.run("remove_library_item", json!({ "itemId": "nope" })).unwrap_err();
        assert!(err.contains("经历库里没有"), "{err}");
        assert!(!new_id.is_empty());
    }

    /// 第 3 条失败 → 前 2 条一条都不能生效，界面指令也要退回去
    #[test]
    fn apply_patch_rolls_back_the_whole_batch() {
        let h = Harness::new();
        let err = h
            .run(
                "apply_patch",
                json!({ "ops": [
                    { "tool": "set_field", "args": { "fieldId": "f1", "value": "李四" } },
                    { "tool": "delete_page", "args": { "pageId": "pg1" } },
                    { "tool": "set_field", "args": { "fieldId": "f-nope", "value": "x" } }
                ]}),
            )
            .unwrap_err();
        assert!(err.contains("第 3 条"), "{err}");
        assert!(err.contains("整批已回滚"), "{err}");
        // 回滚后不该有任何内容或界面残留
        assert!(h.flush().is_none());
        assert!(h.actions().is_empty(), "{:?}", h.actions());
    }

    #[test]
    fn apply_patch_rejects_non_content_tools_and_oversized_batches() {
        let h = Harness::new();
        let err = h
            .run(
                "apply_patch",
                json!({ "ops": [{ "tool": "open_page", "args": { "index": 1 } }] }),
            )
            .unwrap_err();
        assert!(err.contains("不是内容工具"), "{err}");

        let many: Vec<Value> = (0..41)
            .map(|_| json!({ "tool": "set_field", "args": { "fieldId": "f1", "value": "x" } }))
            .collect();
        let err = h.run("apply_patch", json!({ "ops": many })).unwrap_err();
        assert!(err.contains("最多 40 条"), "{err}");
    }

    #[test]
    fn apply_patch_keeps_the_last_value_per_container() {
        let h = Harness::new();
        let text = h
            .run(
                "apply_patch",
                json!({ "ops": [
                    { "tool": "set_field", "args": { "fieldId": "f1", "value": "李四" } },
                    { "tool": "add_field", "args": { "ownerId": "p1", "label": "邮箱", "value": "a@b.c" } }
                ]}),
            )
            .unwrap();
        assert!(text.contains("已提交 2 项改动"), "{text}");
        let ops = h.ops();
        assert_eq!(ops.len(), 1, "同一个容器合并成一条：{ops:?}");
        assert_eq!(ops[0]["fields"].as_array().unwrap().len(), 3);
    }

    /// 契约 1.2：op 里的 id 由后端填，模型不写；纯人级 op 不带 resumeId/pageId
    #[test]
    fn every_op_carries_the_ids_the_contract_asks_for() {
        let h = Harness::new();
        h.run("set_field", json!({ "fieldId": "f1", "value": "李四" })).unwrap();
        h.run("add_entry", json!({ "sectionId": "s1", "fields": [{ "label": "x" }] })).unwrap();
        h.run("set_block_role", json!({ "blockId": "s1", "role": "kv" })).unwrap();
        h.run("add_page", json!({})).unwrap();
        h.run("save_library_item", json!({ "fields": [{ "label": "x" }] })).unwrap();

        for op in h.ops() {
            assert_eq!(op["personId"], json!("p1"), "{op}");
            let kind = op["type"].as_str().unwrap_or("");
            if kind == "personFields" || kind == "libraryItems" {
                // 纯人级：带上页码会让人以为改动只影响某一页
                assert!(op.get("resumeId").is_none(), "{op}");
                assert!(op.get("pageId").is_none(), "{op}");
                continue;
            }
            assert_eq!(op["resumeId"], json!("r1"), "{op}");
            if kind == "addPage" {
                // 新页的 id 走 `id`（前端拿它定页 id），不是 pageId
                assert!(
                    op["id"].as_str().map(|id| !id.is_empty()).unwrap_or(false),
                    "addPage 缺 id：{op}"
                );
                continue;
            }
            // 其余 op 的 pageId 指向被操作的那一页，不一定是当前页，但一定要有值
            assert!(
                op["pageId"].as_str().map(|id| !id.is_empty()).unwrap_or(false),
                "pageId 缺失：{op}"
            );
        }
    }

    /// 跨简历的页也能按 id 定位：候选人范围内都算「当前」
    #[test]
    fn other_resumes_pages_are_reachable_by_id() {
        let h = Harness::new();
        h.run("rename_page", json!({ "pageId": "pg2", "label": "EN" })).unwrap();
        assert_eq!(h.ops()[0]["resumeId"], json!("r1"));
        let err = h.run("delete_page", json!({ "pageId": "pg9" })).unwrap_err();
        // pg9 属于另一份简历：delete_page 只在当前简历里找
        assert!(err.contains("没有页面"), "{err}");
    }

    /// begin 要在每轮开头清干净，否则上一轮没结算的改动会漏进这一轮
    #[test]
    fn begin_discards_leftovers() {
        let h = Harness::new();
        h.run("set_field", json!({ "fieldId": "f1", "value": "李四" })).unwrap();
        begin(&h.out);
        assert!(h.flush().is_none());
    }
}
