import { invoke } from "@tauri-apps/api/core";
import type { Origin, PageMeasure, Person, ResumePage, Workspace } from "@/types";
import { aiReady, slotAi } from "@/lib/ai";

/**
 * 把一条输入交给「页排版 agent」。
 *
 * 为什么是「交出去」而不是「等结果」：
 *   排版现在是常驻的后台工作 —— 内容一变就该重看一遍，而不是等用户点某个按钮。
 *   一旦变成常驻，调用方就不该拿着 Promise 干等：它可能中途被更多输入打断，
 *   也可能连着跑好几轮。
 *
 * 结果走 `layout:applied` 事件回来，由 provider 统一落进 store。
 * 同一页的并发由后端信箱负责：同时只会有一个消费者，中途来的输入攒到下一轮。
 *
 * 一个页面一条会话（key 由 Rust 侧拼 `layout:<resumeId>:<pageId>`），
 * 所以多轮之间它有记忆——记得上一轮为什么这么排，不会反复横跳。
 */
export async function submitLayout(input: {
  person: Person;
  resume: { id: string };
  page: ResumePage;
  state: Workspace;
  /** change = 内容刚变了；request = 用户或会话 agent 明确提出重排 */
  kind: "change" | "request";
  text: string;
  /**
   * 这一页此刻的实测值。只在本次提交里有效 —— 排版侧靠它判断「压到几页了」，
   * 落库没有意义：下次启动时它已经是个会骗人的旧数。
   */
  measure?: PageMeasure | null;
  /** 本轮原件页数（会话侧 `chat_send` 回传、存内存），排版侧据此决定要不要沿用 */
  origin?: Origin | null;
}): Promise<boolean> {
  // 排版是独立用途：它可以用一个便宜快模型，不必占着会话那把
  const ai = slotAi("layout");
  if (!aiReady(ai)) return false;

  await invoke("layout_submit", {
    args: {
      personId: input.person.id,
      resumeId: input.resume.id,
      pageId: input.page.id,
      // 工具层只读镜像：剥掉会话历史，排版 agent 不需要
      workspace: {
        version: input.state.version,
        ui: input.state.ui,
        people: input.state.people.map((item) => ({ ...item, messages: [] })),
      },
      baseUrl: ai.baseUrl,
      apiKey: ai.apiKey,
      model: ai.model,
      temperature: ai.temperature,
      reasoning: ai.reasoning ?? null,
      contextWindow: ai.contextWindow ?? null,
      measure: input.measure ?? null,
      origin: input.origin ?? null,
    },
    note: { kind: input.kind, text: input.text },
  });
  return true;
}
