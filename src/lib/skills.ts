/**
 * 技能库：`prompts/skills/*.md` 在构建时被 glob 进来，随 bundle 一起硬编码。
 *
 * 为什么放前端 glob 而不是 Rust 里读目录：
 *   - 打包后没有源码目录可读，`include_str!` 又要求文件名写死；
 *   - Vite 的 glob 是编译期展开的，新增一个 .md 文件会自动进产物，不用改代码。
 */
const files = import.meta.glob("../../prompts/skills/*.md", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>;

/** README 是给人看的目录说明，不进提示词 */
const isDoc = (path: string) => /README\.md$/i.test(path);

const nameOf = (path: string) => path.split("/").pop()?.replace(/\.md$/i, "") ?? "skill";

export interface Skill {
  name: string;
  body: string;
}

export const SKILLS: Skill[] = Object.entries(files)
  .filter(([path]) => !isDoc(path))
  .map(([path, body]) => ({ name: nameOf(path), body: body.trim() }))
  .sort((a, b) => a.name.localeCompare(b.name));

/** 拼成一段附加在 system 提示词后面的文本；没有技能时返回空串 */
export function skillsPrompt(): string {
  if (!SKILLS.length) return "";
  const blocks = SKILLS.map((skill) => `### ${skill.name}\n${skill.body}`);
  return ["## 可用技能（需要时按其中的步骤执行）", ...blocks].join("\n\n");
}
