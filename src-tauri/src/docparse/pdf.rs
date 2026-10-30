//! PDF 的带格式解析。
//!
//! 现状（待实现）：`pdf_extract` 只能给纯文本，字号与坐标在提取时丢掉了。
//! 要拿回它们，只能自己走 content stream：`Tf` 给字号，`Tm`/`Td` 给位置，
//! 按 y 聚成行、按行首 x 与相对中心判断对齐，按字号聚类分标题层级。
//!
//! 契约：输入 PDF 字节，输出 `DocOutline`；读不出返回 `None`（不要返回空大纲）。

use super::DocOutline;

pub fn parse(_bytes: &[u8]) -> Option<DocOutline> {
    // 尚未实现：先让调用方回落到纯文本路径，别伪装成「解析成功但什么都没有」
    None
}
