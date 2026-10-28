import { useEffect, useRef, useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { Close, Plus, Trash } from "@/components/ui/icons";
import { useVeilWheelLock } from "@/components/ui/veil";
import { Modal } from "@/components/ui/modal";
import { Confirm } from "@/components/ui/confirm";
import { useToast } from "@/components/ui/toast-context";
import { ACCENTS, TEXT_SCALES } from "@/types";
import { isHexColor } from "@/lib/color";
import {
  aiFromModel,
  loadAi,
  loadAux,
  saveAi,
  saveAux,
  testAi,
  type AiSettings,
  type AuxModelChoice,
  type AuxSettings,
} from "@/lib/ai";
import {
  findModel,
  isCustomModel,
  listModels,
  removeCustomModel,
  saveCustomModel,
  type ModelConfig,
  type ModelKind,
} from "@/lib/model-config";
import { isTauri } from "@/lib/storage";
import { configureKnowledge } from "@/lib/knowledge";
import { ColorPicker } from "@/components/ui/color-picker";
import { cx } from "@/lib/cx";
import shared from "@/components/ui/shared.module.css";
import styles from "./sheet.module.css";

type Tab = "model" | "look";

/**
 * 统一设置：模型与外观收在一个地方。
 *
 * 模型配置写在 localStorage（`sharp-resume:ai:v1`），不进工作区、不随简历同步；
 * 模型清单 = `config/models.json` 里的内置项（打包时已硬编码）+ 用户自建的
 * 自定义项（`sharp-resume:models:v1`，同样只在本机）。
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
  const veilRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // 大面板不走 Modal（宽度由内容决定），但「滚轮不许带动底下的纸面」这条要一致
  useVeilWheelLock(veilRef, open);

  if (!open) return null;

  return (
    // 遮罩只挡点击，不关窗 —— 关闭只有两条路：右上角 X 与 ESC
    <div ref={veilRef} className={shared.veil} data-print="hide">
      <SheetBody onClose={onClose} initialTab={initialTab} />
    </div>
  );
}

function SheetBody({ onClose, initialTab }: { onClose: () => void; initialTab: Tab }) {
  const { state, dispatch, textScale, systemTextScale, setTextScale } = useWorkspace();
  const toast = useToast();
  const [tab, setTab] = useState<Tab>(initialTab);
  const [ai, setAi] = useState<AiSettings>(loadAi);
  const [hex, setHex] = useState(state.ui.accentHex ?? "");
  const [probe, setProbe] = useState<{ tone: "idle" | "ok" | "fail"; text: string }>({
    tone: "idle",
    text: "",
  });
  /**
   * 清单存一份在 state 里：`listModels()` 是普通函数，增删之后 React 不会自己重渲染，
   * 下拉就会停在旧选项上。
   */
  const [models, setModels] = useState<ModelConfig[]>(() => listModels("chat"));
  /**
   * 嵌入 / OCR 是「功能用哪个模型」，不是「当前对话用哪个」——
   * 所以单独一份设置，换对话模型不会顺手把它们换掉。
   */
  const [aux, setAux] = useState<AuxSettings>(loadAux);
  /** 正在新建 / 删除的类别；null = 没有弹层 */
  const [creating, setCreating] = useState<ModelKind | null>(null);
  const [removing, setRemoving] = useState<ModelKind | null>(null);

  const spec = findModel(ai.model);

  const write = (patch: Partial<AiSettings>) => {
    const next = { ...ai, ...patch };
    setAi(next);
    saveAi(next);
  };

  /** 换模型时把网关/上下文/温度一起换成该模型的默认值，否则会拿旧网关配新模型 */
  const pickModel = (id: string) => {
    const model = findModel(id);
    if (!model) return;
    write(aiFromModel(model, ai));
  };

  /** 改嵌入/OCR 的选择：只动 aux，不碰当前对话模型 */
  const pickAux = (kind: "embedding" | "ocr", id: string) => {
    const model = findModel(id);
    if (!model) return;
    const choice: AuxModelChoice = { model: id, baseUrl: model.baseUrl, apiKey: "" };
    const next = { ...aux, [kind]: choice };
    setAux(next);
    saveAux({ [kind]: choice });
    configureKnowledge(next.embedding);
  };

  const patchAux = (kind: "embedding" | "ocr", patch: Partial<AuxModelChoice>) => {
    const current = aux[kind];
    if (!current) return;
    const next = { ...current, ...patch };
    setAux({ ...aux, [kind]: next });
    saveAux({ [kind]: next });
    configureKnowledge(kind === "embedding" ? next : aux.embedding);
  };

  const createModel = (kind: ModelKind, model: ModelConfig, apiKey: string) => {
    saveCustomModel(model);
    setModels(listModels("chat"));
    if (kind === "chat") {
      const next = aiFromModel(model, ai);
      // 表单里没填 key 就沿用当前那把 —— 同一个网关换型号通常不需要重输
      if (apiKey.trim()) next.apiKey = apiKey.trim();
      setAi(next);
      saveAi(next);
    } else {
      const choice: AuxModelChoice = { model: model.id, baseUrl: model.baseUrl, apiKey: apiKey.trim() };
      setAux({ ...aux, [kind]: choice });
      saveAux({ [kind]: choice });
    }
    setCreating(null);
    toast(`已新建模型 ${model.label}`);
  };

  const dropModel = (kind: ModelKind) => {
    const id = kind === "chat" ? ai.model : aux[kind]?.model ?? "";
    const label = findModel(id)?.label ?? id;
    setRemoving(null);
    removeCustomModel(id);
    const chatModels = listModels("chat");
    setModels(chatModels);
    if (kind === "chat") {
      // 删掉的正是当前在用的那条：回退到第一条内置，否则设置里会留一个查不到的 id
      const fallback = chatModels.find((item) => !isCustomModel(item.id));
      if (fallback) write(aiFromModel(fallback, ai));
    } else {
      setAux({ ...aux, [kind]: null });
      saveAux({ [kind]: null });
    }
    toast(`已删除自定义模型 ${label}`);
  };

  const runProbe = async () => {
    setProbe({ tone: "idle", text: "正在连接…" });
    try {
      setProbe({ tone: "ok", text: `连上了：${await testAi(ai)}` });
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      setProbe({
        tone: "fail",
        text: reason.includes("跨域") ? `${reason}（这是浏览器调试模式，桌面窗口里点同一个按钮才准）` : reason,
      });
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
              <span className={styles.groupTitle}>对话模型</span>
              <div className={styles.modelRow}>
                <select
                  className={styles.select}
                  value={ai.model}
                  onChange={(event) => pickModel(event.target.value)}
                >
                  {models.map((item) => (
                    <option value={item.id} key={item.id}>
                      {item.label} · {item.provider}
                      {isCustomModel(item.id) ? "（自定义）" : ""}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className={cx(shared.btnGhost, styles.modelBtn)}
                  title="新建自定义模型"
                  aria-label="新建自定义模型"
                  onClick={() => setCreating("chat")}
                >
                  <Plus size={11} />
                </button>
                {isCustomModel(ai.model) && (
                  <button
                    type="button"
                    className={cx(shared.btnGhost, styles.modelBtn)}
                    title={`删除自定义模型 ${ai.model}`}
                    aria-label="删除自定义模型"
                    onClick={() => setRemoving("chat")}
                  >
                    <Trash size={11} />
                  </button>
                )}
              </div>
              {spec?.note && <span className={styles.hint}>{spec.note}</span>}

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
                上下文窗口与思维强度档位来自模型清单：内置项写在{" "}
                <code>config/models.json</code>，自定义项只存在本机（换机器不同步）。
              </span>
            </div>

            <div className={styles.group}>
              <span className={styles.groupTitle}>嵌入模型 · 知识库</span>
              <div className={styles.modelRow}>
                <select
                  className={styles.select}
                  value={aux.embedding?.model ?? ""}
                  onChange={(event) => {
                    const id = event.target.value;
                    if (id) pickAux("embedding", id);
                    else {
                      setAux({ ...aux, embedding: null });
                      saveAux({ embedding: null });
                      configureKnowledge(null);
                    }
                  }}
                >
                  <option value="">未启用 · 只用关键词检索</option>
                  {listModels("embedding").map((item) => (
                    <option value={item.id} key={item.id}>
                      {item.label} · {item.provider}
                      {isCustomModel(item.id) ? "（自定义）" : ""}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className={cx(shared.btnGhost, styles.modelBtn)}
                  title="新建嵌入模型"
                  aria-label="新建嵌入模型"
                  onClick={() => setCreating("embedding")}
                >
                  <Plus size={11} />
                </button>
                {aux.embedding && isCustomModel(aux.embedding.model) && (
                  <button
                    type="button"
                    className={cx(shared.btnGhost, styles.modelBtn)}
                    title="删除这个自定义嵌入模型"
                    aria-label="删除自定义嵌入模型"
                    onClick={() => setRemoving("embedding")}
                  >
                    <Trash size={11} />
                  </button>
                )}
              </div>
              <div className={styles.formRow}>
                <label className={shared.fld}>
                  <span className={shared.fldK}>Base URL · 留空沿用对话模型</span>
                  <input
                    value={aux.embedding?.baseUrl ?? ""}
                    disabled={!aux.embedding}
                    spellCheck={false}
                    onChange={(event) => patchAux("embedding", { baseUrl: event.target.value })}
                  />
                </label>
                <label className={shared.fld}>
                  <span className={shared.fldK}>API Key · 留空沿用对话模型</span>
                  <input
                    type="password"
                    value={aux.embedding?.apiKey ?? ""}
                    disabled={!aux.embedding}
                    spellCheck={false}
                    onChange={(event) => patchAux("embedding", { apiKey: event.target.value })}
                  />
                </label>
              </div>
              <span className={styles.hint}>
                只用于知识库的语义召回（查询时算一次向量）
                {aux.embedding && findModel(aux.embedding.model)?.dim
                  ? ` · ${findModel(aux.embedding.model)?.dim} 维`
                  : ""}
                ；不配也不影响使用，检索会退化成关键词匹配。
              </span>
            </div>

            <div className={styles.group}>
              <span className={styles.groupTitle}>OCR 模型 · 图片转文字</span>
              <div className={styles.modelRow}>
                <select
                  className={styles.select}
                  value={aux.ocr?.model ?? ""}
                  onChange={(event) => {
                    const id = event.target.value;
                    if (id) pickAux("ocr", id);
                    else {
                      setAux({ ...aux, ocr: null });
                      saveAux({ ocr: null });
                    }
                  }}
                >
                  <option value="">未启用</option>
                  {listModels("ocr").map((item) => (
                    <option value={item.id} key={item.id}>
                      {item.label} · {item.provider}
                      {isCustomModel(item.id) ? "（自定义）" : ""}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className={cx(shared.btnGhost, styles.modelBtn)}
                  title="新建 OCR 模型"
                  aria-label="新建 OCR 模型"
                  onClick={() => setCreating("ocr")}
                >
                  <Plus size={11} />
                </button>
                {aux.ocr && isCustomModel(aux.ocr.model) && (
                  <button
                    type="button"
                    className={cx(shared.btnGhost, styles.modelBtn)}
                    title="删除这个自定义 OCR 模型"
                    aria-label="删除自定义 OCR 模型"
                    onClick={() => setRemoving("ocr")}
                  >
                    <Trash size={11} />
                  </button>
                )}
              </div>
              <div className={styles.formRow}>
                <label className={shared.fld}>
                  <span className={shared.fldK}>Base URL · 留空沿用对话模型</span>
                  <input
                    value={aux.ocr?.baseUrl ?? ""}
                    disabled={!aux.ocr}
                    spellCheck={false}
                    onChange={(event) => patchAux("ocr", { baseUrl: event.target.value })}
                  />
                </label>
                <label className={shared.fld}>
                  <span className={shared.fldK}>API Key · 留空沿用对话模型</span>
                  <input
                    type="password"
                    value={aux.ocr?.apiKey ?? ""}
                    disabled={!aux.ocr}
                    spellCheck={false}
                    onChange={(event) => patchAux("ocr", { apiKey: event.target.value })}
                  />
                </label>
              </div>
              <span className={styles.hint}>
                默认构建没有内置 OCR：模型看不了图时，图片会先交给这个接口转成文字。
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
                {/* 自定义色：点色块弹出取色器（色相/明度/饱和 + HEX）。
                    HEX 输入在取色器里，这里不再放第二个 —— 两个框显示同一个值
                    看起来像出了 bug。 */}
                <ColorPicker
                  value={isHexColor(hex) ? hex : state.ui.accentHex ?? "#3b50b4"}
                  onChange={applyHex}
                  label="自定义强调色"
                />
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

      {/* 底栏按 tab 分开：外观页没有「通讯层」「测试连接」这些东西 ——
          那是模型的诊断手段，出现在配色页上只会让人以为配色也要联网 */}
      <footer className={styles.foot}>
        {tab === "model" ? (
          <>
            <span
              className={styles.route}
              title="桌面端请求走 Rust，浏览器调试走 fetch（会被跨域拦）"
            >
              通讯层：{isTauri() ? "Rust · 不经跨域" : "浏览器 fetch · 会被跨域拦"}
            </span>
            <span
              className={cx(
                styles.probe,
                probe.tone === "ok" && styles.probeOk,
                probe.tone === "fail" && styles.probeFail,
              )}
              title={probe.text}
              data-selectable
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
          </>
        ) : (
          // 外观页只有一句说明，用 flex:1 把「完成」顶到右端
          <span className={styles.probe}>主题与字号只影响界面，不改纸面与导出</span>
        )}
        <button type="button" className={shared.btnPrimary} onClick={onClose}>
          完成
        </button>
      </footer>

      {/* 两个弹层挂在面板根节点里：设置面板本身就是个浮层，
          再 portal 出去反而会丢掉「哪一层在上面」的顺序 */}
      {creating && (
        <CreateModel
          kind={creating}
          baseUrl={creating === "chat" ? ai.baseUrl : aux[creating]?.baseUrl || ai.baseUrl}
          taken={listModels(creating).map((item) => item.id)}
          onCancel={() => setCreating(null)}
          onSubmit={(model, apiKey) => createModel(creating, model, apiKey)}
        />
      )}

      {removing && (
        <Confirm
          open
          title={`删除自定义模型「${
            removing === "chat"
              ? spec?.label ?? ai.model
              : findModel(aux[removing]?.model ?? "")?.label ?? "这条"
          }」？`}
          description={
            removing === "chat"
              ? "只从这台机器的模型清单里移除，服务端配置和密钥都不受影响。删完会退到第一条内置模型。"
              : "只从这台机器的模型清单里移除。这个位置会变成「未启用」，该功能回落到关键词检索/纯文字。"
          }
          onCancel={() => setRemoving(null)}
          onConfirm={() => dropModel(removing)}
        />
      )}
    </div>
  );
}

/* ==========================================================================
   新建自定义模型
   ========================================================================== */

/**
 * 能填的字段刻意只留「配置网关需要的」，其余走缺省：
 * 用户要的是「把公司内网的 qwen 接进来」，不是填一份模型能力调研表。
 * 视觉能力单独给一个勾选 —— 它决定图片走视觉直传还是 OCR（见 lib/attach）。
 */
function CreateModel({
  kind,
  baseUrl,
  taken,
  onCancel,
  onSubmit,
}: {
  kind: ModelKind;
  baseUrl: string;
  taken: string[];
  onCancel: () => void;
  onSubmit: (model: ModelConfig, apiKey: string) => void;
}) {
  const [label, setLabel] = useState("");
  const [id, setId] = useState("");
  const [url, setUrl] = useState(baseUrl);
  const [key, setKey] = useState("");
  const [window_, setWindow] = useState(kind === "embedding" ? "8192" : "64000");
  const [temp, setTemp] = useState(kind === "chat" ? "0.4" : "0");
  const [levels, setLevels] = useState("");
  const [vision, setVision] = useState(kind === "ocr");
  const [dim, setDim] = useState("");

  const modelId = id.trim() || label.trim();
  /** 同名会覆盖：说清楚而不是拦下来 —— 改错重填一次比「先删再建」顺手 */
  const override = modelId.length > 0 && taken.includes(modelId);

  const submit = () => {
    if (!modelId) return;
    onSubmit(
      {
        id: modelId,
        label: label.trim() || modelId,
        kind,
        provider: "自定义",
        baseUrl: url.trim(),
        contextWindow: Number(window_) > 0 ? Math.round(Number(window_)) : 64_000,
        maxOutput: 8_192,
        reasoning: levels
          .split(/[,，\s]+/)
          .map((item) => item.trim())
          .filter(Boolean),
        vision,
        defaultTemperature: Math.min(2, Math.max(0, Number(temp) || 0)),
        ...(kind === "embedding" && Number(dim) > 0 ? { dim: Math.round(Number(dim)) } : {}),
      },
      key,
    );
  };

  return (
    <Modal
      title={
        kind === "chat" ? "新建对话模型" : kind === "embedding" ? "新建嵌入模型" : "新建 OCR 模型"
      }
      subtitle="任何 OpenAI 兼容接口都可以"
      onCancel={onCancel}
      bodyClassName={styles.form}
      footer={
        <>
          <button type="button" className={shared.btnGhost} onClick={onCancel}>
            取消
          </button>
          <button type="button" className={shared.btnPrimary} disabled={!modelId} onClick={submit}>
            保存并使用
          </button>
        </>
      }
    >
      <label className={shared.fld}>
        <span className={shared.fldK}>显示名称</span>
        <input
          data-autofocus
          value={label}
          placeholder="例如：内网 Qwen3-32B"
          spellCheck={false}
          onChange={(event) => setLabel(event.target.value)}
        />
      </label>
      <label className={shared.fld}>
        <span className={shared.fldK}>模型名 · 接口里的 model 字段</span>
        <input
          value={id}
          placeholder="qwen3-32b"
          spellCheck={false}
          onChange={(event) => setId(event.target.value)}
        />
      </label>
      <label className={shared.fld}>
        <span className={shared.fldK}>Base URL</span>
        <input
          value={url}
          placeholder="https://your-gateway/v1"
          spellCheck={false}
          onChange={(event) => setUrl(event.target.value)}
        />
      </label>
      <label className={shared.fld}>
        <span className={shared.fldK}>API Key · 留空沿用当前密钥</span>
        <input
          type="password"
          value={key}
          placeholder="sk-…"
          spellCheck={false}
          onChange={(event) => setKey(event.target.value)}
        />
      </label>

      <div className={styles.formRow}>
        <label className={shared.fld}>
          <span className={shared.fldK}>上下文窗口</span>
          <input
            value={window_}
            inputMode="numeric"
            spellCheck={false}
            onChange={(event) => setWindow(event.target.value.replace(/[^\d]/g, ""))}
          />
        </label>
        {kind !== "ocr" && (
          <label className={shared.fld}>
            <span className={shared.fldK}>温度</span>
            <input
              value={temp}
              inputMode="decimal"
              spellCheck={false}
              onChange={(event) => setTemp(event.target.value.replace(/[^\d.]/g, ""))}
            />
          </label>
        )}
        {kind === "embedding" && (
          <label className={shared.fld}>
            <span className={shared.fldK}>向量维度 · 选填</span>
            <input
              value={dim}
              inputMode="numeric"
              placeholder="1024"
              spellCheck={false}
              onChange={(event) => setDim(event.target.value.replace(/[^\d]/g, ""))}
            />
          </label>
        )}
      </div>

      {kind === "chat" && (
        <label className={shared.fld}>
          <span className={shared.fldK}>思维强度档位 · 逗号分隔，留空表示没有</span>
          <input
            value={levels}
            placeholder="low, medium, high"
            spellCheck={false}
            onChange={(event) => setLevels(event.target.value)}
          />
        </label>
      )}

      {kind === "chat" && (
        <label className={styles.formCheck}>
          <input
            type="checkbox"
            checked={vision}
            onChange={(event) => setVision(event.target.checked)}
          />
          这个模型能看图（图片直接给模型，不走 OCR）
        </label>
      )}

      <p className={styles.formHint}>
        {override
          ? "这个模型名已经存在：保存会覆盖你自己建的那一条。"
          : "自定义模型只存在这台机器上，不随工作区同步；模型名与内置项重名时永远选不到内置的那条，所以会拦住。"}
      </p>
    </Modal>
  );
}
