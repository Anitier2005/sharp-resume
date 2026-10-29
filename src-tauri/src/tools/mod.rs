//! 统一工具层。
//!
//! 所有 agent 能力都从这里进出：注册表 → JSON Schema → 执行分发。
//! 会话 agent 和（后面的）页面排版 agent 共用这一层，区别只在「允许调用哪些名字」，
//! 而不是各写一套。页面控制类工具将来也挂在这张表里，不另开旁路。
//!
//! 数据从哪来：前端每轮把工作区快照推上来，工具只读这份镜像。
//! 写操作（P2）不直接改镜像，而是产出补丁指令回传给前端执行 —— 判断在 Rust，
//! 应用在前端，页面层不参与任何决策。

pub mod ask;
pub mod edit;
pub mod image;
pub mod layout;
pub mod page;
pub mod read;
pub mod view;

use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::model::Asset;

/// 工具出站：本轮执行产生、要交给调用方落地的东西。
///
/// 分两类而不是塞进一个数组，因为它们的落地方式不同：
///   `actions` → 交给 reducer 当界面指令分发
///   `assets`  → 并进 state 的材料列表（是数据，不是动作）
/// 合成一个结构体是为了让 `ToolCtx` 只多一个字段：以后再加一类出站结果
/// （审计记录、警告）时，所有构造点都不用动。
#[derive(Default)]
pub struct Outbox {
    /// 界面指令，上限由 `push_action` 控制
    pub actions: Mutex<Vec<Value>>,
    /// 工具新建的材料（现在只有图片转化会产出）
    pub assets: Mutex<Vec<Asset>>,
    /// 本轮内容写入的暂存区。工具改它，轮末由 `agent::turn` flush 成一条 mutate 信封。
    ///
    /// 为什么不直接往 `actions` 里塞内容指令：同一轮里模型常连着改同一个容器
    /// （先改标题、再加一条），每次都基于同一份 `workspace` 快照算新数组，
    /// 后一次会把前一次冲掉。攒在一处、轮末一次结算，才能保证顺序即语义。
    pub edits: Mutex<crate::tools::edit::EditState>,
    /// 本轮已经贴了几条纸面批注。
    ///
    /// 计数放在出站口而不是 `EditState`：批注不是内容写入，走的是 `actions`；
    /// 而且「每轮清零」这件事 `Outbox` 天生就有（每轮新建一个），不需要重置逻辑。
    pub notes: AtomicUsize,
    /// 本轮已经向会话 agent 提了几个问题。上限 1：两个 agent 互相追着问会打成乒乓。
    pub asks: AtomicUsize,
    /// 工具渲染出来、要喂给视觉模型看的图片（data URL）。
    /// 由 `agent::turn` 在工具结果之后作为一条用户消息附上 —— 工具结果本身
    /// 只能是字符串，图片没法塞在里面。
    pub views: Mutex<Vec<String>>,
    /// 当前模型能不能看图。放在出站口是为了不污染 `ToolCtx` 的构造点
    /// （工具与测试里有一堆），每轮由 `chat_send` 按模型能力写一次。
    pub vision: bool,
}

/// 工具执行时能看到的东西：只读快照 + 当前定位 + 出站口。
pub struct ToolCtx<'a> {
    pub workspace: &'a Value,
    pub person_id: &'a str,
    pub resume_id: &'a str,
    pub page_id: &'a str,
    pub out: &'a Outbox,
    /// 数据目录。只有需要碰真实文件的工具才用得到（目前是图片转化）；
    /// 浏览器调试模式没有数据目录，这类工具会明确报「当前环境不支持」。
    pub root: Option<&'a Path>,
}

impl<'a> ToolCtx<'a> {
    /// 只读场景（测试、离线）：没有数据目录，出站口由调用方提供
    pub fn of(
        workspace: &'a Value,
        person_id: &'a str,
        resume_id: &'a str,
        page_id: &'a str,
        out: &'a Outbox,
    ) -> Self {
        Self {
            workspace,
            person_id,
            resume_id,
            page_id,
            out,
            root: None,
        }
    }
}

/// 把一条界面指令放进出站队列。
pub fn push_action(ctx: &ToolCtx, action: Value) -> Result<(), String> {
    let mut list = ctx
        .out
        .actions
        .lock()
        .map_err(|_| "界面指令队列不可用".to_string())?;
    // 上限保护：模型偶尔会在同一轮里反复调 set_ui，没必要把几十条都回传
    if list.len() >= 16 {
        // 静默丢会让模型以为用户看见了。留一行日志，排查「界面没动」时至少能对上账
        let kind = action.get("type").and_then(Value::as_str).unwrap_or("?");
        eprintln!("[tools] action 队列已满，丢弃：{kind}");
        return Ok(());
    }
    list.push(action);
    Ok(())
}

/// 把一份新材料放进出站队列。
/// 渲染出来的图片交给「下一轮消息」。上限 4：再多也不会有模型认真看。
pub fn push_view(ctx: &ToolCtx, data_url: &str) -> Result<(), String> {
    let mut list = ctx.out.views.lock().map_err(|_| "图片队列不可用".to_string())?;
    if list.len() < 4 {
        list.push(data_url.to_string());
    }
    Ok(())
}

pub fn push_asset(ctx: &ToolCtx, asset: Asset) -> Result<(), String> {
    let mut list = ctx
        .out
        .assets
        .lock()
        .map_err(|_| "材料队列不可用".to_string())?;
    if list.len() < 8 {
        list.push(asset);
    }
    Ok(())
}

