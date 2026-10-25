import { useEffect, useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { Close } from "@/components/ui/icons";
import { ACCENTS, TEXT_SCALES } from "@/types";
import { isHexColor } from "@/lib/color";
import { loadAi, saveAi, testAi, type AiSettings } from "@/lib/ai";
import { MODELS, modelById } from "@/lib/model-config";
import { cx } from "@/lib/cx";
import shared from "@/components/ui/shared.module.css";
import styles from "./sheet.module.css";

type Tab = "model" | "look";

/**
 * 统一设置：模型与外观收在一个地方。
 *
 * 模型配置写在 localStorage（`sharp-resume:ai:v1`），不进工作区、不随简历同步；
 * 模型清单与能力参数来自 `config/models.json`，打包时已硬编码。
 *
 * 内容块按 open 挂载/卸载 —— 每次打开都拿到最新配置，也省掉一堆「同步草稿」的副作用。
 */
export function SettingsSheet({
  open,
  onClose,
  initialTab = "model",
}: {
  open: boolean;
  onClose: () => void;
  initialTab?: Tab;
}) {
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className={shared.veil}
      data-print="hide"
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <SheetBody onClose={onClose} initialTab={initialTab} />
    </div>
  );
}

function SheetBody({ onClose, initialTab }: { onClose: () => void; initialTab: Tab }) {
  const { state, dispatch, textScale, systemTextScale, setTextScale } = useWorkspace();
  const [tab, setTab] = useState<Tab>(initialTab);
  const [ai, setAi] = useState<AiSettings>(loadAi);
  const [hex, setHex] = useState(state.ui.accentHex ?? "");
  const [probe, setProbe] = useState<{ tone: "idle" | "ok" | "fail"; text: string }>({
    tone: "idle",
    text: "",
  });

  const spec = modelById(ai.model);

  const write = (patch: Partial<AiSettings>) => {
    const next = { ...ai, ...patch };
    setAi(next);
    saveAi(next);
  };

  const pickModel = (id: string) => {
    const model = modelById(id);
    write({
      model: id,
      baseUrl: model?.baseUrl ?? ai.baseUrl,
      contextWindow: model?.contextWindow,
      reasoning: model?.reasoning[0],
      temperature: model?.defaultTemperature ?? ai.temperature,
    });
  };

  const runProbe = async () => {
    setProbe({ tone: "idle", text: "正在连接…" });
    try {
      setProbe({ tone: "ok", text: `连上了：${await testAi(ai)}` });
    } catch (error) {
      setProbe({ tone: "fail", text: error instanceof Error ? error.message : String(error) });
    }
  };

  const applyHex = (value: string) => {
    setHex(value);
    if (isHexColor(value)) {
      dispatch({ type: "setAccent", accent: state.ui.accent, accentHex: value });
    }
  };

  return (
    <div className={styles.box} role="dialog" aria-modal="true" aria-label="设置">
      <header className={styles.head}>
        <div className={styles.tabs} role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "model"}
            onClick={() => setTab("model")}
          >
            模型
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "look"}
            onClick={() => setTab("look")}
          >
            外观
          </button>
        </div>
        <span className={styles.spacer} />
        <button
          type="button"
          className={shared.overlayClose}
          aria-label="关闭"
          onClick={onClose}
        >
          <Close size={11} />
        </button>
      </header>

      <div className={styles.body}>
        {tab === "model" ? (
          <>
            <div className={styles.group}>
              <span className={styles.groupTitle}>模型</span>
              <label className={shared.fld}>
                <span className={shared.fldK}>选择已配置的模型</span>
                <select
                  className={styles.select}
                  value={ai.model}
                  onChange={(event) => pickModel(event.target.value)}
                >
                  {MODELS.map((item) => (
                    <option value={item.id} key={item.id}>
                      {item.label} · {item.provider}
                    </option>
                  ))}
                </select>
              </label>
              {spec?.note && <span className={styles.hint}>{spec.note}</span>}
            </div>

            <div className={styles.group}>
              <span className={styles.groupTitle}>连接</span>
              <label className={shared.fld}>
                <span className={shared.fldK}>Base URL</span>
                <input
                  value={ai.baseUrl}
                  spellCheck={false}
                  onChange={(event) => write({ baseUrl: event.target.value })}
                />
              </label>
              <label className={shared.fld}>
                <span className={shared.fldK}>API Key · 只存在本机，不进工作区</span>
                <input
                  type="password"
                  value={ai.apiKey}
                  spellCheck={false}
                  placeholder="sk-…"
                  onChange={(event) => write({ apiKey: event.target.value })}
                />
              </label>
            </div>

            <div className={styles.group}>
              <span className={styles.groupTitle}>生成参数</span>
              <div className={styles.row}>
                <label className={shared.fld}>
                  <span className={shared.fldK}>温度 · {ai.temperature.toFixed(2)}</span>
                  <input
                    className={styles.range}
                    type="range"
                    min={0}
                    max={1}
                    step={0.05}
                    value={ai.temperature}
                    onChange={(event) => write({ temperature: Number(event.target.value) })}
                  />
                </label>
                <label className={shared.fld}>
                  <span className={shared.fldK}>上下文窗口</span>
                  <input
                    value={`${(spec?.contextWindow ?? 0).toLocaleString("zh-CN")} tokens`}
                    readOnly
                    className={styles.readonly}
                  />
                </label>
              </div>

              {spec && spec.reasoning.length > 0 && (
                <div className={styles.group}>
                  <span className={shared.fldK}>思维强度</span>
                  <div className={styles.seg}>
                    {spec.reasoning.map((level) => (
                      <button
                        key={level}
                        type="button"
                        aria-pressed={(ai.reasoning ?? spec.reasoning[0]) === level}
                        onClick={() => write({ reasoning: level })}
                      >
                        {level}
                      </button>
                    ))}
                  </div>
                </div>
              )}

              <span className={styles.hint}>
                上下文窗口与思维强度档位来自 <code>config/models.json</code>，
                改完刷新即生效；打包时会硬编码进程序。
              </span>
            </div>
          </>
        ) : (
          <>
            <div className={styles.group}>
              <span className={styles.groupTitle}>明暗</span>
              <div className={styles.seg}>
                <button
                  type="button"
                  aria-pressed={state.ui.mode === "light"}
                  onClick={() => dispatch({ type: "setMode", mode: "light" })}
                >
                  浅色
                </button>
                <button
                  type="button"
                  aria-pressed={state.ui.mode === "dark"}
                  onClick={() => dispatch({ type: "setMode", mode: "dark" })}
                >
                  深色
                </button>
              </div>
            </div>

            <div className={styles.group}>
              <span className={styles.groupTitle}>强调色</span>
              <div className={styles.swatches}>
                {ACCENTS.map((item) => (
                  <button
                    key={item.key}
                    type="button"
                    className={styles.sw}
                    aria-pressed={!state.ui.accentHex && state.ui.accent === item.key}
                    title={item.label}
                    onClick={() => {
                      setHex("");
                      dispatch({ type: "setAccent", accent: item.key });
                    }}
                  >
                    <i style={{ background: item.color }} />
                  </button>
                ))}
                <span className={styles.custom}>
                  <input
                    type="color"
                    aria-label="自定义强调色"
                    value={isHexColor(hex) ? hex : state.ui.accentHex ?? "#3b50b4"}
                    onChange={(event) => applyHex(event.target.value)}
                  />
                  <input
                    type="text"
                    aria-label="强调色 HEX"
                    placeholder="#3b50b4"
                    value={hex}
                    spellCheck={false}
                    onChange={(event) => applyHex(event.target.value)}
                  />
                </span>
              </div>
              <span className={styles.hint}>
                自定义色会覆盖预设；强调色上的文字会按亮度自动取黑或白。
              </span>
            </div>

            <div className={styles.group}>
              <span className={styles.groupTitle}>
                界面字号 · {Math.round(textScale * 100)}%
              </span>
              <div className={styles.seg}>
                <button
                  type="button"
                  aria-pressed={state.ui.textScale == null}
                  title={`跟随系统（${Math.round(systemTextScale * 100)}%）`}
                  onClick={() => setTextScale(null)}
                >
                  自动
                </button>
                {TEXT_SCALES.map((item) => (
                  <button
                    key={item.key}
                    type="button"
                    aria-pressed={state.ui.textScale === item.value}
                    onClick={() => setTextScale(item.value)}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
              <span className={styles.hint}>只影响界面，不影响纸面与导出。</span>
            </div>
          </>
        )}
      </div>

      <footer className={styles.foot}>
        <span
          className={cx(
            styles.probe,
            probe.tone === "ok" && styles.probeOk,
            probe.tone === "fail" && styles.probeFail,
          )}
          title={probe.text}
        >
          {probe.text || "配置即时保存"}
        </span>
        <button
          type="button"
          className={shared.btnGhost}
          onClick={() => {
            void runProbe();
          }}
        >
          测试连接
        </button>
        <button type="button" className={shared.btnPrimary} onClick={onClose}>
          完成
        </button>
      </footer>
    </div>
  );
}
