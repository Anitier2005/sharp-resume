import { useMemo, useState, type ChangeEvent } from "react";
import {
  aiFromModel,
  choiceOf,
  loadAi,
  loadAux,
  loadSlots,
  saveAi,
  saveAux,
  saveSlot,
  type AiSettings,
  type AuxModelChoice,
  type SlotSettings,
} from "@/lib/ai";
import {
  CONTEXT_PRESETS,
  MODEL_KINDS,
  defaultProfile,
  findModel,
  isCustomModel,
  kindLabel,
  kindOf,
  listAllModels,
  listProfiles,
  removeCustomModel,
  removeProfile,
  saveCustomModel,
  saveProfile,
  type ModelConfig,
  type ModelKind,
  type ModelProfile,
} from "@/lib/model-config";
import { configureKnowledge } from "@/lib/knowledge";
import { notifySettingsChanged } from "@/lib/settings-window";
import { Button, Field, Input, ListEmpty, PasswordInput, Select, Tooltip } from "@/components/ui";
import { Confirm } from "@/components/ui/confirm";
import { Check, ChevronLeft, Gear, Plus, Trash } from "@/components/ui/icons";
import { cx } from "@/lib/cx";
import styles from "./settings.module.css";

/**
 * 模型设置（独立窗口）。
 *
 * 两个 tab，对应两件不同的事：
 *   1. **模型选择**：六类用途（会话 / 压缩 / 排版 / 知识库分片 / 嵌入 / OCR）
 *      各自「用哪个模型的哪套配置」。这是「指针」，改一次就能换掉。
 *   2. **模型配置**：模型清单本身 —— 模型 item → 配置 item（网关 + 密钥 + 参数）。
 *
 * 为什么拆开：选型是高频、轻量（换一家试试）；配置是低频、重（填网关和 key）。
 * 混在一页时，想「换个模型试试」会被一堆字段挡住。
 *
 * 勾永远在最左边：它表示「当前生效」，是行状态而不是行内容。
 */

type Tab = "pick" | "manage";

function blank(kind: ModelKind, baseUrl: string): ModelConfig {
  return {
    kind,
    id: "",
    label: "",
    provider: "自定义",
    baseUrl,
    contextWindow: kind === "embedding" ? 8_192 : 64_000,
    maxOutput: 8_192,
    reasoning: [],
    vision: kind === "ocr",
    defaultTemperature: kind === "chat" ? 0.4 : 0,
  };
}

