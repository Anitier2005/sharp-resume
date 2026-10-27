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
pub mod image;
pub mod layout;
pub mod page;
pub mod read;

use std::path::Path;
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
    if list.len() < 16 {
        list.push(action);
    }
    Ok(())
}

/// 把一份新材料放进出站队列。
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
];

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

        // 会话 agent：能读、能转交、能转图，但不能自己动版面
        assert!(session.contains(&"request_layout"));
        assert!(session.contains(&"convert_image"));
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
        assert!(!readers.contains(&"set_page_layout"));
        assert!(!readers.contains(&"convert_image"));
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
