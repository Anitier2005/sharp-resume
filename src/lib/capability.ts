import { invoke } from "@tauri-apps/api/core";

/**
 * 模型能力的唯一读取口（前端）。
 *
 * 这里**不做任何判定**：能不能看图由 Rust 侧 `capability::resolve` 说，
 * 前端只负责显示和「帮我测一次」。以前这里是一个手勾开关，勾的是用户的印象，
 * 不是链路的实测 —— 那条路的后果是「勾了但看不图」，或者「能看却一直没发图」。
 */
export type CapabilityState = "supported" | "unsupported" | "unknown";

export type CapabilityReason =
  | "none"
  | "protocol-reject"
  | "auth"
  | "quota"
  | "bad-request"
  | "offline"
  | "timeout"
  | "no-probe";

export interface Capability {
  state: CapabilityState;
  reason: CapabilityReason;
  /** unix 秒；从没测过是 null */
  checkedAt: number | null;
}

export interface CapabilityConf {
  baseUrl: string;
  model: string;
  apiKey: string;
}

/** 读上次的结论（不发请求）。 */
export function capabilityOf(conf: CapabilityConf): Promise<Capability> {
  return invoke<Capability>("capability_of", { conf });
}

/** 真的测一次：画随机色块图问它颜色，答对了才记成 supported。 */
export function capabilityTest(conf: CapabilityConf): Promise<Capability> {
  return invoke<Capability>("capability_test", { conf });
}

const REASON_TEXT: Record<CapabilityReason, string> = {
  none: "",
  "protocol-reject": "模型在协议层明确拒收图像",
  auth: "这组配置的密钥不对或没有权限",
  quota: "余额或免费额度用完了",
  "bad-request": "请求本身有问题（配置或参数）",
  offline: "网络没连上",
  timeout: "超时了",
  "no-probe": "答错了，得不出结论",
};

/** 状态栏/设置页共用的一句话。永远包含「实测」两个字：这是测出来的，不是猜的。 */
export function capabilityLine(cap: Capability | null): string {
  if (!cap) return "还没测过";
  if (cap.state === "supported") return "实测支持图像输入：图片会直接发给它";
  if (cap.state === "unsupported") {
    const why = REASON_TEXT[cap.reason];
    return why
      ? `实测不支持图像输入（${why}）：图片会先走 OCR 转文字`
      : "实测不支持图像输入：图片会先走 OCR 转文字";
  }
  const why = REASON_TEXT[cap.reason];
  return why ? `没测出来（${why}）：既不当支持也不当不支持` : "没测出来";
}

/** 「3 天前测过」——界面上要能区分「从没测过」和「测过但没结论」 */
export function checkedAgo(cap: Capability | null, now = Date.now()): string {
  if (!cap || cap.checkedAt === null) return "";
  const seconds = Math.max(0, Math.floor(now / 1000 - cap.checkedAt));
  if (seconds < 60) return "刚刚测过";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} 分钟前测过`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} 小时前测过`;
  return `${Math.floor(seconds / 86400)} 天前测过`;
}
