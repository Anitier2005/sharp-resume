import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkBreaks from "remark-breaks";
import styles from "./markdown.module.css";

/**
 * Markdown 渲染：react-markdown（CommonMark 全量）+ remark-gfm（表格、任务列表、
 * 删除线、自动链接、脚注）+ remark-breaks（单换行即换行）。
 *
 * 为什么不再手写：模型实际会用的语法比「粗体+列表」多得多 —— 表格、任务列表、
 * 嵌套列表、脚注、嵌套引用、紧凑/松散列表的差异，手写解析器每补一个就多一处
 * 边界 bug。这里要的是「模型写什么就正确渲染什么」。
 *
 * 安全：react-markdown 不解析原始 HTML（默认跳过，只当文本），全程生成 React
 * 元素、没有 dangerouslySetInnerHTML，所以不需要额外的 sanitizer。
 *
 * remark-breaks 是刻意的：CommonMark 里段内换行等于空格，但模型常写
 * 「一、…\n二、…\n三、…」，不保留换行会糊成一整段。
 */

const remarkPlugins = [remarkGfm, remarkBreaks];

/** 只补一层 class 钩子，样式全在 module.css 里用后代选择器写。
    这里不展开 { ...props }：react-markdown 会往组件里塞一个 node 字段，
    展开到 DOM 上就是一个非法属性。 */
const components: Components = {
  table: ({ children }) => (
    <div className={styles.tableWrap}>
      <table>{children}</table>
    </div>
  ),
  a: ({ href, title, children }) => (
    <a href={href} title={title} target="_blank" rel="noreferrer">
      {children}
    </a>
  ),
};

export function Markdown({ text, className }: { text: string; className?: string }) {
  return (
    // data-selectable：全局禁选了，这一块要能选、能复制（Ctrl+A 也只选这一块）
    <div className={className ? `${styles.md} ${className}` : styles.md} data-selectable>
      <ReactMarkdown remarkPlugins={remarkPlugins} components={components}>
        {text}
      </ReactMarkdown>
    </div>
  );
}
