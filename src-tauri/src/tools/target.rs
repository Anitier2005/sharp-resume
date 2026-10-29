//! 目标页数：用户明确说过「压到 N 页」时记下来。
//!
//! 为什么需要落一条 op 而不是让模型自己记住：页数目标要跨轮、跨重启都成立。
//! 模型每轮只看得到历史，用户说「压到一页」可能是十轮之前的事；把它写进
//! `Resume.extra` 之后，每一轮的【页面事实】块都会把结论摆回来（`facts::target_of`）。
//!
//! **只在用户明确说过页数时调**。用户没说就调它，等于替用户定了个目标 ——
//! 模型接着就会为了达成这个自己编的目标去删内容。

use serde_json::{json, Value};

use crate::tools::ToolCtx;

/// 一次能记住的目标页数上限。
///
/// 不是技术限制，是常识限制：一份简历超过十页就不该叫简历了。
/// 放开上限只会让模型顺手把「尽量简短」翻译成一个荒唐的数字。
const MAX_TARGET_PAGES: u32 = 10;

pub fn set_target_pages_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pages": {
                "type": "integer",
                "description": "用户明确要求的目标页数，1–10。用户没说过页数就不要调这个工具；要清除用户之前的要求就不传这个参数"
            }
        },
        "additionalProperties": false
    })
}

/// 记下 / 清除目标页数。
///
/// 产物走 `mutate` 信封（与同轮内容改动同一个出口）：顺序即语义 ——
/// 同一轮里先设目标再改内容，和反过来，前端看到的是不同的两条流水。
pub fn set_target_pages(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let pages = match args.get("pages") {
        // 省略 = 清除。用 null 表达清除会让「没这个键」和「显式 null」两种载荷
        // 走到不同分支，而前端 reducer 只认「有没有 targetPages」
        None | Some(Value::Null) => None,
        Some(raw) => match raw.as_u64() {
            Some(pages) if (1..=MAX_TARGET_PAGES as u64).contains(&pages) => Some(pages as u32),
            _ => {
                return Err(format!(
                    "pages 只能是 1 到 {MAX_TARGET_PAGES} 的整数（收到 {raw}）。要清除目标页数就不要传这个参数"
                ))
            }
        },
    };

    let mut op = json!({
        "type": "resumeTargetPages",
        "personId": ctx.person_id,
        "resumeId": ctx.resume_id,
        "turn": ctx.out.turn,
    });
    if let Some(pages) = pages {
        op["targetPages"] = json!(pages);
    }
    crate::tools::edit::push_op(ctx, op);

    Ok(match pages {
        Some(pages) => format!("已记下：这份简历压到 {pages} 页。"),
        None => "已清除目标页数：不再按页数判达标。".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{edit, Outbox, Scope, ToolCtx};

    fn workspace() -> Value {
        json!({ "people": [{ "id": "p1", "resumes": [{ "id": "r1", "pages": [] }] }] })
    }

    fn with_outbox(run: impl FnOnce(&ToolCtx)) {
        let ws = workspace();
        let out = Outbox::default();
        let ctx = ToolCtx {
            workspace: &ws,
            person_id: "p1",
            resume_id: "r1",
            page_id: "pg1",
            out: &out,
            root: None,
            facts: None,
        };
        run(&ctx);
    }

    #[test]
    fn setting_a_target_emits_one_op() {
        with_outbox(|ctx| {
            let text = set_target_pages(&json!({ "pages": 1 }), ctx).unwrap();
            assert!(text.contains("压到 1 页"), "{text}");

            let envelope = edit::flush_all(ctx.out).expect("要有信封");
            assert_eq!(envelope["type"], "mutate");
            assert_eq!(
                envelope["ops"][0],
                json!({
                    "type": "resumeTargetPages",
                    "personId": "p1",
                    "resumeId": "r1",
                    "targetPages": 1,
                    "turn": 0,
                })
            );
        });
    }

    #[test]
    fn clearing_omits_the_key_instead_of_sending_null() {
        with_outbox(|ctx| {
            let text = set_target_pages(&json!({}), ctx).unwrap();
            assert!(text.contains("已清除目标页数"), "{text}");

            let envelope = edit::flush_all(ctx.out).expect("要有信封");
            let op = &envelope["ops"][0];
            assert_eq!(op["type"], "resumeTargetPages");
            assert!(
                op.get("targetPages").is_none(),
                "清除必须是「没有这个键」：{op}"
            );
        });
    }

    #[test]
    fn illegal_pages_are_rejected_without_leaving_an_op() {
        for bad in [json!({ "pages": 0 }), json!({ "pages": 11 }), json!({ "pages": "1" })] {
            with_outbox(|ctx| {
                let err = set_target_pages(&bad, ctx).unwrap_err();
                assert!(err.contains("pages"), "{err}");
                // 报错不能留下半条指令：模型会以为设成功了
                assert!(edit::flush_all(ctx.out).is_none(), "非法值不该产出 op");
            });
        }
    }

    #[test]
    fn the_tool_is_registered_for_the_session_scope_only() {
        let ws = workspace();
        let out = Outbox::default();
        let ctx = ToolCtx {
            workspace: &ws,
            person_id: "p1",
            resume_id: "r1",
            page_id: "pg1",
            out: &out,
            root: None,
            facts: None,
        };
        assert!(crate::tools::call("set_target_pages", &json!({ "pages": 2 }), &ctx, Scope::Session).is_ok());
        assert!(
            crate::tools::call("set_target_pages", &json!({ "pages": 2 }), &ctx, Scope::Layout).is_err(),
            "排版 agent 不改内容，也不该改目标页数"
        );
    }
}
