import { describe, expect, it } from "vitest";
import { capabilityLine, checkedAgo, type Capability } from "./capability";

/**
 * 这几条守的是「界面不许替程序编能力」：
 * 结论只有三态，措辞只有一套，而且每一句都必须带「实测」或「没测出来」——
 * 用户看到的永远是测出来的东西，不是我们猜的。
 */
const cap = (patch: Partial<Capability> = {}): Capability => ({
  state: "unknown",
  reason: "no-probe",
  checkedAt: null,
  ...patch,
});

describe("capabilityLine", () => {
  it("支持：说清图片会直接发过去", () => {
    const text = capabilityLine(cap({ state: "supported", reason: "none", checkedAt: 1 }));
    expect(text).toContain("实测支持");
    expect(text).toContain("直接发给它");
  });

  it("协议层拒图：说清原因，并交代图片会走 OCR", () => {
    const text = capabilityLine(cap({ state: "unsupported", reason: "protocol-reject" }));
    expect(text).toContain("实测不支持");
    expect(text).toContain("拒收图像");
    expect(text).toContain("OCR");
  });

  it("没测出来：既不说能看也不说不能看", () => {
    const text = capabilityLine(cap({ reason: "no-probe" }));
    expect(text).toContain("没测出来");
    expect(text).not.toContain("不支持图像输入");
    expect(text).not.toContain("支持图像输入");
  });

  it("额度用完不算能力问题，措辞里不许出现「不支持图像输入」", () => {
    const text = capabilityLine(cap({ reason: "quota" }));
    expect(text).toContain("没测出来");
    expect(text).toContain("额度");
    // 「既不当支持也不当不支持」这句本身含「不支持」，所以要钉的是那个具体结论
    expect(text).not.toContain("不支持图像输入");
  });

  it("从没读过（null）也要有一句话", () => {
    expect(capabilityLine(null)).toContain("还没测过");
  });
});

describe("checkedAgo", () => {
  const now = 1_800_000_000_000;

  it("从没测过就没有时间", () => {
    expect(checkedAgo(null, now)).toBe("");
    expect(checkedAgo(cap(), now)).toBe("");
  });

  it("分钟 / 小时 / 天三档", () => {
    expect(checkedAgo(cap({ checkedAt: now / 1000 - 30 }), now)).toBe("刚刚测过");
    expect(checkedAgo(cap({ checkedAt: now / 1000 - 600 }), now)).toBe("10 分钟前测过");
    expect(checkedAgo(cap({ checkedAt: now / 1000 - 7200 }), now)).toBe("2 小时前测过");
    expect(checkedAgo(cap({ checkedAt: now / 1000 - 259200 }), now)).toBe("3 天前测过");
  });
});
