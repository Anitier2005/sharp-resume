import { ACCENTS, TEXT_SCALES, type AccentKey } from "@/types";
import { isHexColor } from "@/lib/color";
import { ColorPicker } from "@/components/ui/color-picker";
import { Tooltip } from "@/components/ui";
import { cx } from "@/lib/cx";
import styles from "./settings.module.css";

/**
 * 外观设置（独立窗口）。
 *
 * 全部受控：值住在窗口根上，改一项就同时更新**这个窗自己**和主窗口 ——
 * 换强调色时如果只有主窗口变色、设置窗还是旧色，那就等于没有预览。
 *
 * 没有「确定」也没有底栏：每一项都是即时生效，关窗就是结束。
 */
export interface LookSettingsProps {
  accent: AccentKey;
  /** 自定义色（非空时压过 accent） */
  hex: string;
  mode: "light" | "dark";
  /** 用户档位；空字符串 = 跟随系统 */
  scale: string;
  /** 系统给出的倍率，用于「自动」那一档的提示 */
  systemScale: number;
  onAccent: (key: AccentKey) => void;
  onHex: (hex: string) => void;
  onMode: (mode: "light" | "dark") => void;
  onScale: (scale: string) => void;
}

export function LookSettings({
  accent,
  hex,
  mode,
  scale,
  systemScale,
  onAccent,
  onHex,
  onMode,
  onScale,
}: LookSettingsProps) {
  return (
    <div className={styles.page}>
      <section className={styles.group}>
        <span className={styles.groupTitle}>明暗</span>
        <div className={styles.segRow}>
          {(["light", "dark"] as const).map((item) => (
            <button
              key={item}
              type="button"
              className={styles.segBtn}
              aria-pressed={mode === item}
              onClick={() => onMode(item)}
            >
              {item === "light" ? "浅色" : "深色"}
            </button>
          ))}
        </div>
      </section>

      <section className={styles.group}>
        <span className={styles.groupTitle}>强调色</span>
        <div className={styles.swatches}>
          {ACCENTS.map((item) => (
            <Tooltip key={item.key} label={item.label}>
              <button
                type="button"
                className={cx(styles.sw, !hex && accent === item.key && styles.swOn)}
                aria-pressed={!hex && accent === item.key}
                onClick={() => onAccent(item.key)}
              >
                <i style={{ background: item.color }} />
              </button>
            </Tooltip>
          ))}
          {/* 自定义色：色块本身就是取色器入口，HEX 输入在浮层里 */}
          <ColorPicker
            value={isHexColor(hex) ? hex : "#3b50b4"}
            onChange={onHex}
            label="自定义强调色"
          />
        </div>
      </section>

      <section className={styles.group}>
        <span className={styles.groupTitle}>界面字号</span>
        <div className={styles.segRow}>
          <button
            type="button"
            className={styles.segBtn}
            aria-pressed={scale === ""}
            title={`跟随系统（${Math.round(systemScale * 100)}%）`}
            onClick={() => onScale("")}
          >
            自动
          </button>
          {TEXT_SCALES.map((item) => (
            <button
              key={item.key}
              type="button"
              className={styles.segBtn}
              aria-pressed={scale === String(item.value)}
              onClick={() => onScale(String(item.value))}
            >
              {item.label}
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}