export function ModelSettings() {
  const [tab, setTab] = useState<Tab>("pick");
  const [ai, setAi] = useState(loadAi);
  const [aux, setAux] = useState(loadAux);
  const [slots, setSlots] = useState<SlotSettings>(loadSlots);
  /** 详情页显示哪个模型；null = 停在列表。支持 `?open=<id>` 直接落进去 */
  const [openId, setOpenId] = useState<string | null>(() =>
    new URLSearchParams(window.location.search).get("open"),
  );
  const [draft, setDraft] = useState<ModelConfig | null>(null);
  const [dropModel, setDropModel] = useState<ModelConfig | null>(null);
  const [dropProfile, setDropProfile] = useState<ModelProfile | null>(null);
  /** localStorage 写完之后用它触发重读 */
  const [tick, setTick] = useState(0);

  const models = useMemo(() => listAllModels(), [tick]);
  const editing = draft ?? (openId ? (findModel(openId) ?? null) : null);
  const isDraft = draft !== null;
  const custom = editing !== null && (isDraft || isCustomModel(editing.id));

  const profiles = useMemo(
    () => (editing && !isDraft ? listProfiles(editing) : []),
    // tick 变了说明磁盘上的配置变了，必须重算
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [editing, tick, isDraft],
  );

  /* ---------------- 读取当前指针 ---------------- */

  const choiceFor = (kind: ModelKind): AuxModelChoice | null => {
    if (kind === "chat") {
      if (!ai.model) return null;
      return {
        model: ai.model,
        profileId: ai.profileId,
        baseUrl: ai.baseUrl,
        apiKey: ai.apiKey,
      };
    }
    if (kind === "embedding" || kind === "ocr") return aux[kind];
    return slots[kind as keyof SlotSettings] ?? null;
  };

  /** 某个模型被哪几类用途在用：勾与「会话 · 压缩」小字都靠它 */
  const usedBy = (modelId: string): ModelKind[] =>
    MODEL_KINDS.map((item) => item.kind).filter((kind) => choiceFor(kind)?.model === modelId);

  const profileUsedBy = (modelId: string, profileId: string): ModelKind[] =>
    MODEL_KINDS.map((item) => item.kind).filter((kind) => {
      const choice = choiceFor(kind);
      return choice?.model === modelId && checkProfile(choice, modelId, profileId);
    });

  /* ---------------- 写入 ---------------- */

  const writeAiSettings = (next: AiSettings) => {
    setAi(next);
    saveAi(next);
    void notifySettingsChanged("ai");
  };

  const writeChoice = (kind: ModelKind, choice: AuxModelChoice | null) => {
    if (kind === "chat") {
      if (!choice) return;
      const spec = findModel(choice.model);
      writeAiSettings({
        ...ai,
        model: choice.model,
        profileId: choice.profileId,
        baseUrl: choice.baseUrl.trim() || spec?.baseUrl || ai.baseUrl,
        apiKey: choice.apiKey,
        temperature: spec?.defaultTemperature ?? ai.temperature,
        reasoning: spec?.reasoning[0],
        contextWindow: spec?.contextWindow ?? ai.contextWindow,
      });
      return;
    }
    if (kind === "embedding" || kind === "ocr") {
      const next = { ...aux, [kind]: choice };
      setAux(next);
      saveAux({ [kind]: choice });
      if (kind === "embedding") configureKnowledge(choice);
      void notifySettingsChanged("aux");
      return;
    }
    saveSlot(kind, choice);
    setSlots(loadSlots());
    void notifySettingsChanged("slots");
  };

  const pickModel = (kind: ModelKind, modelId: string) => {
    if (!modelId) {
      writeChoice(kind, null);
      return;
    }
    const profile = listProfiles(findModel(modelId) ?? blank(kind, ""))[0];
    if (!profile) {
      // 配置被删光了的模型：选中时补一套默认的，否则选中了也没有可用网关
      const spec = findModel(modelId);
      if (!spec) return;
      const fresh = defaultProfile(spec);
      saveProfile(spec.id, fresh);
      setTick((value) => value + 1);
      writeChoice(kind, choiceOf(modelId, fresh));
      return;
    }
    writeChoice(kind, choiceOf(modelId, profile));
  };

  const pickProfile = (kind: ModelKind, modelId: string, profileId: string) => {
    const spec = findModel(modelId);
    if (!spec) return;
    const profile = listProfiles(spec).find((item) => item.id === profileId);
    if (!profile) return;
    writeChoice(kind, choiceOf(modelId, profile));
  };

  /* ---------------- 模型本身 ---------------- */

  const patchModel = (patch: Partial<ModelConfig>) => {
    if (!editing) return;
    const next = { ...editing, ...patch };
    if (isDraft) {
      setDraft(next);
      return;
    }
    saveCustomModel(next);
    setTick((value) => value + 1);
  };

  const commitDraft = () => {
    if (!draft) return;
    const id = draft.id.trim();
    if (!id) return;
    const model = { ...draft, label: draft.label.trim() || id };
    saveCustomModel(model);
    const profile = defaultProfile(model);
    saveProfile(model.id, profile);
    setDraft(null);
    setTick((value) => value + 1);
    setOpenId(model.id);
  };

  const drop = (model: ModelConfig) => {
    setDropModel(null);
    removeCustomModel(model.id);
    setOpenId(null);
    setTick((value) => value + 1);
    // 被它占着的用途全部回退：会话回到第一条内置，其余回到「不使用」
    MODEL_KINDS.forEach((item) => {
      if (choiceFor(item.kind)?.model !== model.id) return;
      if (item.kind === "chat") {
        const fallback = listAllModels().find((entry) => !isCustomModel(entry.id));
        if (fallback) writeAiSettings(aiFromModel(fallback, ai));
      } else {
        writeChoice(item.kind, null);
      }
    });
  };

  const addProfile = () => {
    if (!editing) return;
    const profile: ModelProfile = {
      id: `p-${Date.now().toString(36)}`,
      label: `配置 ${profiles.length + 1}`,
      baseUrl: editing.baseUrl,
      apiKey: "",
      temperature: editing.defaultTemperature,
      reasoning: editing.reasoning[0],
      contextWindow: editing.contextWindow,
    };
    saveProfile(editing.id, profile);
    setTick((value) => value + 1);
  };

  const patchProfile = (profile: ModelProfile, patch: Partial<ModelProfile>) => {
    if (!editing) return;
    const next = { ...profile, ...patch };
    saveProfile(editing.id, next);
    setTick((value) => value + 1);
    // 它正被某几类用途引用：同步把新参数写进去，否则改了不生效
    const used = profileUsedBy(editing.id, profile.id);
    if (used.length > 0) {
      used.forEach((kind) => {
        const current = choiceFor(kind);
        if (!current) return;
        writeChoice(kind, { ...current, baseUrl: next.baseUrl, apiKey: next.apiKey });
      });
    }
  };

  const back = () => {
    setDraft(null);
    setOpenId(null);
  };

  /* ---------------- 详情 ---------------- */

  if (editing) {
    return (
      <div className={styles.page}>
        <header className={styles.head}>
          <Button variant="quiet" size="sm" icon aria-label="返回列表" onClick={back}>
            <ChevronLeft size={12} />
          </Button>
          <b>{isDraft ? "添加模型" : editing.label}</b>
          {!isDraft && custom && (
            <Button variant="quiet" size="sm" onClick={() => setDropModel(editing)}>
              删除模型
            </Button>
          )}
        </header>

        <section className={styles.group}>
          <span className={styles.groupTitle}>模型</span>
          <div className={styles.fields}>
            <Field label="显示名称">
              <Input
                data-autofocus
                value={editing.label}
                placeholder={isDraft ? "例如：内网 Qwen3" : undefined}
                disabled={!custom}
                onChange={(event) => patchModel({ label: event.target.value })}
              />
            </Field>
            <Field label="模型名 · 接口里的 model 字段">
              <Input
                value={editing.id}
                placeholder="qwen3-32b"
                spellCheck={false}
                // 保存后不允许改 id：改 id 等于另建一条，会留下孤儿配置
                disabled={!isDraft}
                onChange={(event) => patchModel({ id: event.target.value })}
              />
            </Field>
            <Field label="用途 · 只影响它出现在哪些清单里">
              <Select
                ariaLabel="用途"
                value={kindOf(editing)}
                options={MODEL_KINDS.map((item) => ({ value: item.kind, label: item.label }))}
                onChange={(value) => patchModel({ kind: value as ModelKind })}
              />
            </Field>
            {kindOf(editing) !== "ocr" && custom && (
              <Field label="思维强度档位 · 逗号分隔">
                <Input
                  value={editing.reasoning.join(", ")}
                  placeholder="low, medium, high"
                  spellCheck={false}
                  onChange={(event) =>
                    patchModel({
                      reasoning: event.target.value
                        .split(/[,，\s]+/)
                        .map((item) => item.trim())
                        .filter(Boolean),
                    })
                  }
                />
              </Field>
            )}
          </div>
        </section>

        {isDraft ? (
          <DraftProfile draft={draft!} onCommit={commitDraft} />
        ) : (
          <>
            <section className={styles.group}>
              <div className={styles.groupHead}>
                <span className={styles.groupTitle}>配置</span>
                <Button size="sm" leading={<Plus size={10} />} onClick={addProfile}>
                  添加配置
                </Button>
              </div>
              <div className={styles.list}>
                {profiles.map((profile) => {
                  const used = profileUsedBy(editing.id, profile.id);
                  return (
                    <div key={profile.id} className={cx(styles.row, styles.rowStatic)}>
                      {/* 勾在最左：它是「这一行正在被用」的状态，不是内容 */}
                      <span className={styles.rowCheck}>
                        {used.length > 0 && <Check size={10} />}
                      </span>
                      <button
                        type="button"
                        className={styles.rowMain}
                        onClick={() => {
                          const first = used[0] ?? "chat";
                          pickProfile(first, editing.id, profile.id);
                        }}
                      >
                        <span className={styles.rowName}>{profile.label}</span>
                        <span className={styles.rowMeta}>{profile.baseUrl || "沿用网关"}</span>
                        {used.length > 0 && (
                          <span className={styles.rowMeta}>
                            {used.map(kindLabel).join(" · ")}
                          </span>
                        )}
                      </button>
                      <Tooltip label="删除这组配置">
                        <Button
                          variant="quiet"
                          size="xs"
                          icon
                          aria-label={`删除配置 ${profile.label}`}
                          onClick={() => setDropProfile(profile)}
                        >
                          <Trash size={10} />
                        </Button>
                      </Tooltip>
                    </div>
                  );
                })}
                {profiles.length === 0 && <ListEmpty>还没有配置，点右上「添加配置」</ListEmpty>}
              </div>
            </section>

            {profiles.map((profile) => (
              <ProfileFields
                key={profile.id}
                model={editing}
                profile={profile}
                live={profileUsedBy(editing.id, profile.id).length > 0}
                onPatch={(patch) => patchProfile(profile, patch)}
              />
            ))}
          </>
        )}

        {dropModel && (
          <Confirm
            open
            title={`删除模型「${dropModel.label}」？`}
            description="模型和它下面的所有配置都会从这台机器上移除；用到它的用途会回退。"
            onCancel={() => setDropModel(null)}
            onConfirm={() => drop(dropModel)}
          />
        )}

        {dropProfile && editing && (
          <Confirm
            open
            title={`删除配置「${dropProfile.label}」？`}
            description="只删这一组网关与密钥，模型本身保留。"
            onCancel={() => setDropProfile(null)}
            onConfirm={() => {
              removeProfile(editing.id, dropProfile.id);
              setDropProfile(null);
              setTick((value) => value + 1);
            }}
          />
        )}
      </div>
    );
  }

  /* ---------------- 模型选择 ---------------- */

  if (tab === "pick") {
    return (
      <div className={styles.page}>
        <nav className={styles.tabs} role="tablist">
          <button type="button" role="tab" aria-selected onClick={() => setTab("pick")}>
            模型选择
          </button>
          <button type="button" role="tab" aria-selected={false} onClick={() => setTab("manage")}>
            模型配置
          </button>
        </nav>

        <div className={styles.pickList}>
          {MODEL_KINDS.map((item) => {
            const choice = choiceFor(item.kind);
            const spec = choice ? findModel(choice.model) : null;
            const list = spec ? listProfiles(spec) : [];
            const profile = choice
              ? list.find((entry) => entry.id === choice.profileId) ?? list[0]
              : undefined;
            return (
              <section key={item.kind} className={styles.group}>
                <div className={styles.groupHead}>
                  <span className={styles.groupTitle}>{item.label}</span>
                  <span className={styles.groupNote}>{item.note}</span>
                </div>
                <div className={styles.fields}>
                  <Field label="模型">
                    <Select
                      ariaLabel={`${item.label}模型`}
                      value={choice?.model ?? ""}
                      options={[
                        ...(item.kind === "chat"
                          ? []
                          : [{ value: "", label: "不使用" }]),
                        ...models.map((model) => ({
                          value: model.id,
                          label: `${model.label} · ${model.provider}`,
                        })),
                      ]}
                      onChange={(value) => pickModel(item.kind, value)}
                    />
                  </Field>
                  <Field label="配置">
                    <Select
                      ariaLabel={`${item.label}配置`}
                      value={profile?.id ?? ""}
                      options={
                        list.length > 0
                          ? list.map((entry) => ({ value: entry.id, label: entry.label }))
                          : [{ value: "", label: "—" }]
                      }
                      onChange={(value) =>
                        spec && pickProfile(item.kind, spec.id, value)
                      }
                    />
                  </Field>
                </div>
                <span className={styles.hint}>
                  {spec ? `${spec.label}：${profile?.label ?? "默认"}` : "沿用会话模型"}
                </span>
              </section>
            );
          })}
        </div>
      </div>
    );
  }

  /* ---------------- 模型配置（清单） ---------------- */

  return (
    <div className={styles.page}>
      <nav className={styles.tabs} role="tablist">
        <button type="button" role="tab" aria-selected={false} onClick={() => setTab("pick")}>
          模型选择
        </button>
        <button type="button" role="tab" aria-selected onClick={() => setTab("manage")}>
          模型配置
        </button>
      </nav>

      <div className={styles.list}>
        {models.map((model) => {
          const used = usedBy(model.id);
          return (
            <div key={model.id} className={cx(styles.row, styles.rowStatic, used.length > 0 && styles.rowOn)}>
              {/* 勾在最左：见本文件顶部说明 */}
              <span className={styles.rowCheck}>{used.length > 0 && <Check size={10} />}</span>
              <button
                type="button"
                className={styles.rowMain}
                onClick={() => {
                  setDraft(null);
                  setOpenId(model.id);
                }}
              >
                <span className={styles.rowName}>{model.label}</span>
                <span className={styles.rowMeta}>{model.provider}</span>
                <span className={styles.rowMeta}>
                  {used.length > 0 ? used.map(kindLabel).join(" · ") : kindLabel(kindOf(model))}
                </span>
              </button>
              <Tooltip label="设置">
                <Button
                  variant="quiet"
                  size="xs"
                  icon
                  aria-label={`打开 ${model.label} 的设置`}
                  onClick={() => {
                    setDraft(null);
                    setOpenId(model.id);
                  }}
                >
                  <Gear size={11} />
                </Button>
              </Tooltip>
            </div>
          );
        })}

        {/* 添加是列表的最后一项：这一页就是「模型清单」 */}
        <button
          type="button"
          className={`${styles.row} ${styles.rowAdd}`}
          onClick={() => {
            setOpenId(null);
            setDraft(blank("chat", ai.baseUrl));
          }}
        >
          <span className={styles.rowName}>添加模型</span>
        </button>
      </div>
    </div>
  );
}

