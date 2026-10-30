//! docx / pptx / xlsx 的带格式解析。
//!
//! 三种格式都是 zip + XML（OOXML），所以共用同一套「解压 + quick-xml」：
//!   - docx：`word/document.xml` → `w:p` / `w:pPr/w:jc` / `w:rPr/w:sz`(半磅) / `w:b` / `w:tbl`
//!   - pptx：`ppt/slides/slideN.xml` → `a:p` / `a:rPr@sz`(1/100 磅) / `a:t` / `ph type="title"`
//!   - xlsx：`xl/sharedStrings.xml` + `xl/worksheets/sheetN.xml` → 单元格网格
//!
//! zip 读取不引 `zip` crate：`flate2`（已在锁文件里）解 deflate，
//! 中央目录自己走一遍即可 —— 一个只读的 zip 解析器不到百行。
//!
//! 契约：`ext` 是 `docx|pptx|xlsx`，输出 `DocOutline`；读不出返回 `None`。

use super::DocOutline;

pub fn parse(_ext: &str, _bytes: &[u8]) -> Option<DocOutline> {
    // 尚未实现：返回 None，让调用方说「这个格式还读不出结构」，而不是给一个空大纲
    None
}
