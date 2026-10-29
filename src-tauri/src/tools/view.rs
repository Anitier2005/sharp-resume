//! 把文件渲染成图片给自己看。
//!
//! 为什么需要：PDF 的文字能提出来，版式提不出来。模型要判断「这一页挤不挤」
//! 「原件怎么排的」，只能看图。图片附件本来就能直接看（前端会把它放进视觉段），
//! 这个工具专门解决**文件**：把 PDF 逐页渲染成图片，附在下一轮消息里，
//! 同时也写进材料池，用户在自己的材料清单里能看到。
//!
//! 非视觉模型不能调：渲染出来它也看不见，白花一秒和多花的 token。
//! 这条判断放在工具里而不是提示词里 —— 提示词挡不住模型的好奇心。

use serde_json::{json, Value};

use super::{push_asset, ToolCtx};
use crate::pdfimage;

pub fn params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "assetId": {
                "type": "string",
                "description": "要看的文件材料 id，从 read_person / read_page 的材料清单里取。只支持 PDF"
            },
            "maxPages": {
                "type": "integer",
                "description": "渲染前几页，默认 2。整份十几页的 PDF 全渲染会挤爆上下文"
            }
        },
        "required": ["assetId"],
        "additionalProperties": false
    })
}

pub fn render(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    if !ctx.out.vision {
        return Err("当前模型看不了图，把 PDF 转成图片没有意义 —— 直接用提取出来的文字".into());
    }
    let root = ctx
        .root
        .ok_or_else(|| "当前环境不支持文件渲染（浏览器调试模式没有数据目录）".to_string())?;
    let asset_id = args
        .get("assetId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let asset = super::image::find_asset(ctx.workspace, asset_id)
        .ok_or_else(|| format!("找不到材料 {asset_id}"))?;
    let stored = asset
        .get("stored")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name = asset.get("name").and_then(Value::as_str).unwrap_or("文件");
    if !name.to_ascii_lowercase().ends_with(".pdf") {
        return Err(format!("{name} 不是 PDF —— 图片材料本身就能直接看，直接引用它即可"));
    }

    let bytes = crate::pool::read_at(root, stored)?;
    let pages = args
        .get("maxPages")
        .and_then(Value::as_u64)
        .unwrap_or(2)
        .clamp(1, pdfimage::MAX_PAGES as u64) as usize;
    let images = pdfimage::pdf_pages(&bytes, pages, pdfimage::DEFAULT_WIDTH)?;
    if images.is_empty() {
        return Err("这份 PDF 一页都没渲染出来".into());
    }

    // 文件名去掉 .pdf 换 .jpg：材料清单里一眼能看出是第几页
    let stem = name
        .rsplit_once('.')
        .map(|(head, _)| head)
        .unwrap_or(name)
        .to_string();
    let mut lines = vec![format!("已渲染《{name}》前 {} 页：", images.len())];
    for (index, page) in images.iter().enumerate() {
        let page_name = format!("{stem}-第{}页.jpg", index + 1);
        let id = format!("a-{}", crate::session::uuid_like());
        let asset = crate::pool::put_at(root, &id, &page_name, &page.jpeg)?;
        // 数据 URL 给「下一轮真的要看见」，资产给「用户能在清单里点开」
        super::push_view(ctx, &pdfimage::data_url(&page.jpeg))?;
        lines.push(format!(
            "- 第 {} 页：![{}](asset:{})",
            index + 1,
            page_name,
            asset.stored
        ));
        push_asset(ctx, asset)?;
    }
    lines.push(String::new());
    lines.push("这些页面的原图已经附在下一轮消息里，可以直接看。".into());
    Ok(lines.join("\n"))
}