/**
 * 某套配置是否就是某个用途正在用的那套。
 *
 * 老数据没有 profileId：那时每个模型只有一套配置，把它当作生效的那条。
 */
function checkProfile(choice: AuxModelChoice, modelId: string, profileId: string): boolean {
  if (choice.model !== modelId) return false;
  if (choice.profileId) return choice.profileId === profileId;
  const spec = findModel(modelId);
  if (!spec) return false;
  const first = listProfiles(spec)[0];
  return first?.id === profileId;
}

/* ==========================================================================
   一组配置的字段
   ========================================================================== */

function ProfileFields({
  model,
  profile,
  live,
  onPatch,
}: {
  model: ModelConfig;
  profile: ModelProfile;
  live: boolean;
  onPatch: (patch: Partial<ModelProfile>) => void;
}) {
  const kind = kindOf(model);
  return (
    <section className={styles.group}>
      <span className={styles.groupTitle}>
        配置内容 · {profile.label}
        {live ? " · 正在使用" : ""}
      </span>
      <div className={styles.fields}>
        <Field label="配置名称">
          <Input
            value={profile.label}
            onChange={(event: ChangeEvent<HTMLInputElement>) =>
              onPatch({ label: event.target.value })
            }
          />
        </Field>
        <Field label="上下文长度">
          <Select
            ariaLabel="上下文长度"
            value={String(profile.contextWindow ?? model.contextWindow)}
            options={contextOptions(profile.contextWindow ?? model.contextWindow)}
            onChange={(value) => onPatch({ contextWindow: Number(value) })}
          />
        </Field>
        <Field label="接口地址" wide>
          <Input
            value={profile.baseUrl}
            spellCheck={false}
            placeholder="https://your-gateway/v1"
            onChange={(event) => onPatch({ baseUrl: event.target.value })}
          />
        </Field>
        <Field label="API Key" wide>
          <PasswordInput
            value={profile.apiKey}
            spellCheck={false}
            placeholder="sk-…"
            onChange={(event) => onPatch({ apiKey: event.target.value })}
          />
        </Field>
        {kind !== "ocr" && (
          <Field label="温度">
            <Input
              value={String(profile.temperature ?? model.defaultTemperature)}
              inputMode="decimal"
              onChange={(event) => {
                if (event.target.value.trim() === "") return;
                const next = Number(event.target.value.replace(/[^\d.]/g, ""));
                onPatch({
                  temperature: Number.isFinite(next) ? Math.min(2, Math.max(0, next)) : undefined,
                });
              }}
            />
          </Field>
        )}
        {kind !== "ocr" && model.reasoning.length > 0 && (
          <Field label="思维强度">
            <Select
              ariaLabel="思维强度"
              value={profile.reasoning ?? model.reasoning[0] ?? ""}
              options={model.reasoning.map((level) => ({ value: level, label: level }))}
              onChange={(value) => onPatch({ reasoning: value })}
            />
          </Field>
        )}
      </div>
    </section>
  );
}

