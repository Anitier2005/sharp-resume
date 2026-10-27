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
pub mod layout;
pub mod page;
pub mod read;

use serde_json::{json, Value};

/// 工具执行时能看到的东西：只读快照 + 当前定位 + 待回传的界面指令。
pub struct ToolCtx<'a> {
    pub workspace: &'a Value,
    pub person_id: &'a str,
    pub resume_id: &'a str,
    pub page_id: &'a str,
    /// 页面控制类工具产出的指令。本轮结束后随回复一起回传前端执行。
    /// 用 Mutex 是因为工具执行发生在 async 流程里，且将来可能并行。
    pub actions: &'a std::sync::Mutex<Vec<Value>>,
}

/// 把一条界面指令放进出站队列。
pub fn push_action(ctx: &ToolCtx, action: Value) -> Result<(), String> {
    let mut list = ctx
        .actions
        .lock()
        .map_err(|_| "界面指令队列不可用".to_string())?;
    // 上限保护：模型偶尔会在同一轮里反复调 set_ui，没必要把几十条都回传
    if list.len() < 16 {
        list.push(action);
    }
    Ok(())
}

/// 工具归属：统一的注册表，但不同 agent 看到的能力不同。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// 两个 agent 都能用
    All,
    /// 只有会话 agent
    Session,
    /// 只有页排版 agent
    Layout,
}

