import { invoke } from "@tauri-apps/api/core";
import type { Asset, Person, ResumePage } from "@/types";
import { resolveUrl } from "@/lib/assets";

/**
 * 成品层：把一页编译成可直接渲染的 HTML。
 *
 * 版面规则全部在 Rust 侧（`src-tauri/src/render`）：生成器按块拼结构、
 * 预设 CSS 提供样式、手写片段优先替换。前端只做两件事 ——
 * 调一次编译、把 `asset:` 占位换成真地址。
 *
 * 为什么不在前端拼：预览、打印、Word 导出必须共用同一份成品，
 * 否则三处各排各的，改装版等于要改三遍。
 */
export interface CompiledPage {
  html: string;
  css: string;
  /** structured = 生成器拼的；manual = agent 整页手写 */
  mode: "structured" | "manual";
  preset: string;
  /** 生成过程中发现的问题：未知块、死引用、找不到的图片 */
  warnings: string[];
}

export async function compilePage(
  page: ResumePage,
  person: Person,
): Promise<CompiledPage | null> {
  return invoke<CompiledPage>("compile_page", { page, person });
}

function findAsset(person: Person, id: string): Asset | undefined {
  const own = person.assets?.find((asset) => asset.id === id);
  if (own) return own;
  for (const item of person.items) {
    const hit = item.assets?.find((asset) => asset.id === id);
    if (hit) return hit;
  }
  return undefined;
}

/**
 * 把 `asset:<id>` 换成能直接加载的地址。
 *
 * 编译层只输出占位符：真地址怎么来（材料池 / data URL）
 * 是前端的事，后端不该知道 —— 它也不该把图片字节塞进 HTML。
 */
export async function resolveAssetRefs(html: string, person: Person): Promise<string> {
  const ids = new Set<string>();
  // **id 里带点**：资产 id 就是内容寻址的存储名（`92c40bfc645989ee.jpg`），
  // 早先写成 `/asset:([\w-]+)/` 会在第一个点处截断，取到 `92c40bfc645989ee`
  // 这个不存在的 id → 换不到地址 → 纸上是一张破图（照片一直显示不出来的原因）。
  // 只允许「短后缀」吃进点，免得把句末的句号也当成 id 的一部分。
  for (const match of html.matchAll(/asset:([\w-]+(?:\.[A-Za-z0-9]{2,5})?)/g)) {
    ids.add(match[1]);
  }
  if (!ids.size) return html;

  const urls = new Map<string, string>();
  await Promise.all(
    [...ids].map(async (id) => {
      const asset = findAsset(person, id);
      if (!asset) return;
      const url = await resolveUrl(asset);
      if (url) urls.set(id, url);
    }),
  );

  // split/join 而不是 replace：同一个 id 可能出现多次
  let out = html;
  for (const [id, url] of urls) {
    out = out.split(`asset:${id}`).join(url);
  }
  return out;
}