/// 工具归属：统一的注册表，但不同 agent 看到的能力不同。
///
/// 分配方式是**每个工具声明一份角色列表**（`Tool::scopes`），而不是反过来
/// 维护「每个角色有哪些工具」—— 那样同一个事实存两遍，加一个工具就要记得改两处。
///
/// 新增一个 agent 的成本：这个枚举加一项 → 想给它的工具在列表里加上它。
/// 分发、越权拒绝、schema 生成、测试断言全部不用动。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// 会话 agent：和用户对话、读内容、转交排版
    Session,
    /// 页排版 agent：拥有这一页版面的全部决定权
    Layout,
}

/// 两个 agent 都能用
pub const EVERYONE: &[Scope] = &[Scope::Session, Scope::Layout];

pub struct Tool {
    pub name: &'static str,
    /// 写给模型看的：什么时候该用它。措辞直接决定它会不会乱调。
    pub description: &'static str,
    pub params: fn() -> Value,
    pub run: fn(&Value, &ToolCtx) -> Result<String, String>,
    /// 谁能调。这是唯一的能力配置点。
    pub scopes: &'static [Scope],
    /// 有没有副作用：会改数据、写文件、发起外部调用。
    ///
    /// 目前只用于把「只读工具」和「会改变状态的工具」分开，便于
    /// `read_person` 这类工具在将来做并行；也让审查时一眼能看出哪些是写操作。
    pub writes: bool,
}

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "read_person",
        description: "读取当前候选人：所有字段、有几份简历、经历库有多少条。\
                      用户问「我的信息里有什么」「还缺什么」时先调这个。",
        params: read::no_params,
        run: read::read_person,
        scopes: EVERYONE,
        writes: false,
    },
    Tool {
        name: "list_pages",
        description: "列出当前这份简历的所有页（页码、页名、页面 id）并标出当前页。\
                      不确定「一共几页」「第几页叫什么」时必须调它，不要猜。",
        params: read::no_params,
        run: read::list_pages,
        scopes: EVERYONE,
        writes: false,
    },
    Tool {
        name: "read_page",
        description: "读取某一页的完整结构化内容：页名、页头字段、每个分区的标题与条目\
                      （含本页措辞）。不传 pageId 就是当前页。\
                      任何涉及「这一页写了什么」的问题都必须先读，不许凭记忆回答。",
        params: read::page_params,
        run: read::read_page,
        scopes: EVERYONE,
        writes: false,
    },
    Tool {
        name: "read_library",
        description: "读取经历库条目（字段 + 详实记录）。用户提到「那段经历」「库里的材料」\
                      或要改写却没有足够细节时调它。",
        params: read::library_params,
        run: read::read_library,
        scopes: EVERYONE,
        writes: false,
    },
    /* ---------- 内容写入 ----------
     *
     * 全部只改 `Outbox.edits`（本轮的暂存区），轮末由 `EditState::flush()` 结算成
     * 一条 `mutate` 信封交给前端 reducer。所以它们「写」但不「立刻可见」——
     * 返回文案必须说清「已提交」，不能让模型以为用户那边已经变了。
     * 破坏性操作还要提醒模型别报「已删除」：用户可能点取消。
     */
    Tool {
        name: "set_field",
        description: "改一个已有字段的标签或内容。用户说「邮箱换成…」「职位写错了」「照片换这张」时用它。\
                      产物是这条字段所在容器的全量替换，落在表单里对应的那一格。\
                      字段 id 从 read_person / read_page 的输出里取，不要按标签猜；只想清空内容就把 value 给空串。",
        params: edit::set_field_params,
        run: edit::set_field,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "add_field",
        description: "在某个容器里新增一项（「期望城市」「到岗时间」这类补丁）。用户说「再加一项」「补个电话」时用它。\
                      产物是一行新字段，插在 ownerId 那个容器的末尾，或 afterFieldId 指向的字段之后。\
                      ownerId 可以是候选人 id、页头字段块 id、条目 id、经历库条目 id —— 工具自己判断归哪个容器，\
                      所以别为了加一项先去改别的字段；改已有字段请用 set_field。",
        params: edit::add_field_params,
        run: edit::add_field,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "remove_field",
        description: "删掉一个字段。只在用户明确说「这项不要了」时用，产物是它从所在容器里消失。\
                      只是想清空文字不要用它 —— 那会让表单少一行，用户会以为界面出错了；清空请用 set_field 传 value: \"\"。",
        params: edit::remove_field_params,
        run: edit::remove_field,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "add_entry",
        description: "在一个分区里加一条条目（一段经历、一个项目、一条证书）。用户说「再补一段工作经历」时用它。\
                      产物是新条目出现在该分区末尾，点开能看到你给的字段与要点。\
                      这条素材经历库里已经有就传 refId，不要先把库里的字段抄一遍 —— 抄一遍之后两边就不同步了。",
        params: edit::add_entry_params,
        run: edit::add_entry,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "update_entry",
        description: "改某一条条目自己的字段或要点（用户在说「第二节那句改成…」时）。\
                      产物是这一条就地更新，只覆盖你给的部分，没给的字段原样留着。\
                      要整段重写就把 bullets 全量给；只想换位置请用 move_entry，别删了再新建。",
        params: edit::update_entry_params,
        run: edit::update_entry,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "remove_entry",
        description: "删掉分区里的一条条目（这一段经历不想要了）。产物是这一条从分区里消失，分区本身和别的条目都还在。\
                      删整栏请用 remove_section，别循环删条目。",
        params: edit::remove_entry_params,
        run: edit::remove_entry,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "move_entry",
        description: "调整条目在分区里的先后。用户说「最近这段放最前面」「项目排在经历后面」时用它，`to` 是 0 基的新位置。\
                      产物是分区内的条目顺序变了，内容一个字都没动。",
        params: edit::move_entry_params,
        run: edit::move_entry,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "add_section",
        description: "新建一个分区（页面上的一栏，如「教育经历」「技能」）。用户说「加一栏技能」时用它。\
                      产物是页面上多一个空分区，标题有了但还没有条目 —— 紧接着还要 add_entry 才有内容。",
        params: edit::add_section_params,
        run: edit::add_section,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "set_section_title",
        description: "改分区标题那几个字。用户说「这栏叫项目经历，不叫作品」时用它。\
                      产物只是标题变了，里面的条目一条不动。",
        params: edit::set_section_title_params,
        run: edit::set_section_title,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "remove_section",
        description: "删掉整个分区，连同里面所有条目。只在用户说「这栏整个不要了」时用。\
                      产物是这一栏从页面上消失，别的分区不受影响。\
                      删除会弹确认窗、用户可能拒绝，所以不要说「已删除」，说「已提交，界面会问你」；\
                      只是暂时不想让它出现就先别删，问问用户是要删还是要挪到别的页。",
        params: edit::remove_section_params,
        run: edit::remove_section,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "set_block_role",
        description: "给某个块指定结构化角色，让生成器按这个形态排：tags（一行标签）、kv（标签-值清单）、\
                      table（表格）、grid-2/grid-3（多栏）、band（通栏色带）、tight/plain（普通条目）。\
                      用户说「技能挤成一行」「证书做成表格」时用它。产物只是排版提示，一个字的正文都不改；\
                      不确定就用 plain，别为了「看起来更专业」乱设。",
        params: edit::set_block_role_params,
        run: edit::set_block_role,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "add_page",
        description: "给当前简历新增一页（零内容）。用户说「再加一页放作品集」时用它。\
                      产物是新页出现在页列表末尾，需要再往里加分区与条目。\
                      空的整页原件（证书 PDF）不是这个工具的事，那种要用户自己传文件。",
        params: edit::add_page_params,
        run: edit::add_page,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "delete_page",
        description: "删掉一整页，连同这一页的内容。用户明确说要删某一页时才用。\
                      产物是这一页从简历里移除。\
                      这会弹确认窗、用户可能拒绝，所以**不要对用户说「已删除」**，说「已提交，界面会问你」；\
                      只想清空内容不要删页。",
        params: edit::delete_page_params,
        run: edit::delete_page,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "rename_page",
        description: "改页名（页签上显示的那几个字）。用户说「这页叫英文版」时用它。\
                      产物只是页名变了，页里的内容不受影响。",
        params: edit::rename_page_params,
        run: edit::rename_page,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "add_resume",
        description: "新建一份简历（另一个版本、另一种语言）。用户说「再做一份英文的」时用它。\
                      产物是候选人名下多一份空简历，页面要另外加。\
                      只是想在现有简历里加页，请用 add_page。",
        params: edit::add_resume_params,
        run: edit::add_resume,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "rename_resume",
        description: "改简历标题（侧栏里显示的那份名字）。产物只是标题变了，页与内容都不动。",
        params: edit::rename_resume_params,
        run: edit::rename_resume,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "delete_resume",
        description: "删掉整份简历，含它的全部页面。用户明确说「那份不要了」时才用。\
                      产物是这份简历从候选人名下移除。\
                      这会弹确认窗、用户可能拒绝，所以**不要对用户说「已删除」**，说「已提交，界面会问你」；\
                      用户只是想换个版本看，请先 open_resume 切过去。",
        params: edit::delete_resume_params,
        run: edit::delete_resume,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "save_library_item",
        description: "把一段素材写进经历库（库里是给自己看的原料，不会印到简历上）。\
                      带 itemId 是更新那一条，不带是新建；产物是库条目变了，页面上的正文不动。\
                      想改已经印在纸上的内容，请用 update_entry，别在库里改完指望它会同步。",
        params: edit::save_library_item_params,
        run: edit::save_library_item,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "remove_library_item",
        description: "从经历库里删掉一条素材。产物是库条目消失。\
                      注意：页面上引用它的条目会失去从库里继承来的字段 —— 删之前先 read_page 看一眼有没有人在用。",
        params: edit::remove_library_item_params,
        run: edit::remove_library_item,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "apply_patch",
        description: "一次改多处时用它：ops 里每一项是 {tool, args}，tool 就是上面这些工具的名字，按顺序执行。\
                      产物仍是一条信封、一次提交，比连着调十次省时间也不会被中途打断。\
                      任何一条失败整批回滚，返回第几条错、为什么；单点小改别用它，直接调那个工具更清楚。",
        params: edit::apply_patch_params,
        run: edit::apply_patch,
        scopes: &[Scope::Session],
        writes: true,
    },
    /* ---------- 页面控制（只影响「看到什么」，不改内容） ---------- */
    Tool {
        name: "open_page",
        description: "把用户的界面切到某一页。用户说「打开第二页」「切到英文那页」「回到刚才那页」时用它。\
                      不要凭记忆猜页码：不确定就先 list_pages。",
        params: page::open_page_params,
        run: page::open_page,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "list_people",
        description: "列出所有候选人：能认人的名字、id、各有几份简历。\
                      只在用户提到**另一个**候选人、或要切换候选人时用它 —— 换人之前先知道有哪些人。\
                      它不报任何简历内容，问「我的简历里写了什么」请用 read_person / read_page。",
        params: read::no_params,
        run: page::list_people,
        scopes: &[Scope::Session],
        writes: false,
    },
    Tool {
        name: "open_person",
        description: "把界面切到另一个候选人。用户说「换回上一个候选人」「看看另一个人」时用它。\
                      产物只是焦点变了，不写任何内容。\
                      切过去之后你看到的材料才跟着变；在切换之前不要动别人的数据，你本来也只能改当前这位。",
        params: page::open_person_params,
        run: page::open_person,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "open_resume",
        description: "把界面切到同一候选人下的另一份简历。用户说「我要看英文那份」「切回中文版」时用它。\
                      产物只是焦点变了，不写任何内容。\
                      不知道有哪些简历就先 read_person，别猜 resumeId。",
        params: page::open_resume_params,
        run: page::open_resume,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "set_view",
        description: "切换编辑 / 预览。用户说「看看成品长什么样」「排版效果如何」用 preview；\
                      说「我要改」「回到表单」用 edit。只在用户明确要看/要改时切，别自作主张。",
        params: page::view_params,
        run: page::set_view,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "set_open_pages",
        description: "决定右侧快捷标签显示哪几页、什么顺序（只影响快捷栏，不删页也不改内容）。\
                      用户说「把这三页放到标签上」「只留英文页的标签」时用它，pageIds 从 list_pages 取。",
        params: page::open_pages_params,
        run: page::set_open_pages,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "ask_user",
        description: "向用户发一张填写表单（单行/多行文本、单选、多选、是/否、数字、日期）。\
                      信息不足、需要用户拍板、或要一次性问清多件事时用它，不要靠猜；\
                      能只问一句就别开表单（直接问更自然），要问三件以上才用表单。",
        params: ask::params,
        run: ask::ask,
        scopes: &[Scope::Session],
        writes: false,
    },
    Tool {
        name: "set_ui",
        description: "改界面外观：主题色、明暗、字号、左右栏宽度与显隐。\
                      仅当用户明确要求换配色 / 字太小 / 栏太窄时才调；用户没提就别动。\
                      改完要把结果说清楚，让用户知道变了什么。",
        params: page::ui_params,
        run: page::set_ui,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "request_layout",
        description: "把「这一页要怎么排」的要求交给该页的排版 agent（它负责留白与疏密，                      不碰内容）。用户抱怨太挤 / 太散 / 装不下时用它，别自己调样式。",
        params: layout::request_layout_params,
        run: layout::request_layout,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "read_layout_guide",
        description: "读取纸面组件手册：有哪些可用的 class、怎么拼、能写哪些标签、不能写什么。\
                      第一次给某一页排版前读一次；之后按需要再查。不读就写 HTML 很容易用到不存在的组件。",
        params: layout::guide_params,
        run: layout::read_layout_guide,
        scopes: &[Scope::Layout],
        writes: false,
    },
    Tool {
        name: "convert_image",
        description: "把一张已有图片处理成合适的大小/格式/比例：压到最长边上限、转 jpeg 或 png、\
                      按 1:1 / 3:4 / 16:9 居中裁剪。用户说「照片太大」「裁成正方形」「要 jpg」时用它，\
                      不要建议用户自己去找压缩工具。处理完会得到一个新资产 id，原图不会被改。",
        params: image::params,
        run: image::convert,
        scopes: EVERYONE,
        writes: true,
    },
    Tool {
        name: "view_file_pages",
        description: "把一份 PDF 逐页渲染成图片来「看」：原件怎么排的、留白多少、哪里挤。\
                      用户让你照着某个 PDF 还原、或让你核对版式时用。\
                      渲染出来的页会作为图片附在下一轮消息里，同时进材料清单；只对能看图的模型有效。",
        params: view::params,
        run: view::render,
        scopes: &[Scope::Session],
        writes: true,
    },
    Tool {
        name: "set_page_layout",
        description: "改这一页的成品版面。三档力度，按需选最轻的那档：\
                      preset 换整体疏密；parts 只重写某几个块（块 id 从 read_page 取）；\
                      html 整页接管，只在结构化确实做不出来时用。\
                      这是排版 agent 的核心工具 —— 内容不在你手里，版面全部由你决定。",
        params: layout::set_layout_params,
        run: layout::set_page_layout,
        scopes: &[Scope::Layout],
        writes: true,
    },
    Tool {
        name: "note_on_page",
        description: "在纸面上留一条给**用户**看的批注。拿不准这里该怎么排、或想提醒「这段按现在这样会挤出页」时用它。\
                      产物是一个贴在纸面上的批注点：给了 blockId 就贴那一块，没给就贴页边，用户能点开也能关掉。\
                      它不是给自己记笔记，用户会直接看到 —— 想改内容请用 ask_session。一轮最多 2 条，说完继续排版。",
        params: layout::note_params,
        run: layout::note_on_page,
        scopes: &[Scope::Layout],
        writes: true,
    },
    Tool {
        name: "ask_session",
        description: "向会话 agent 提一个具体问题：这段内容要不要拆成两条、这个措辞能不能改。\
                      产物是一条发给会话 agent 的消息（带上你指的那一块），它可能自己查库回答，也可能转去问用户。\
                      要问就问能一句话回答的（「这两段合并成一条行不行」），不要写「请确认」这种没法接的话；\
                      一轮只能问一次，同一个问题别重复问 —— 你说完继续排版，不要停下来等。",
        params: layout::ask_session_params,
        run: layout::ask_session,
        scopes: &[Scope::Layout],
        writes: true,
    },
    Tool {
        name: "search_knowledge",
        description: "查知识库：客观的求职知识 —— 行业口径、岗位要求、术语、常见面试问法。\
                      写简历里的数字、时间、职责之前先查一次：查到就照着写，查不到就照实说不知道，\
                      不要用记忆里的印象补。返回的是命中的「块」，带摘要与 chunkId；需要原文再 read_knowledge。",
        params: knowledge_params,
        run: crate::knowledge::search_tool,
        scopes: &[Scope::Session],
        writes: false,
    },
    Tool {
        name: "search_history",
        description: "翻这段对话的旧记录。上下文窗口装不下全部历史，早先说过的事会被挤出去 ——\
                      用户提到「之前说过的」「上次那个数」而你没印象时，用它按关键词检索原文，\
                      不要凭猜测补。命中的是原话摘录，可直接引用。",
        params: history_params,
        run: crate::history::search_tool,
        scopes: &[Scope::Session],
        writes: false,
    },
    Tool {
        name: "read_knowledge",
        description: "读知识库原文。传 chunkId 只读那一块，传 entryId 读整篇。\
                      只在 search_knowledge 的摘要不足以回答时用 —— 整篇很长，读多了会挤掉真正要改的内容。",
        params: knowledge_read_params,
        run: crate::knowledge::read_tool,
        scopes: &[Scope::Session],
        writes: false,
    },
];

