import { invoke } from "@tauri-apps/api/core";
import type { Asset } from "@/types";
import { isTauri } from "@/lib/storage";
import { uid } from "@/lib/uid";

/**
 * 材料的存取。
 *
 * 桌面端：文件进 `<数据目录>/pool/`（资产池，按内容寻址，同一份文件只落一份），
 * state 里只留存储名；浏览器（`npm run dev`）：没有文件系统，退化成 data URL 塞进 stored。
 * 两条路对上层是同一个 `resolveUrl()`，界面不用关心自己在哪跑。
 *
 * 为什么不再用 `asset_put`：那套写 `assets/` 目录、按原文件名存，同一张图拖两次就是两份。
 * 资产池把「存在哪」和「谁在用」分开了 —— 引用统计由后端在每次保存后同步。
 */

const IMAGE_EXT = new Set(["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"]);
const extOf = (name: string) => (name.includes(".") ? name.split(".").pop()!.toLowerCase() : "");

/** 已解析过的显示地址：同一个文件不重复读盘，也不重复造 blob URL */
const urlCache = new Map<string, string>();

/** 单张图片上限 12MB：再大就不是身份证照片了，是设计稿 */
const MAX_BYTES = 12 * 1024 * 1024;

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error("读取文件失败"));
    reader.readAsDataURL(file);
  });
}

/** 文件 → 材料。图片归 image，其余归 file（链接走 addLink） */
export async function saveAsset(file: File): Promise<Asset> {
  if (file.size > MAX_BYTES) {
    throw new Error(`「${file.name}」超过 12MB，先压缩再放进来`);
  }
  const ext = extOf(file.name);
  const kind: Asset["kind"] = IMAGE_EXT.has(ext) || file.type.startsWith("image/") ? "image" : "file";
  const id = uid("a");

  if (isTauri()) {
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    // 资产池按内容寻址：同一份文件拖两次只落一份，存储名由后端算（sha256 前 16 位）
    const saved = await invoke<{ stored: string; sha256: string }>("pool_put", {
      id,
      name: file.name,
      data: bytes,
    });
    return { id, kind, name: file.name, stored: saved.stored };
  }

  return { id, kind, name: file.name, stored: await readAsDataUrl(file) };
}

/** 链接型材料：没有文件，stored 就是网址 */
export function addLink(url: string, name?: string): Asset {
  const trimmed = url.trim();
  return {
    id: uid("a"),
    kind: "link",
    name: name?.trim() || trimmed.replace(/^https?:\/\//, "").slice(0, 40),
    stored: trimmed,
  };
}

/**
 * 取一个可以直接放进 <img src> 或 <a href> 的地址。
 * 读不出来返回 null（界面显示占位，不炸）。
 */
export async function resolveUrl(asset: Asset): Promise<string | null> {
  if (asset.kind === "link") return asset.stored;
  if (!isTauri() || asset.stored.startsWith("data:")) return asset.stored;

  const cached = urlCache.get(asset.stored);
  if (cached) return cached;

  try {
    const bytes = await invoke<number[]>("pool_get", { stored: asset.stored });
    const blob = new Blob([new Uint8Array(bytes)]);
    const url = URL.createObjectURL(blob);
    urlCache.set(asset.stored, url);
    return url;
  } catch {
    return null;
  }
}

/**
 * 从 state 里摘掉一份材料后调用：只清显示缓存。
 *
 * **不删文件**。同一个文件可能还被别的页引用着，删不删由后端按引用统计决定
 * （引用归零才挪进 `pool/.trash/<日期>/`）。前端直接删会误伤共用同一张图的页。
 */
export async function dropAsset(asset: Asset): Promise<void> {
  const cached = urlCache.get(asset.stored);
  if (cached) {
    URL.revokeObjectURL(cached);
    urlCache.delete(asset.stored);
  }
}

export const isImageAsset = (asset: Asset) => asset.kind === "image";

/** 第一个图片材料：纸面上当成证件照用 */
export const photoOf = (assets: Asset[] | undefined): Asset | undefined =>
  assets?.find((asset) => asset.kind === "image");
