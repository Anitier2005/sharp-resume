/**
 * 本地改写规则：只做真实可解释的文本变换，不编造数字。
 * 接入模型后由模型产出，这里保留为离线兜底。
 */
export function rewriteBullet(text: string): string {
  let out = text.trim().replace(/[。；]$/, "");
  out = out.replace(/^负责/, "主导");
  out = out.replace(/，主导/, "，并推动");
  if (!/[，、]/.test(out)) {
    out = `${out}，对最终结果负责`;
  }
  return `${out}。`;
}

export function rewriteSummary(text: string): string {
  const clean = text.trim().replace(/[。]$/, "");
  if (!clean) return "把复杂业务收敛成可复用流程，是过去几年反复在做的一件事。";
  return `${clean}，落到流程与组件上。`;
}

export function keywordCount(text: string): number {
  const words = text
    .replace(/[^\u4e00-\u9fa5a-zA-Z0-9\s]/g, " ")
    .split(/\s+/)
    .filter((word) => word.length > 1);
  return Math.min(24, Math.max(6, words.length));
}