fn knowledge_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "要查的问题或关键词，用用户原话里的名词" },
            "tags": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：只在这些标签里找"
            },
            "limit": { "type": "integer", "description": "返回几条，默认 5，最多 10" }
        },
        "required": ["query"]
    })
}

fn history_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "要找的内容，用当时提到的名词或数字（如「QPS」「薪资」「第三轮」）"
            },
            "role": {
                "type": "string",
                "enum": ["user", "assistant"],
                "description": "可选：只在用户说的话或只说助手说过的话里找"
            },
            "limit": { "type": "integer", "description": "返回几条，默认 6" }
        },
        "required": ["query"]
    })
}

fn knowledge_read_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {
                "type": "string",
                "description": "search_knowledge 返回的 chunkId（读一块）或 entryId（读整篇）"
            }
        },
        "required": ["id"]
    })
}

/// 转成 OpenAI 的 tools 数组（按 agent 过滤）
pub fn specs(scope: Scope) -> Value {
    let list: Vec<Value> = TOOLS
        .iter()
        .filter(|tool| visible(tool, scope))
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": (tool.params)(),
                }
            })
        })
        .collect();
    Value::Array(list)
}

fn visible(tool: &Tool, scope: Scope) -> bool {
    tool.scopes.contains(&scope)
}

