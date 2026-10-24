/**
 * 拼接类名，过滤掉 false / null / undefined。
 *
 * CSS Modules 的 `styles.x` 在类名不存在时是 undefined，条件类名容易拼出
 * `"a  b"` 这种带空串的结果；用这个函数统一处理，也让调用处更好读：
 *   cx(styles.item, active && styles.itemOn)
 */
export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}