/* ==========================================================================
   新建模型时的第一组配置
   ========================================================================== */

function DraftProfile({ draft, onCommit }: { draft: ModelConfig; onCommit: () => void }) {
  const [profile, setProfile] = useState<ModelProfile>(() => ({
    ...defaultProfile(draft),
    label: "默认",
  }));

  return (
    <>
      <section className={styles.group}>
        <span className={styles.groupTitle}>第一组配置</span>
        <div className={styles.fields}>
          <Field label="配置名称">
            <Input
              value={profile.label}
              onChange={(event: ChangeEvent<HTMLInputElement>) =>
                setProfile({ ...profile, label: event.target.value })
              }
            />
          </Field>
          <Field label="上下文长度">
            <Select
              ariaLabel="上下文长度"
              value={String(profile.contextWindow ?? draft.contextWindow)}
              options={contextOptions(profile.contextWindow ?? draft.contextWindow)}
              onChange={(value) => setProfile({ ...profile, contextWindow: Number(value) })}
            />
          </Field>
          <Field label="接口地址" wide>
            <Input
              value={profile.baseUrl}
              spellCheck={false}
              placeholder="https://your-gateway/v1"
              onChange={(event) => setProfile({ ...profile, baseUrl: event.target.value })}
            />
          </Field>
          <Field label="API Key" wide>
            <PasswordInput
              value={profile.apiKey}
              spellCheck={false}
              placeholder="sk-…"
              onChange={(event) => setProfile({ ...profile, apiKey: event.target.value })}
            />
          </Field>
        </div>
      </section>

      <div className={styles.actions}>
        <Button variant="primary" disabled={!draft.id.trim()} onClick={onCommit}>
          添加
        </Button>
      </div>
    </>
  );
}

/** 下拉档位：模型自带的值如果不在预设里，补一条进去，免得显示成空 */
function contextOptions(value: number) {
  const options: { value: string; label: string }[] = CONTEXT_PRESETS.map((item) => ({
    value: String(item.value),
    label: String(item.label),
  }));
  if (!options.some((item) => item.value === String(value))) {
    options.push({ value: String(value), label: `${Math.round(value / 1000)}K` });
  }
  return options;
}