/// 某个角色能用的全部工具名，按注册顺序。测试与审查用它做快照。
pub fn names_for(scope: Scope) -> Vec<&'static str> {
    TOOLS
        .iter()
        .filter(|tool| visible(tool, scope))
        .map(|tool| tool.name)
        .collect()
}

/// 按名字分发。未知工具不 panic —— 模型偶尔会编一个名字，要把错误当结果喂回去，
/// 让它下一轮自己纠正。
///
/// 越权调用（会话 agent 调 set_page_style）也走这里拒绝：报错文本会变成工具结果，
/// 模型下一轮就知道该改调 request_layout。
pub fn call(name: &str, args: &Value, ctx: &ToolCtx, scope: Scope) -> Result<String, String> {
    match TOOLS.iter().find(|tool| tool.name == name && visible(tool, scope)) {
        Some(tool) => (tool.run)(args, ctx),
        None => {
            let denied = TOOLS
                .iter()
                .any(|tool| tool.name == name && !visible(tool, scope));
            if denied {
                return Err(format!("当前角色不能用 {name}，换成你这边允许的工具"));
            }
            Err(format!(
                "没有名为 {name} 的工具，可用的有：{}",
                TOOLS
                    .iter()
                    .filter(|tool| visible(tool, scope))
                    .map(|t| t.name)
                    .collect::<Vec<_>>()
                    .join(" / ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_openai_shaped() {
        let specs = specs(Scope::Session);
        let first = &specs[0];
        assert_eq!(first["type"], "function");
        assert!(first["function"]["name"].is_string());
        assert_eq!(first["function"]["parameters"]["type"], "object");
    }

    /// 分配机制的自检：能力只写在每个工具的 `scopes` 里。
    ///
    /// 这条测试的作用不是「断言现在长这样」，而是把「谁能干什么」固定在
    /// 一个能一眼看完的地方 —— 加角色、加工具时如果漏了配置，这里会红。
    #[test]
    fn capability_map_is_one_line_per_tool() {
        let session = names_for(Scope::Session);
        let layout = names_for(Scope::Layout);

        // 工具数就是能力边界的快照：少一个说明注册时漏了，多一个说明有人偷偷
        // 给某个角色开了口子，两者都该在这里被看见
        assert_eq!(session.len(), 38, "会话 agent 的工具数变了：{session:?}");
        assert_eq!(layout.len(), 9, "排版 agent 的工具数变了：{layout:?}");

        // 会话 agent：能读、能转交、能转图，但不能自己动版面
        assert!(session.contains(&"request_layout"));
        assert!(session.contains(&"convert_image"));
        // PDF 渲染给自己看：只有会话需要，排版 agent 拿到的是结构化内容
        assert!(session.contains(&"view_file_pages"));
        assert!(!layout.contains(&"view_file_pages"));
        // 知识库是只读检索：会话能查、排版不能（排版不改内容，查事实没有落点）
        assert!(session.contains(&"search_knowledge"));
        assert!(session.contains(&"read_knowledge"));
        // 历史检索是会话独有的：排版 agent 没有对话可翻
        assert!(session.contains(&"search_history"));
        assert!(!layout.contains(&"search_history"));
        assert!(!layout.contains(&"search_knowledge"));
        assert!(!session.contains(&"set_page_layout"));
        assert!(!session.contains(&"read_layout_guide"));

        // 排版 agent：能读、能写版面、能转图，但不能切页/改界面/问用户
        assert!(layout.contains(&"set_page_layout"));
        assert!(layout.contains(&"read_layout_guide"));
        assert!(layout.contains(&"convert_image"));
        assert!(!layout.contains(&"open_page"));
        assert!(!layout.contains(&"ask_user"));

        // 写操作必须标出来：审查时靠它快速看出哪些会改数据
        let readers: Vec<&str> = TOOLS
            .iter()
            .filter(|tool| !tool.writes)
            .map(|tool| tool.name)
            .collect();
        assert!(readers.contains(&"read_page"));
        assert!(readers.contains(&"read_layout_guide"));
        assert!(readers.contains(&"list_people"), "列候选人不动任何数据");
        assert!(readers.contains(&"search_knowledge"), "查知识库不改任何状态");
        assert!(readers.contains(&"search_history"), "翻历史不改任何状态");
        assert!(!readers.contains(&"set_page_layout"));
        assert!(!readers.contains(&"convert_image"));
        // 内容工具全部有副作用（改的是本轮暂存区，轮末才落地，但仍然是写）
        assert!(!readers.contains(&"set_field"));
        assert!(!readers.contains(&"apply_patch"));
        assert!(!readers.contains(&"note_on_page"));
        // 问用户不改任何状态：它只发一张表单，等用户填
        assert!(readers.contains(&"ask_user"));
    }

    /// 会话 agent 现在有内容写权限，但仍然不能自己动版面。
    ///
    /// 这两件事必须靠 `scopes` 分开：一旦给会话 agent 开了 `set_page_layout`，
    /// 它就会绕过 `request_layout` 直接改样式，排版 agent 的存在就没有意义了。
    #[test]
    fn session_tools_include_content_writes() {
        let specs = specs(Scope::Session);
        let names: Vec<&str> = specs
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|item| item["function"]["name"].as_str())
                    .collect()
            })
            .unwrap_or_default();

        for name in ["set_field", "add_field", "delete_page", "apply_patch", "open_resume"] {
            assert!(names.contains(&name), "会话 agent 少了 {name}：{names:?}");
        }
        // 换人的前提是知道有哪些人：没有目录，open_person 只能靠报错试 id
        assert!(names.contains(&"list_people"), "{names:?}");
        assert!(
            !names.contains(&"set_page_layout"),
            "排版不归会话 agent，它只能 request_layout"
        );
    }

    /// 排版 agent 只能改版面：批注与提问是它的出口，内容工具一个都不给。
    #[test]
    fn layout_tools_do_not_write_content() {
        let names = names_for(Scope::Layout);
        for name in ["note_on_page", "ask_session", "set_page_layout"] {
            assert!(names.contains(&name), "排版 agent 少了 {name}：{names:?}");
        }
        for name in ["set_field", "delete_page", "apply_patch", "open_person"] {
            assert!(!names.contains(&name), "排版 agent 不该有 {name}");
        }
    }

    #[test]
    fn unknown_tool_lists_alternatives() {
        let ws = json!({ "people": [] });
        let out = Outbox::default();
        let ctx = ToolCtx {
            workspace: &ws,
            person_id: "p",
            resume_id: "r",
            page_id: "g",
            out: &out,
            root: None,
        };
        let err = call("read_everything", &json!({}), &ctx, Scope::Session).unwrap_err();
        assert!(err.contains("read_page"), "{err}");
    }

    fn sample_ws() -> Value {
        json!({
            "people": [{
                "id": "p1",
                "assets": [],
                "resumes": [{
                    "id": "r1",
                    "pages": [
                        {
                            "id": "pg1",
                            "kind": "native",
                            "label": "第 1 页",
                            "blocks": [
                                { "id": "b1", "kind": "fields" },
                                { "id": "b2", "kind": "section", "title": "工作" }
                            ]
                        },
                        { "id": "pg2", "kind": "native", "label": "English", "blocks": [] }
                    ]
                }]
            }]
        })
    }

    fn ctx_of<'a>(ws: &'a Value, out: &'a Outbox) -> ToolCtx<'a> {
        ToolCtx {
            workspace: ws,
            person_id: "p1",
            resume_id: "r1",
            page_id: "pg1",
            out,
            root: None,
        }
    }

    #[test]
    fn page_tools_emit_actions_not_state_changes() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        let text = call("open_page", &json!({ "index": 2 }), &ctx, Scope::Session).unwrap();
        assert!(text.contains("English"), "{text}");

        call("set_view", &json!({ "view": "preview" }), &ctx, Scope::Session).unwrap();
        call("set_open_pages", &json!({ "pageIds": ["pg2"] }), &ctx, Scope::Session).unwrap();
        call("set_ui", &json!({ "mode": "dark", "sideW": 9999 }), &ctx, Scope::Session).unwrap();

        let actions = out.actions.lock().unwrap().clone();
        assert_eq!(actions[0], json!({ "type": "openPage", "pageId": "pg2" }));
        assert_eq!(actions[1], json!({ "type": "setView", "view": "preview" }));
        // 当前页是 pg1，必须被补回列表首位
        assert_eq!(actions[2]["pageIds"], json!(["pg1", "pg2"]));
        // 宽度被夹到 480，而不是原样传给前端
        assert_eq!(actions[3]["patch"]["sideW"], json!(480));
        assert_eq!(actions[3]["patch"]["mode"], json!("dark"));
    }

    #[test]
    fn page_tools_reject_bad_arguments() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        assert!(call("open_page", &json!({ "pageId": "pg-nope" }), &ctx, Scope::Session)
            .unwrap_err()
            .contains("没有 id"));
        assert!(call("open_page", &json!({}), &ctx, Scope::Session).unwrap_err().contains("哪一页"));
        assert!(call("set_view", &json!({ "view": "fullscreen" }), &ctx, Scope::Session).is_err());
        assert!(call("set_ui", &json!({ "accentHex": "blue" }), &ctx, Scope::Session).is_err());
        assert!(call("set_ui", &json!({}), &ctx, Scope::Session).is_err());
        // 失败不该留下任何指令
        assert!(out.actions.lock().unwrap().is_empty());
    }

    #[test]
    fn scope_keeps_layout_tools_out_of_session_agent() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        // 会话 agent 动不了版面：它只能转交，不能直接写
        for name in ["set_page_layout", "read_layout_guide"] {
            let err = call(name, &json!({}), &ctx, Scope::Session).unwrap_err();
            assert!(err.contains("不能用"), "{name}: {err}");
        }

        // 排版 agent 调不了切页 / 改界面
        for name in ["open_page", "set_ui", "set_view", "ask_user"] {
            let err = call(name, &json!({}), &ctx, Scope::Layout).unwrap_err();
            assert!(err.contains("不能用"), "{name}: {err}");
        }

        // 两边都能读
        assert!(call("list_pages", &json!({}), &ctx, Scope::Layout).is_ok());
        assert!(call("list_pages", &json!({}), &ctx, Scope::Session).is_ok());
    }

    /// 连着改两次版面：第二次必须看到第一次的结果。
    /// 曾经以快照为基线，第二次会把第一次的 preset 与块一起抹掉。
    #[test]
    fn layout_tool_accumulates_within_one_turn() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        call(
            "set_page_layout",
            &json!({ "preset": "compact", "parts": { "b1": "<h1>A</h1>" } }),
            &ctx,
            Scope::Layout,
        )
        .unwrap();
        call(
            "set_page_layout",
            &json!({ "parts": { "b2": "<section>B</section>" } }),
            &ctx,
            Scope::Layout,
        )
        .unwrap();

        let actions = out.actions.lock().unwrap().clone();
        assert_eq!(actions.len(), 2);
        let layout = &actions[1]["layout"];
        assert_eq!(actions[1]["type"], json!("patchPageLayout"));
        assert_eq!(actions[1]["pageId"], json!("pg1"));
        assert_eq!(layout["preset"], json!("compact"), "没传的预设不能被清掉");
        assert_eq!(layout["parts"]["b1"], json!("<h1>A</h1>"), "上一块要留着");
        assert_eq!(layout["parts"]["b2"], json!("<section>B</section>"));
    }

    #[test]
    fn layout_tool_rejects_unknown_blocks_and_oversized_parts() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        let err = call(
            "set_page_layout",
            &json!({ "parts": { "nope": "<p>x</p>" } }),
            &ctx,
            Scope::Layout,
        )
        .unwrap_err();
        assert!(err.contains("b1"), "报错要把正确的块 id 列出来：{err}");

        let huge = "x".repeat(7000);
        let err = call(
            "set_page_layout",
            &json!({ "parts": { "b1": huge } }),
            &ctx,
            Scope::Layout,
        )
        .unwrap_err();
        assert!(err.contains("上限"), "{err}");
    }

    #[test]
    fn layout_tool_can_undo_everything() {
        let mut ws = sample_ws();
        ws["people"][0]["resumes"][0]["pages"][0]["layout"] =
            json!({ "preset": "compact", "parts": { "b1": "<h1>A</h1>" } });
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        call("set_page_layout", &json!({ "reset": true }), &ctx, Scope::Layout).unwrap();
        let actions = out.actions.lock().unwrap().clone();
        // 空对象是「没有任何覆盖」，不是「没给」—— 前端要据此清空
        assert_eq!(actions[0]["layout"], json!({}));
    }

    /// 切人 / 切简历只改焦点：产物只有一条界面指令，不碰任何内容。
    ///
    /// 已经在那个人身上时不再发指令 —— 空转会让前端多一次重渲染和滚动复位，
    /// 用户看到的是「界面闪了一下」。
    #[test]
    fn open_person_and_open_resume_only_move_the_focus() {
        let ws = json!({
            "people": [
                {
                    "id": "p1",
                    "fields": [{ "id": "f1", "label": "姓名", "value": "张三" }],
                    "resumes": [
                        { "id": "r1", "title": "中文版", "pages": [] },
                        { "id": "r2", "title": "English", "pages": [] }
                    ]
                },
                { "id": "p2", "fields": [{ "id": "f2", "label": "姓名", "value": "李四" }], "resumes": [] }
            ]
        });
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        // 换人的前提：先有目录。只报名字 + id + 简历数，别把内容也倒出来
        let text = call("list_people", &json!({}), &ctx, Scope::Session).unwrap();
        assert!(text.contains("张三") && text.contains("李四"), "{text}");
        assert!(text.contains("← 当前"), "当前那位要标出来：{text}");
        assert!(text.contains("2 份简历"), "{text}");
        assert!(out.actions.lock().unwrap().is_empty(), "只读工具不该产出指令");

        let text = call("open_person", &json!({ "personId": "p2" }), &ctx, Scope::Session).unwrap();
        assert!(text.contains("李四"), "要用人名回话，用户才认得出：{text}");
        let text = call("open_resume", &json!({ "resumeId": "r2" }), &ctx, Scope::Session).unwrap();
        assert!(text.contains("English"), "{text}");

        let actions = out.actions.lock().unwrap().clone();
        assert_eq!(actions[0], json!({ "type": "openPerson", "personId": "p2" }));
        // openResume 要带上 personId：前端按「人 + 简历」定位，缺一个就定不到位
        assert_eq!(
            actions[1],
            json!({ "type": "openResume", "personId": "p1", "resumeId": "r2" })
        );

        // 错的 id 要把可用的一并列出，模型才有下一轮的自纠机会
        let err = call("open_person", &json!({ "personId": "p9" }), &ctx, Scope::Session)
            .unwrap_err();
        assert!(err.contains("p1") && err.contains("p2"), "{err}");
        let err = call("open_resume", &json!({ "resumeId": "r9" }), &ctx, Scope::Session)
            .unwrap_err();
        assert!(err.contains("r1") && err.contains("r2"), "{err}");

        let before = out.actions.lock().unwrap().len();
        call("open_person", &json!({ "personId": "p1" }), &ctx, Scope::Session).unwrap();
        assert_eq!(out.actions.lock().unwrap().len(), before, "原地切换不该再发指令");
    }

    /// 批注与提问的产物形状：id 与时间戳必须在 Rust 生成，前端要做去重。
    #[test]
    fn note_and_question_carry_their_own_ids() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        call(
            "note_on_page",
            &json!({ "text": "技能这一栏压成一行后字号会掉到 8px", "blockId": "b2" }),
            &ctx,
            Scope::Layout,
        )
        .unwrap();
        call("note_on_page", &json!({ "text": "整页留白偏多" }), &ctx, Scope::Layout).unwrap();
        call(
            "ask_session",
            &json!({ "question": "这段实习要不要和上一段合并成一条", "blockId": "b2" }),
            &ctx,
            Scope::Layout,
        )
        .unwrap();

        let actions = out.actions.lock().unwrap().clone();
        assert_eq!(actions[0]["type"], json!("pageNote"));
        assert_eq!(actions[0]["personId"], json!("p1"));
        assert_eq!(actions[0]["resumeId"], json!("r1"));
        assert_eq!(actions[0]["pageId"], json!("pg1"));
        assert_eq!(actions[0]["note"]["blockId"], json!("b2"));
        assert!(actions[0]["note"]["id"].as_str().unwrap().starts_with("n-"));
        assert!(actions[0]["note"]["at"].as_i64().unwrap() > 0, "at 要是 unix 秒");

        // 整页批注不该带 blockId 这个键
        assert!(actions[1]["note"].get("blockId").is_none(), "{:?}", actions[1]);

        assert_eq!(actions[2]["type"], json!("askSession"));
        assert_eq!(actions[2]["pageId"], json!("pg1"));
        assert_eq!(actions[2]["question"], json!("这段实习要不要和上一段合并成一条"));
        // 会话 agent 靠 blockId 定位问的是哪一段，没有它就得把整页再读一遍
        assert_eq!(actions[2]["blockId"], json!("b2"));
    }

    /// 批注 2 条、提问 1 次是硬限，第 3 条必须报错。
    ///
    /// 静默丢会让模型以为话已经说出去了，总结里还会说「我已经在纸面上标注了」——
    /// 用户那边什么都没有，而且不会再被提醒。
    #[test]
    fn notes_and_questions_are_hard_limited_per_turn() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        call("note_on_page", &json!({ "text": "第一条" }), &ctx, Scope::Layout).unwrap();
        call("note_on_page", &json!({ "text": "第二条" }), &ctx, Scope::Layout).unwrap();
        let err = call("note_on_page", &json!({ "text": "第三条" }), &ctx, Scope::Layout)
            .unwrap_err();
        assert!(err.contains("2 条"), "要说清上限是几条：{err}");
        assert_eq!(out.actions.lock().unwrap().len(), 2, "被拒的那条不能留下指令");

        call("ask_session", &json!({ "question": "第一问" }), &ctx, Scope::Layout).unwrap();
        let err = call("ask_session", &json!({ "question": "第二问" }), &ctx, Scope::Layout)
            .unwrap_err();
        assert!(err.contains("1 条"), "{err}");
        assert_eq!(out.actions.lock().unwrap().len(), 3);

        // 名额记在出站口上：换一个 Outbox 就是新的一轮，不需要额外重置
        let fresh = Outbox::default();
        let next = ctx_of(&ws, &fresh);
        call("note_on_page", &json!({ "text": "新一轮的批注" }), &next, Scope::Layout).unwrap();
        call("ask_session", &json!({ "question": "新一轮的问题" }), &next, Scope::Layout).unwrap();
    }

    #[test]
    fn note_and_question_reject_bad_input_without_side_effects() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        let err = call(
            "note_on_page",
            &json!({ "text": "贴错块", "blockId": "nope" }),
            &ctx,
            Scope::Layout,
        )
        .unwrap_err();
        assert!(err.contains("b1"), "报错要列出这一页真实的块：{err}");

        let long = "字".repeat(201);
        let err = call("note_on_page", &json!({ "text": long }), &ctx, Scope::Layout).unwrap_err();
        assert!(err.contains("上限"), "{err}");
        assert!(call("note_on_page", &json!({}), &ctx, Scope::Layout)
            .unwrap_err()
            .contains("不能为空"));

        let err = call("ask_session", &json!({ "question": "x".repeat(301) }), &ctx, Scope::Layout)
            .unwrap_err();
        assert!(err.contains("上限"), "{err}");

        // 会话 agent 不能留批注，排版 agent 不能改内容：两个方向都要拦住
        assert!(call("note_on_page", &json!({ "text": "x" }), &ctx, Scope::Session)
            .unwrap_err()
            .contains("不能用"));
        assert!(call("set_field", &json!({ "fieldId": "f1" }), &ctx, Scope::Layout)
            .unwrap_err()
            .contains("不能用"));

        assert!(out.actions.lock().unwrap().is_empty(), "失败不该留下指令");
    }

    #[test]
    fn ask_user_builds_a_form_and_rejects_bad_questions() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        let text = call(
            "ask_user",
            &json!({ "questions": [
                { "label": "目标岗位", "kind": "text", "required": true },
                { "label": "要不要英文版", "kind": "confirm" },
                { "label": "重点突出哪些", "kind": "multi", "options": ["规模", "量化", "工具"] }
            ]}),
            &ctx,
            Scope::Session,
        )
        .unwrap();
        assert!(text.contains("3 道题"), "{text}");

        let actions = out.actions.lock().unwrap().clone();
        assert_eq!(actions[0]["type"], json!("ask"));
        assert_eq!(actions[0]["questions"][0]["id"], json!("q1"));
        assert_eq!(actions[0]["questions"][2]["options"][0], json!("规模"));

        // select 不给 options 必须拒绝，否则前端渲染成空下拉
        let err = call(
            "ask_user",
            &json!({ "questions": [{ "label": "选一个", "kind": "select" }] }),
            &ctx,
            Scope::Session,
        )
        .unwrap_err();
        assert!(err.contains("options"), "{err}");
    }
}