pub struct Tool {
    pub name: &'static str,
    /// 写给模型看的：什么时候该用它。措辞直接决定它会不会乱调。
    pub description: &'static str,
    pub params: fn() -> Value,
    pub run: fn(&Value, &ToolCtx) -> Result<String, String>,
    pub scope: Scope,
}

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "read_person",
        description: "读取当前候选人：所有字段、有几份简历、经历库有多少条。\
                      用户问「我的信息里有什么」「还缺什么」时先调这个。",
        params: read::no_params,
        run: read::read_person,
        scope: Scope::All,
    },
    Tool {
        name: "list_pages",
        description: "列出当前这份简历的所有页（页码、页名、页面 id）并标出当前页。\
                      不确定「一共几页」「第几页叫什么」时必须调它，不要猜。",
        params: read::no_params,
        run: read::list_pages,
        scope: Scope::All,
    },
    Tool {
        name: "read_page",
        description: "读取某一页的完整结构化内容：页名、页头字段、每个分区的标题与条目\
                      （含本页措辞）。不传 pageId 就是当前页。\
                      任何涉及「这一页写了什么」的问题都必须先读，不许凭记忆回答。",
        params: read::page_params,
        run: read::read_page,
        scope: Scope::All,
    },
    Tool {
        name: "read_library",
        description: "读取经历库条目（字段 + 详实记录）。用户提到「那段经历」「库里的材料」\
                      或要改写却没有足够细节时调它。",
        params: read::library_params,
        run: read::read_library,
        scope: Scope::All,
    },
    /* ---------- 页面控制（只影响「看到什么」，不改内容） ---------- */
    Tool {
        name: "open_page",
        description: "把用户的界面切到某一页。用户说「打开第二页」「切到英文那页」「回到刚才那页」时用它。\
                      不要凭记忆猜页码：不确定就先 list_pages。",
        params: page::open_page_params,
        run: page::open_page,
        scope: Scope::Session,
    },
    Tool {
        name: "set_view",
        description: "切换编辑 / 预览。用户说「看看成品长什么样」「排版效果如何」用 preview；\
                      说「我要改」「回到表单」用 edit。只在用户明确要看/要改时切，别自作主张。",
        params: page::view_params,
        run: page::set_view,
        scope: Scope::Session,
    },
    Tool {
        name: "set_open_pages",
        description: "决定右侧快捷标签显示哪几页、什么顺序（只影响快捷栏，不删页也不改内容）。\
                      用户说「把这三页放到标签上」「只留英文页的标签」时用它，pageIds 从 list_pages 取。",
        params: page::open_pages_params,
        run: page::set_open_pages,
        scope: Scope::Session,
    },
    Tool {
        name: "ask_user",
        description: "向用户发一张填写表单（单行/多行文本、单选、多选、是/否、数字、日期）。\
                      信息不足、需要用户拍板、或要一次性问清多件事时用它，不要靠猜；\
                      能只问一句就别开表单（直接问更自然），要问三件以上才用表单。",
        params: ask::params,
        run: ask::ask,
        scope: Scope::Session,
    },
    Tool {
        name: "set_ui",
        description: "改界面外观：主题色、明暗、字号、左右栏宽度与显隐。\
                      仅当用户明确要求换配色 / 字太小 / 栏太窄时才调；用户没提就别动。\
                      改完要把结果说清楚，让用户知道变了什么。",
        params: page::ui_params,
        run: page::set_ui,
        scope: Scope::Session,
    },
    Tool {
        name: "request_layout",
        description: "把「这一页要怎么排」的要求交给该页的排版 agent（它负责留白与疏密，                      不碰内容）。用户抱怨太挤 / 太散 / 装不下时用它，别自己调样式。",
        params: layout::request_layout_params,
        run: layout::request_layout,
        scope: Scope::Session,
    },
    Tool {
        name: "set_page_style",
        description: "设置当前页的排版参数：疏密、页边距、页头对齐。                      这是排版 agent 的专属工具；改完要说明你为什么这么排。",
        params: layout::page_style_params,
        run: layout::set_page_style,
        scope: Scope::Layout,
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
    tool.scope == Scope::All || tool.scope == scope
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

    #[test]
    fn unknown_tool_lists_alternatives() {
        let ws = json!({ "people": [] });
        let sink = std::sync::Mutex::new(Vec::new());
        let ctx = ToolCtx { workspace: &ws, person_id: "p", resume_id: "r", page_id: "g", actions: &sink };
        let err = call("read_everything", &json!({}), &ctx, Scope::Session).unwrap_err();
        assert!(err.contains("read_page"), "{err}");
    }

    fn sample_ws() -> Value {
        json!({
            "people": [{
                "id": "p1",
                "resumes": [{
                    "id": "r1",
                    "pages": [
                        { "id": "pg1", "label": "第 1 页" },
                        { "id": "pg2", "label": "English" }
                    ]
                }]
            }]
        })
    }

    fn ctx_of<'a>(ws: &'a Value, sink: &'a std::sync::Mutex<Vec<Value>>) -> ToolCtx<'a> {
        ToolCtx { workspace: ws, person_id: "p1", resume_id: "r1", page_id: "pg1", actions: sink }
    }

    #[test]
    fn page_tools_emit_actions_not_state_changes() {
        let ws = sample_ws();
        let sink = std::sync::Mutex::new(Vec::new());
        let ctx = ctx_of(&ws, &sink);

        let text = call("open_page", &json!({ "index": 2 }), &ctx, Scope::Session).unwrap();
        assert!(text.contains("English"), "{text}");

        call("set_view", &json!({ "view": "preview" }), &ctx, Scope::Session).unwrap();
        call("set_open_pages", &json!({ "pageIds": ["pg2"] }), &ctx, Scope::Session).unwrap();
        call("set_ui", &json!({ "mode": "dark", "sideW": 9999 }), &ctx, Scope::Session).unwrap();

        let actions = sink.lock().unwrap().clone();
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
        let sink = std::sync::Mutex::new(Vec::new());
        let ctx = ctx_of(&ws, &sink);

        assert!(call("open_page", &json!({ "pageId": "pg-nope" }), &ctx, Scope::Session)
            .unwrap_err()
            .contains("没有 id"));
        assert!(call("open_page", &json!({}), &ctx, Scope::Session).unwrap_err().contains("哪一页"));
        assert!(call("set_view", &json!({ "view": "fullscreen" }), &ctx, Scope::Session).is_err());
        assert!(call("set_ui", &json!({ "accentHex": "blue" }), &ctx, Scope::Session).is_err());
        assert!(call("set_ui", &json!({}), &ctx, Scope::Session).is_err());
        // 失败不该留下任何指令
        assert!(sink.lock().unwrap().is_empty());
    }

    #[test]
    fn scope_keeps_layout_tools_out_of_session_agent() {
        let ws = sample_ws();
        let sink = std::sync::Mutex::new(Vec::new());
        let ctx = ctx_of(&ws, &sink);

        // 会话 agent 调不了排版写工具
        let err = call("set_page_style", &json!({ "density": "compact" }), &ctx, Scope::Session)
            .unwrap_err();
        assert!(err.contains("不能用"), "{err}");

        // 排版 agent 调不了切页 / 改界面
        for name in ["open_page", "set_ui", "set_view", "ask_user"] {
            let err = call(name, &json!({}), &ctx, Scope::Layout).unwrap_err();
            assert!(err.contains("不能用"), "{name}: {err}");
        }

        // 两边都能读
        assert!(call("list_pages", &json!({}), &ctx, Scope::Layout).is_ok());
        assert!(call("list_pages", &json!({}), &ctx, Scope::Session).is_ok());
    }

    #[test]
    fn layout_tool_merges_style_instead_of_replacing() {
        let mut ws = sample_ws();
        ws["people"][0]["resumes"][0]["pages"][0]["style"] =
            json!({ "density": "roomy", "margin": 1.1 });
        let sink = std::sync::Mutex::new(Vec::new());
        let ctx = ctx_of(&ws, &sink);

        call("set_page_style", &json!({ "margin": 0.9 }), &ctx, Scope::Layout).unwrap();
        let actions = sink.lock().unwrap().clone();
        let style = &actions[0]["style"];
        // 没传的 density 不能被清掉
        assert_eq!(style["density"], json!("roomy"));
        assert_eq!(style["margin"], json!(0.9));
        assert_eq!(actions[0]["type"], json!("patchPageStyle"));
    }

    #[test]
    fn ask_user_builds_a_form_and_rejects_bad_questions() {
        let ws = sample_ws();
        let sink = std::sync::Mutex::new(Vec::new());
        let ctx = ctx_of(&ws, &sink);

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

        let actions = sink.lock().unwrap().clone();
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
