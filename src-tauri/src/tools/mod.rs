//! 统一工具层。
//!
//! 所有 agent 能力都从这里进出：注册表 → JSON Schema → 执行分发。
//! 只有一条 agent 循环（`agent::turn::run`），所以这张表就是它能用的**全部**工具 ——
//! 没有角色过滤，也没有「某一边不该看见」的旁路。加工具只改这一处。
//!
//! 数据从哪来：前端每轮把工作区快照推上来。读工具读的**不是**那份静态快照，
//! 而是本轮工作副本（`ToolCtx::view`：快照 + 本轮已经暂存的写入）—— 否则模型写完
//! 立刻回头读，读不到自己刚写的字，就会再写一遍。
//! 写操作（P2）不直接改前端状态，而是产出补丁指令回传给前端执行 —— 判断在 Rust，
//! 应用在前端，页面层不参与任何决策。

pub mod ask;
pub mod edit;
pub mod extract;
pub mod image;
pub mod layout;
pub mod page;
pub mod read;
pub mod render;
pub mod source;
pub mod target;
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
    /// 为什么需要暂存区而不是直接往 `actions` 里塞内容指令：同一个容器一轮里可能被改
    /// 十次，每次都要基于「含前几次改动」的底本算新数组，否则后一次会冲掉前一次。
    /// 这份底本就是 `EditState` 里的工作副本 —— 暂存的每一条写入入队时会**同步镜像**
    /// 进去，所以读工具看得到刚写的字，同轮多次写同一容器也不会互相覆盖。
    /// 轮末的信封格式没变：还是一条 `mutate`、一串同样的 op，前端 reducer 照旧应用。
    pub edits: Mutex<crate::tools::edit::EditState>,
    /// 本轮已经贴了几条纸面批注。
    ///
    /// 计数放在出站口而不是 `EditState`：批注不是内容写入，走的是 `actions`；
    /// 而且「每轮清零」这件事 `Outbox` 天生就有（每轮新建一个），不需要重置逻辑。
    pub notes: AtomicUsize,
    /// 工具渲染出来、要喂给视觉模型看的图片（data URL）。
    /// 由 `agent::turn` 在工具结果之后作为一条用户消息附上 —— 工具结果本身
    /// 只能是字符串，图片没法塞在里面。
    pub views: Mutex<Vec<String>>,
    /// 当前模型能不能看图。放在出站口是为了不污染 `ToolCtx` 的构造点
    /// （工具与测试里有一堆），每轮由 `chat_send` 按模型能力写一次。
    pub vision: bool,
    /// 本轮用的模型名。只给「看不了图」这类文案用（要指名道姓说清是哪个模型），
    /// 空串表示测试/离线，文案里就不带名字。
    pub model: String,
    /// 这一轮是「第几条用户消息」。工具写目标页数时要把它落库
    /// （`targetPagesTurn`），下一轮才能判出「这一轮刚要求」还是「以前要求过」。
    pub turn: u32,
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
    /// 本轮页面事实（原件页数 / 目标 / 实测）。`None` = 调用点没带上，
    /// 需要它的工具（`set_page_layout`）就不追加实测结论，而不是编一个。
    pub facts: Option<&'a crate::agent::facts::PageFacts>,
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
            facts: None,
        }
    }

    /// 本轮工作副本：本轮开始时的快照，叠上本轮已经暂存的每一条写入。
    ///
    /// 为什么读工具要读它而不是 `workspace`：`workspace` 是前端上送的静态快照，
    /// 一轮之内永不更新。模型 `add_field` 加一个字段、紧接着 `read_page`，
    /// 读到的还是旧快照 —— 它以为没写进去，再写一遍，用户就看到重复字段。
    ///
    /// 没 `begin` 过（测试、离线）时回落到快照，行为与旧版一致。
    pub fn view(&self) -> Value {
        crate::tools::edit::live(self.out).unwrap_or_else(|| self.workspace.clone())
    }

    /// 换一份只读底本，其余（定位 / 出站口 / 环境）照旧。
    ///
    /// 分发层用它把 `view()` 的结果装回 ctx，这样工具内部照常写 `ctx.workspace`，
    /// 读到的已经是工作副本 —— 不必在几十个工具入口各写一遍（漏一个就又是旧 bug）。
    pub fn on<'b>(&'b self, workspace: &'b Value) -> ToolCtx<'b> {
        ToolCtx {
            workspace,
            person_id: self.person_id,
            resume_id: self.resume_id,
            page_id: self.page_id,
            out: self.out,
            root: self.root,
            facts: self.facts,
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
/// 文案里怎么写模型名：`「deepseek-chat」`。名字缺失（测试、离线）时留空，
/// 免得印出「当前模型「」不支持看图」。
pub(crate) fn model_label(model: &str) -> String {
    let name = model.trim();
    if name.is_empty() {
        String::new()
    } else {
        format!("「{name}」")
    }
}

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

/// 工具注册表里的一行。
pub struct Tool {
    pub name: &'static str,
    /// 写给模型看的：什么时候该用它。措辞直接决定它会不会乱调。
    pub description: &'static str,
    pub params: fn() -> Value,
    pub run: fn(&Value, &ToolCtx) -> Result<String, String>,
    /// 有没有副作用：会改数据、写文件、发起外部调用。
    ///
    /// 只用于区分「只读工具」与「会改变状态的工具」，让审查时一眼能看出哪些是写操作；
    /// 全量清单的护栏测试也靠它做只读/写两侧的快照。
    pub writes: bool,
}

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "read_person",
        description: "读取当前候选人：所有字段、有几份简历、经历库有多少条。\
                      用户问「我的信息里有什么」「还缺什么」时先调这个。",
        params: read::no_params,
        run: read::read_person,
        writes: false,
    },
    Tool {
        name: "list_pages",
        description: "列出当前这份简历的所有页（页码、页名、页面 id）并标出当前页。\
                      不确定「一共几页」「第几页叫什么」时必须调它，不要猜。",
        params: read::no_params,
        run: read::list_pages,
        writes: false,
    },
    Tool {
        name: "read_page",
        description: "读取某一页的完整结构化内容：页名、页头字段、每个分区的标题与条目\
                      （含本页措辞）。不传 pageId 就是当前页。\
                      任何涉及「这一页写了什么」的问题都必须先读，不许凭记忆回答。",
        params: read::page_params,
        run: read::read_page,
        writes: false,
    },
    Tool {
        name: "read_library",
        description: "读取经历库条目（字段 + 详实记录）。用户提到「那段经历」「库里的材料」\
                      或要改写却没有足够细节时调它。",
        params: read::library_params,
        run: read::read_library,
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
        writes: true,
    },
    Tool {
        name: "remove_field",
        description: "删掉一个字段。只在用户明确说「这项不要了」时用，产物是它从所在容器里消失。\
                      只是想清空文字不要用它 —— 那会让表单少一行，用户会以为界面出错了；清空请用 set_field 传 value: \"\"。",
        params: edit::remove_field_params,
        run: edit::remove_field,
        writes: true,
    },
    Tool {
        name: "add_entry",
        description: "在一个分区里加一条条目（一段经历、一个项目、一条证书）。用户说「再补一段工作经历」时用它。\
                      产物是新条目出现在该分区末尾，点开能看到你给的字段与要点。\
                      这条素材经历库里已经有就传 refId，不要先把库里的字段抄一遍 —— 抄一遍之后两边就不同步了。",
        params: edit::add_entry_params,
        run: edit::add_entry,
        writes: true,
    },
    Tool {
        name: "update_entry",
        description: "改某一条条目自己的字段或要点（用户在说「第二节那句改成…」时）。\
                      产物是这一条就地更新，只覆盖你给的部分，没给的字段原样留着。\
                      要整段重写就把 bullets 全量给；只想换位置请用 move_entry，别删了再新建。",
        params: edit::update_entry_params,
        run: edit::update_entry,
        writes: true,
    },
    Tool {
        name: "remove_entry",
        description: "删掉分区里的一条条目（这一段经历不想要了）。产物是这一条从分区里消失，分区本身和别的条目都还在。\
                      删整栏请用 remove_section，别循环删条目。",
        params: edit::remove_entry_params,
        run: edit::remove_entry,
        writes: true,
    },
    Tool {
        name: "move_entry",
        description: "调整条目在分区里的先后。用户说「最近这段放最前面」「项目排在经历后面」时用它，`to` 是 0 基的新位置。\
                      产物是分区内的条目顺序变了，内容一个字都没动。",
        params: edit::move_entry_params,
        run: edit::move_entry,
        writes: true,
    },
    Tool {
        name: "add_section",
        description: "新建一个分区（页面上的一栏，如「教育经历」「技能」）。用户说「加一栏技能」时用它。\
                      产物是页面上多一个空分区，标题有了但还没有条目 —— 紧接着还要 add_entry 才有内容。",
        params: edit::add_section_params,
        run: edit::add_section,
        writes: true,
    },
    Tool {
        name: "set_section_title",
        description: "改分区标题那几个字。用户说「这栏叫项目经历，不叫作品」时用它。\
                      产物只是标题变了，里面的条目一条不动。",
        params: edit::set_section_title_params,
        run: edit::set_section_title,
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
        writes: true,
    },
    Tool {
        name: "add_page",
        description: "给当前简历新增一页（零内容）。用户说「再加一页放作品集」时用它。\
                      产物是新页出现在页列表末尾，需要再往里加分区与条目。\
                      空的整页原件（证书 PDF）不是这个工具的事，那种要用户自己传文件。",
        params: edit::add_page_params,
        run: edit::add_page,
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
        writes: true,
    },
    Tool {
        name: "rename_page",
        description: "改页名（页签上显示的那几个字）。用户说「这页叫英文版」时用它。\
                      产物只是页名变了，页里的内容不受影响。",
        params: edit::rename_page_params,
        run: edit::rename_page,
        writes: true,
    },
    Tool {
        name: "add_resume",
        description: "新建一份简历（另一个版本、另一种语言）。用户说「再做一份英文的」时用它。\
                      产物是候选人名下多一份空简历，页面要另外加。\
                      只是想在现有简历里加页，请用 add_page。",
        params: edit::add_resume_params,
        run: edit::add_resume,
        writes: true,
    },
    Tool {
        name: "rename_resume",
        description: "改简历标题（侧栏里显示的那份名字）。产物只是标题变了，页与内容都不动。",
        params: edit::rename_resume_params,
        run: edit::rename_resume,
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
        writes: true,
    },
    Tool {
        name: "save_library_item",
        description: "把一段素材写进经历库（库里是给自己看的原料，不会印到简历上）。\
                      带 itemId 是更新那一条，不带是新建；产物是库条目变了，页面上的正文不动。\
                      想改已经印在纸上的内容，请用 update_entry，别在库里改完指望它会同步。",
        params: edit::save_library_item_params,
        run: edit::save_library_item,
        writes: true,
    },
    Tool {
        name: "remove_library_item",
        description: "从经历库里删掉一条素材。产物是库条目消失。\
                      注意：页面上引用它的条目会失去从库里继承来的字段 —— 删之前先 read_page 看一眼有没有人在用。",
        params: edit::remove_library_item_params,
        run: edit::remove_library_item,
        writes: true,
    },
    Tool {
        name: "apply_patch",
        description: "一次改多处时用它：ops 里每一项是 {tool, args}，tool 就是上面这些工具的名字，按顺序执行。\
                      产物仍是一条信封、一次提交，比连着调十次省时间也不会被中途打断。\
                      任何一条失败整批回滚，返回第几条错、为什么；单点小改别用它，直接调那个工具更清楚。",
        params: edit::apply_patch_params,
        run: edit::apply_patch,
        writes: true,
    },
    Tool {
        name: "extract_images",
        description: "从用户上传的 PDF 里抽出内嵌位图（证件照通常就在里面），登记成候选材料。\
                      用户传了带照片的简历 PDF、或说「把照片放到页头」时先调它。\
                      `stored` 从材料段里那行「附件标识」取，**不是文件名**。\
                      返回的清单每行开头反引号里就是 assetId，交给 set_field 写进页头图片字段。\
                      系统不判断哪张是证件照，挑不出就问用户，不要猜。",
        params: extract::extract_images_params,
        run: extract::extract_images,
        writes: true,
    },
    Tool {
        name: "set_target_pages",
        description: "记下用户要求的目标页数（「压到一页」「控制在一页内」）。\
                      只带 pages，不带 resumeId —— 改的是当前这份简历。\
                      只在用户**明确说过**页数时调；他没说过就别替他定目标。\
                      他说「不要求页数了」就不传 pages（= 清除）。",
        params: target::set_target_pages_params,
        run: target::set_target_pages,
        writes: true,
    },
    /* ---------- 页面控制（只影响「看到什么」，不改内容） ---------- */
    Tool {
        name: "open_page",
        description: "把用户的界面切到某一页。用户说「打开第二页」「切到英文那页」「回到刚才那页」时用它。\
                      不要凭记忆猜页码：不确定就先 list_pages。",
        params: page::open_page_params,
        run: page::open_page,
        writes: true,
    },
    Tool {
        name: "list_people",
        description: "列出所有候选人：能认人的名字、id、各有几份简历。\
                      只在用户提到**另一个**候选人、或要切换候选人时用它 —— 换人之前先知道有哪些人。\
                      它不报任何简历内容，问「我的简历里写了什么」请用 read_person / read_page。",
        params: read::no_params,
        run: page::list_people,
        writes: false,
    },
    Tool {
        name: "open_person",
        description: "把界面切到另一个候选人。用户说「换回上一个候选人」「看看另一个人」时用它。\
                      产物只是焦点变了，不写任何内容。\
                      切过去之后你看到的材料才跟着变；在切换之前不要动别人的数据，你本来也只能改当前这位。",
        params: page::open_person_params,
        run: page::open_person,
        writes: true,
    },
    Tool {
        name: "open_resume",
        description: "把界面切到同一候选人下的另一份简历。用户说「我要看英文那份」「切回中文版」时用它。\
                      产物只是焦点变了，不写任何内容。\
                      不知道有哪些简历就先 read_person，别猜 resumeId。",
        params: page::open_resume_params,
        run: page::open_resume,
        writes: true,
    },
    Tool {
        name: "set_view",
        description: "切换编辑 / 预览。用户说「看看成品长什么样」「排版效果如何」用 preview；\
                      说「我要改」「回到表单」用 edit。只在用户明确要看/要改时切，别自作主张。",
        params: page::view_params,
        run: page::set_view,
        writes: true,
    },
    Tool {
        name: "set_open_pages",
        description: "决定右侧快捷标签显示哪几页、什么顺序（只影响快捷栏，不删页也不改内容）。\
                      用户说「把这三页放到标签上」「只留英文页的标签」时用它，pageIds 从 list_pages 取。",
        params: page::open_pages_params,
        run: page::set_open_pages,
        writes: true,
    },
    Tool {
        name: "ask_user",
        description: "向用户发一张填写表单（单行/多行文本、单选、多选、是/否、数字、日期）。\
                      信息不足、需要用户拍板、或要一次性问清多件事时用它，不要靠猜；\
                      能只问一句就别开表单（直接问更自然），要问三件以上才用表单。",
        params: ask::params,
        run: ask::ask,
        writes: false,
    },
    Tool {
        name: "set_ui",
        description: "改界面外观：主题色、明暗、字号、左右栏宽度与显隐。\
                      仅当用户明确要求换配色 / 字太小 / 栏太窄时才调；用户没提就别动。\
                      改完要把结果说清楚，让用户知道变了什么。",
        params: page::ui_params,
        run: page::set_ui,
        writes: true,
    },
    Tool {
        name: "read_layout_guide",
        description: "读取纸面组件手册：有哪些可用的 class、怎么拼、能写哪些标签、不能写什么。\
                      第一次给某一页排版前读一次；之后按需要再查。不读就写 HTML 很容易用到不存在的组件。",
        params: layout::guide_params,
        run: layout::read_layout_guide,
        writes: false,
    },
    Tool {
        name: "convert_image",
        description: "把一张已有图片处理成合适的大小/格式/比例：压到最长边上限、转 jpeg 或 png、\
                      按 1:1 / 3:4 / 16:9 居中裁剪。用户说「照片太大」「裁成正方形」「要 jpg」时用它，\
                      不要建议用户自己去找压缩工具。处理完会得到一个新资产 id，原图不会被改。",
        params: image::params,
        run: image::convert,
        writes: true,
    },
    Tool {
        name: "view_asset",
        description: "把一份材料变成「这一轮你能看见的图片」：图片直接附原件，PDF 逐页渲染。\
                      想确认证件照合不合格、想看 PDF 原件怎么排的、或用户问「你看得到吗」时用它。\
                      排版时也用它看「原件样张」—— 那一页是从原件改编来的，版式以那张图为准。\
                      附上的图片在下一轮消息里；PDF 渲染出的页也会进材料清单。只对能看图的模型有效。",
        params: view::params,
        run: view::render,
        writes: true,
    },
    Tool {
        name: "set_page_layout",
        description: "改这一页的成品版面。三档力度，按需选最轻的那档：\
                      preset 换整体疏密；parts 只重写某几个块（块 id 从 read_page 取）；\
                      html 整页接管，只在结构化确实做不出来时用。\
                      它只决定「怎么摆」，一个字都不改；要改字请用内容工具。",
        params: layout::set_layout_params,
        run: layout::set_page_layout,
        writes: true,
    },
    Tool {
        name: "note_on_page",
        description: "在纸面上留一条给**用户**看的批注。拿不准这里该怎么排、或想提醒「这段按现在这样会挤出页」时用它。\
                      产物是一个贴在纸面上的批注点：给了 blockId 就贴那一块，没给就贴页边，用户能点开也能关掉。\
                      它不是给自己记笔记，用户会直接看到；批注不改任何内容，该改的字用内容工具。一轮最多 2 条，说完继续。",
        params: layout::note_params,
        run: layout::note_on_page,
        writes: true,
    },
    Tool {
        name: "search_knowledge",
        description: "查知识库：客观的求职知识 —— 行业口径、岗位要求、术语、常见面试问法。\
                      写简历里的数字、时间、职责之前先查一次：查到就照着写，查不到就照实说不知道，\
                      不要用记忆里的印象补。返回的是命中的「块」，带摘要与 chunkId；需要原文再 read_knowledge。",
        params: knowledge_params,
        run: crate::knowledge::search_tool,
        writes: false,
    },
    Tool {
        name: "search_history",
        description: "翻这段对话的旧记录。上下文窗口装不下全部历史，早先说过的事会被挤出去 ——\
                      用户提到「之前说过的」「上次那个数」而你没印象时，用它按关键词检索原文，\
                      不要凭猜测补。命中的是原话摘录，可直接引用。",
        params: history_params,
        run: crate::history::search_tool,
        writes: false,
    },
    Tool {
        name: "read_knowledge",
        description: "读知识库原文。传 chunkId 只读那一块，传 entryId 读整篇。\
                      只在 search_knowledge 的摘要不足以回答时用 —— 整篇很长，读多了会挤掉真正要改的内容。",
        params: knowledge_read_params,
        run: crate::knowledge::read_tool,
        writes: false,
    },
    Tool {
        name: "list_sources",
        description: "列出我手上存着的原件（用户上传过的简历 PDF / 图片 / 文本）。\
                      对话过了几轮、不确定「原件还在不在」「叫什么」「stored 是什么」时先调它，\
                      不要凭记忆猜，也不要让用户重新贴一遍原文。\
                      每行末尾的 stored 就是 read_source / render_doc_page 的参数。",
        params: source::list_params,
        run: source::list_sources,
        writes: false,
    },
    Tool {
        name: "render_doc_page",
        description: "把一份原件变成这一轮看得见的图片：PDF 渲你要的那一页（一次一页，清晰度可调），\
                      图片原件直接把原图附上。材料段只给摘要和【原件结构】，没给页图：\
                      想确认版式、照片位置、表格怎么摆时，自己按 stored 要一张，别凭描述猜。\
                      docx / pptx / txt 渲不成页图，读正文用 read_source。",
        params: render::params,
        run: render::run,
        writes: false,
    },
    Tool {
        name: "read_source",
        description: "读用户上传的原件正文（PDF / 文本）。材料段里写了「要全文调 read_source」时先读它再动手，\
                      不要凭 120 字摘要写简历 —— 摘要只够认出这是谁，不够写经历。\
                      参数 stored 从 list_sources 或材料段那行「（附件标识：…）」取，不是文件名；长文档用 offset/limit 分段读。",
        params: source::params,
        run: source::run,
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

/// 转成 OpenAI 的 tools 数组。
pub fn specs() -> Value {
    let list: Vec<Value> = TOOLS
        .iter()
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

/// 全部工具名，按注册顺序。测试与审查用它做快照。
pub fn names() -> Vec<&'static str> {
    TOOLS.iter().map(|tool| tool.name).collect()
}

/// 按名字分发。未知工具不 panic —— 模型偶尔会编一个名字，要把错误当结果喂回去，
/// 让它下一轮自己纠正。
pub fn call(name: &str, args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    // 所有工具都从这里进出，所以「读工作副本」只在这一处做：
    // 工具内部照常读 `ctx.workspace`，拿到的已经是视图，谁也漏不了。
    // 必须在分发**之前**取（`view()` 会锁 `out.edits`，工具里 `with_state` 也会锁，
    // 同一线程重入会死锁）。
    let view = ctx.view();
    let ctx = &ctx.on(&view);

    match TOOLS.iter().find(|tool| tool.name == name) {
        Some(tool) => (tool.run)(args, ctx),
        None => Err(format!(
            "没有名为 {name} 的工具，可用的有：{}",
            TOOLS
                .iter()
                .map(|tool| tool.name)
                .collect::<Vec<_>>()
                .join(" / ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_openai_shaped() {
        let specs = specs();
        let first = &specs[0];
        assert_eq!(first["type"], "function");
        assert!(first["function"]["name"].is_string());
        assert_eq!(first["function"]["parameters"]["type"], "object");

        // 抽图的 schema：stored 必填、不接受别的键（模型乱塞参数会静默失败）
        let list = specs.as_array().expect("数组");
        let extract = list
            .iter()
            .find(|item| item["function"]["name"] == "extract_images")
            .expect("extract_images 要在工具清单里");
        assert_eq!(extract["function"]["parameters"]["required"], json!(["stored"]));
        assert_eq!(
            extract["function"]["parameters"]["additionalProperties"],
            json!(false)
        );
    }

    /// 全量清单的自检：这张表就是模型能用的**全部**工具，没有角色过滤。
    ///
    /// 这条测试的作用不是「断言现在长这样」，而是把「有哪些能力」固定在一个
    /// 能一眼看完的地方 —— 加工具时如果忘了注册，或者有人偷偷加了第二层过滤，
    /// 这里会红。数量是快照：改注册表就要来这里改数，逼人看一遍全表。
    #[test]
    fn capability_map_is_one_line_per_tool() {
        let names = names();

        // 45 = 内容读写 + 页面控制 + 版面三件套 + 原件四件套，一个不多一个不少
        assert_eq!(names.len(), 45, "工具数变了：{names:?}");
        // specs 与注册表必须同源：少一条说明 specs 里还在过滤
        assert_eq!(
            specs().as_array().map(|list| list.len()).unwrap_or(0),
            TOOLS.len(),
            "specs 与注册表对不上"
        );

        // 内容写入
        for name in [
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
            "add_page",
            "delete_page",
            "rename_page",
            "add_resume",
            "rename_resume",
            "delete_resume",
            "save_library_item",
            "remove_library_item",
            "apply_patch",
        ] {
            assert!(names.contains(&name), "少了内容工具 {name}：{names:?}");
        }
        // 读 / 定位 / 界面
        for name in [
            "read_person",
            "list_pages",
            "read_page",
            "read_library",
            "list_people",
            "open_page",
            "open_person",
            "open_resume",
            "set_view",
            "set_open_pages",
            "set_ui",
            "ask_user",
        ] {
            assert!(names.contains(&name), "少了 {name}：{names:?}");
        }
        // 材料与事实
        for name in [
            "extract_images",
            "convert_image",
            "view_asset",
            "list_sources",
            "render_doc_page",
            "read_source",
            "set_target_pages",
            "search_knowledge",
            "read_knowledge",
            "search_history",
        ] {
            assert!(names.contains(&name), "少了 {name}：{names:?}");
        }
        // 版面三件套：会话 agent 自己动手，不再转交
        for name in ["read_layout_guide", "set_page_layout", "note_on_page"] {
            assert!(names.contains(&name), "少了版面工具 {name}：{names:?}");
        }

        // 写操作必须标出来：审查时靠它快速看出哪些会改数据。只有只读、无副作用的
        // 那几个该在 readers 里 —— 多一个就说明有人把写工具标成了读
        let readers: Vec<&str> = TOOLS
            .iter()
            .filter(|tool| !tool.writes)
            .map(|tool| tool.name)
            .collect();
        let expected_readers = [
            "read_person",
            "list_pages",
            "read_page",
            "read_library",
            "list_people",
            "ask_user",
            "read_layout_guide",
            "search_knowledge",
            "search_history",
            "read_knowledge",
            "list_sources",
            "render_doc_page",
            "read_source",
        ];
        for name in expected_readers {
            assert!(readers.contains(&name), "{name} 应当是只读工具：{readers:?}");
        }
        assert_eq!(readers.len(), expected_readers.len(), "只读清单变了：{readers:?}");
        // 会改数据的一个都不能混进来
        for name in ["set_page_layout", "note_on_page", "set_field", "apply_patch"] {
            assert!(!readers.contains(&name), "{name} 是写操作");
        }
    }

    /// `ask_session` 是**彻底不存在**，不是「某个角色不能用」。
    ///
    /// 它当年是排版 agent 问会话 agent 的通道，产物是一条插进会话流的用户可见消息；
    /// 两个 agent 合并之后这个角色没了，工具也不该以任何形式留下 —— 连拒绝分支都不留，
    /// 因为「不能提问」这句话本身就是旧角色存在的证据。模型硬调时只能拿到
    /// 「没有名为 ask_session 的工具」，和它编一个别的名字是同一种结果。
    #[test]
    fn ask_session_is_gone_instead_of_hidden() {
        assert!(!names().contains(&"ask_session"), "全量清单里还有 ask_session");
        let spec_names: Vec<String> = specs()
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|item| item["function"]["name"].as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        assert!(!spec_names.iter().any(|name| name == "ask_session"), "{spec_names:?}");

        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);
        let err = call(
            "ask_session",
            &json!({ "question": "这段实习要不要和上一段合并成一条" }),
            &ctx,
        )
        .unwrap_err();
        assert!(err.contains("没有名为 ask_session 的工具"), "{err}");
        assert!(!err.contains("不能用"), "不许再出现「角色不能用」这种说法：{err}");
        assert!(
            out.actions.lock().unwrap().is_empty(),
            "被拒的调用不能留下指令"
        );
    }

    /// 提示词里「调 `XX`」点到的工具必须真的存在。
    ///
    /// 为什么要这条：`view` 那次和更早的 `set_field` 那次是同一个形状 —— 文案读起来
    /// 毫无问题，单测只断言「这里有个词」，全绿；真机上模型第一步就撞「没有名为 XX
    /// 的工具」。字符串断言永远抓不到这类错，因为它核的是「文案里有这句话」，
    /// 不是「这句话在真机上行得通」。
    ///
    /// 合并之后只有一个工具清单，所以这里直接对**全量** `specs()` 核对：
    /// 提示词写错、工具改名、注册漏了，都在这儿红。
    #[test]
    fn every_tool_the_system_prompt_tells_it_to_call_really_exists() {
        const SYSTEM: &str = include_str!("../../../prompts/system.md");
        let specs = specs();
        let list = specs.as_array().expect("specs 是数组");

        let seen = called_tools(SYSTEM);
        // 抽不到东西说明护栏自己失效了（句式改了就没人盯着了），比漏检更该红
        assert!(seen.len() >= 2, "一个「调 XX」都没抽到，护栏失效：{seen:?}");

        for name in &seen {
            assert!(
                list.iter().any(|item| item["function"]["name"] == name.as_str()),
                "提示词让模型调 {name}，但工具表里没有它"
            );
        }
        // 合并进来之后，系统提示词必然会点名版面工具：抽不到就是提示词还没跟上
        assert!(
            seen.iter().any(|name| name == "set_target_pages"),
            "漏抽了 set_target_pages：{seen:?}"
        );
    }

    /// 从一段文案里抽出「调 `name`」「调 name」点到的工具名。
    ///
    /// 只认带下划线的标识符：中文句子里的「调」后面跟的多半是「调整」「调用」这类词，
    /// 工具名一定有下划线，这个约束足够把噪声挡在外面。
    fn called_tools(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] != '调' {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let quoted = j < chars.len() && chars[j] == '`';
            if quoted {
                j += 1;
            }
            let start = j;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let closed = !quoted || (j < chars.len() && chars[j] == '`');
            if j > start && closed {
                let name: String = chars[start..j].iter().collect();
                if name.contains('_') && !out.contains(&name) {
                    out.push(name);
                }
            }
            i = j.max(i + 1);
        }
        out
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
            facts: None,
        };
        let err = call("read_everything", &json!({}), &ctx).unwrap_err();
        assert!(err.contains("read_page"), "{err}");
        assert!(err.contains("set_page_layout"), "清单要全量给出：{err}");
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
            facts: None,
        }
    }

    #[test]
    fn page_tools_emit_actions_not_state_changes() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        let text = call("open_page", &json!({ "index": 2 }), &ctx).unwrap();
        assert!(text.contains("English"), "{text}");

        call("set_view", &json!({ "view": "preview" }), &ctx).unwrap();
        call("set_open_pages", &json!({ "pageIds": ["pg2"] }), &ctx).unwrap();
        call("set_ui", &json!({ "mode": "dark", "sideW": 9999 }), &ctx).unwrap();

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

        assert!(call("open_page", &json!({ "pageId": "pg-nope" }), &ctx)
            .unwrap_err()
            .contains("没有 id"));
        assert!(call("open_page", &json!({}), &ctx).unwrap_err().contains("哪一页"));
        assert!(call("set_view", &json!({ "view": "fullscreen" }), &ctx).is_err());
        assert!(call("set_ui", &json!({ "accentHex": "blue" }), &ctx).is_err());
        assert!(call("set_ui", &json!({}), &ctx).is_err());
        // 失败不该留下任何指令
        assert!(out.actions.lock().unwrap().is_empty());
    }

    /// 连着改两次版面：第二次必须看到第一次的结果。
    /// 曾经以快照为基线，第二次会把第一次的 preset 与块一起抹掉。
    ///
    /// 手写片段必须带着块里原有的字（b1 是空字段组、b2 的标题是「工作」）：
    /// 文字锁会拒掉任何改字的片段，这条测试只关心合并行为。
    #[test]
    fn layout_tool_accumulates_within_one_turn() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        call(
            "set_page_layout",
            &json!({ "preset": "compact", "parts": { "b1": "<div class=\"row\"></div>" } }),
            &ctx,
        )
        .unwrap();
        call(
            "set_page_layout",
            &json!({ "parts": { "b2": "<section class=\"sec tight\"><h2>工作</h2></section>" } }),
            &ctx,
        )
        .unwrap();

        let actions = out.actions.lock().unwrap().clone();
        assert_eq!(actions.len(), 2);
        let layout = &actions[1]["layout"];
        assert_eq!(actions[1]["type"], json!("patchPageLayout"));
        assert_eq!(actions[1]["pageId"], json!("pg1"));
        assert_eq!(layout["preset"], json!("compact"), "没传的预设不能被清掉");
        assert_eq!(
            layout["parts"]["b1"],
            json!("<div class=\"row\"></div>"),
            "上一块要留着"
        );
        assert_eq!(
            layout["parts"]["b2"],
            json!("<section class=\"sec tight\"><h2>工作</h2></section>")
        );
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
        )
        .unwrap_err();
        assert!(err.contains("b1"), "报错要把正确的块 id 列出来：{err}");

        let huge = "x".repeat(7000);
        let err = call(
            "set_page_layout",
            &json!({ "parts": { "b1": huge } }),
            &ctx,
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

        call("set_page_layout", &json!({ "reset": true }), &ctx).unwrap();
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
        let text = call("list_people", &json!({}), &ctx).unwrap();
        assert!(text.contains("张三") && text.contains("李四"), "{text}");
        assert!(text.contains("← 当前"), "当前那位要标出来：{text}");
        assert!(text.contains("2 份简历"), "{text}");
        assert!(out.actions.lock().unwrap().is_empty(), "只读工具不该产出指令");

        let text = call("open_person", &json!({ "personId": "p2" }), &ctx).unwrap();
        assert!(text.contains("李四"), "要用人名回话，用户才认得出：{text}");
        let text = call("open_resume", &json!({ "resumeId": "r2" }), &ctx).unwrap();
        assert!(text.contains("English"), "{text}");

        let actions = out.actions.lock().unwrap().clone();
        assert_eq!(actions[0], json!({ "type": "openPerson", "personId": "p2" }));
        // openResume 要带上 personId：前端按「人 + 简历」定位，缺一个就定不到位
        assert_eq!(
            actions[1],
            json!({ "type": "openResume", "personId": "p1", "resumeId": "r2" })
        );

        // 错的 id 要把可用的一并列出，模型才有下一轮的自纠机会
        let err = call("open_person", &json!({ "personId": "p9" }), &ctx)
            .unwrap_err();
        assert!(err.contains("p1") && err.contains("p2"), "{err}");
        let err = call("open_resume", &json!({ "resumeId": "r9" }), &ctx)
            .unwrap_err();
        assert!(err.contains("r1") && err.contains("r2"), "{err}");

        let before = out.actions.lock().unwrap().len();
        call("open_person", &json!({ "personId": "p1" }), &ctx).unwrap();
        assert_eq!(out.actions.lock().unwrap().len(), before, "原地切换不该再发指令");
    }

    /// 批注的产物形状：id 与时间戳必须在 Rust 生成，前端要做去重。
    #[test]
    fn note_carries_its_own_id() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        call(
            "note_on_page",
            &json!({ "text": "技能这一栏压成一行后字号会掉到 8px", "blockId": "b2" }),
            &ctx,
        )
        .unwrap();
        call("note_on_page", &json!({ "text": "整页留白偏多" }), &ctx).unwrap();

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
    }

    /// 批注 2 条是硬限，第 3 条必须报错。
    ///
    /// 静默丢会让模型以为话已经说出去了，总结里还会说「我已经在纸面上标注了」——
    /// 用户那边什么都没有，而且不会再被提醒。
    #[test]
    fn notes_are_hard_limited_per_turn() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        call("note_on_page", &json!({ "text": "第一条" }), &ctx).unwrap();
        call("note_on_page", &json!({ "text": "第二条" }), &ctx).unwrap();
        let err = call("note_on_page", &json!({ "text": "第三条" }), &ctx)
            .unwrap_err();
        assert!(err.contains("2 条"), "要说清上限是几条：{err}");
        assert_eq!(out.actions.lock().unwrap().len(), 2, "被拒的那条不能留下指令");

        // 名额记在出站口上：换一个 Outbox 就是新的一轮，不需要额外重置
        let fresh = Outbox::default();
        let next = ctx_of(&ws, &fresh);
        call("note_on_page", &json!({ "text": "新一轮的批注" }), &next).unwrap();
        assert_eq!(fresh.actions.lock().unwrap().len(), 1);
    }

    #[test]
    fn note_rejects_bad_input_without_side_effects() {
        let ws = sample_ws();
        let out = Outbox::default();
        let ctx = ctx_of(&ws, &out);

        let err = call(
            "note_on_page",
            &json!({ "text": "贴错块", "blockId": "nope" }),
            &ctx,
        )
        .unwrap_err();
        assert!(err.contains("b1"), "报错要列出这一页真实的块：{err}");

        let long = "字".repeat(201);
        let err = call("note_on_page", &json!({ "text": long }), &ctx).unwrap_err();
        assert!(err.contains("上限"), "{err}");
        assert!(call("note_on_page", &json!({}), &ctx)
            .unwrap_err()
            .contains("不能为空"));

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
        )
        .unwrap_err();
        assert!(err.contains("options"), "{err}");
    }
}
