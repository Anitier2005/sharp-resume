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
  DEFAULT_RESERVE,
  MODEL_KINDS,
  RESERVE_PRESETS,
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
import {
  Button,
  Field,
  Input,
  List,
  ListAdd,
  ListEmpty,
  ListRow,
  PasswordInput,
  Select,
  Tooltip,
} from "@/components/ui";
import { cx } from "@/lib/cx";
import { Confirm } from "@/components/ui/confirm";
import { Check, Trash } from "@/components/ui/icons";
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
  /** 详情页里正在看哪一组配置（列表点进来时直接落到那一组） */
  const [openProfileId, setOpenProfileId] = useState<string | null>(null);
  /** 详情页内的分区：模型本身 / 这组配置。一次只看一段，免得一页长滚动 */
  const [pane, setPane] = useState<"model" | "config">("config");
  const [draft, setDraft] = useState<ModelConfig | null>(null);
  const [dropModel, setDropModel] = useState<ModelConfig | null>(null);
  /** 删配置要记着它属于哪个模型：列表里删的时候手上没有 editing */
  const [dropProfile, setDropProfile] = useState<{
    model: ModelConfig;
    profile: ModelProfile;
  } | null>(null);
  /** localStorage 写完之后用它触发重读 */
  const [tick, setTick] = useState(0);

  const models = useMemo(() => listAllModels(), [tick]);
  /**
   * 「模型 + 配置」组合：一个组合就是一个选项，名字统一是「模型名：配置名」。
   * 列表与下拉都从这里取，免得两个地方各自拼一遍名字、拼出两种写法。
   */
  const combos = useMemo(
    () => models.flatMap((model) => listProfiles(model).map((profile) => ({ model, profile }))),
    [models],
  );
  const editing = draft ?? (openId ? (findModel(openId) ?? null) : null);
  const isDraft = draft !== null;
  const custom = editing !== null && (isDraft || isCustomModel(editing.id));

  const profiles = useMemo(
    () => (editing && !isDraft ? listProfiles(editing) : []),
    // tick 变了说明磁盘上的配置变了，必须重算
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [editing, tick, isDraft],
  );
  /** 详情页当前看的那组配置：从列表点进来的优先，否则退回第一组 */
  const selectedProfile = profiles.find((item) => item.id === openProfileId) ?? profiles[0];

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

  /** 某组配置被哪几类用途在用：勾与「会话 · 压缩」小字都靠它 */
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

  /** 下拉里一个值 = 「模型|配置」。空串是「不使用」 */
  const pickCombo = (kind: ModelKind, packed: string) => {
    if (!packed) {
      writeChoice(kind, null);
      return;
    }
    const [modelId, profileId] = packed.split("|");
    pickProfile(kind, modelId, profileId);
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

  /**
   * 删配置的确认窗：列表页和详情页都会用到，所以做成一段共用的 JSX。
   * 手上必须带模型 —— 从列表删的时候没有 editing 这个上下文。
   */
  const dropProfileDialog = dropProfile ? (
    <Confirm
      open
      title={`删除配置「${dropProfile.profile.label}」？`}
      description="只删这一组网关与密钥，模型本身保留；如果这是最后一组，会补一套空配置，免得模型变成点不进去的空壳。"
      onCancel={() => setDropProfile(null)}
      onConfirm={() => {
        const { model, profile } = dropProfile;
        removeProfile(model.id, profile.id);
        // 组合列表是按配置展开的：一组都不剩，这个模型就从所有清单里消失了，
        // 但它还在模型清单里占一行 —— 补回一套默认配置，让它仍然可配
        if (listProfiles(model).length === 0) saveProfile(model.id, defaultProfile(model));
        setDropProfile(null);
        setTick((value) => value + 1);
      }}
    />
  ) : null;

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
                {/* 一个下拉列完所有「模型：配置」：分成两个下拉时，
                    选了模型还得再选一次配置，而多数模型只有一组配置 */}
                <Field label="模型 · 一个模型加一组配置算一个选项">
                  <Select
                    ariaLabel={`${item.label}模型`}
                    value={choice && profile ? `${choice.model}|${profile.id}` : ""}
                    options={[
                      ...(item.kind === "chat" ? [] : [{ value: "", label: "不使用" }]),
                      ...combos.map((combo) => ({
                        value: `${combo.model.id}|${combo.profile.id}`,
                        label: `${combo.model.label}：${combo.profile.label}`,
                      })),
                    ]}
                    onChange={(value) => pickCombo(item.kind, value)}
                  />
                </Field>
                {spec && (
                  <span className={styles.hint}>
                    {spec.label} · {spec.provider} · {profile?.baseUrl || spec.baseUrl}
                  </span>
                )}
              </section>
            );
          })}
        </div>
      </div>
    );
  }

  /* ---------------- 模型配置：左列模型、右列配置 ---------------- */

  /** 模型字段（显示名称 / 接口 model 字段 / 用途 / 思维强度），两侧共用一份 */
  const modelFields = editing ? (
    <div className={styles.fields}>
      <Field label="显示名称">
        <Input
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
  ) : null;

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

      <div className={styles.cols}>
        {/* 左列：模型。选一个模型，右列才长出来 —— 和知识库的「库 → 条目」同一套读法 */}
        <section className={cx(styles.col, styles.colLeft)}>
          <div className={styles.colHead}>
            <span className={styles.colTitle}>模型</span>
          </div>
          <List className={styles.colList}>
            <ListAdd
              label="添加模型"
              onClick={() => {
                setOpenId(null);
                setOpenProfileId(null);
                setDraft(blank("chat", ai.baseUrl));
              }}
            />
            {models.map((model) => {
              const live = listProfiles(model).some(
                (profile) => profileUsedBy(model.id, profile.id).length > 0,
              );
              return (
                <ListRow
                  key={model.id}
                  active={!isDraft && openId === model.id}
                  leading={live ? <Check size={10} /> : undefined}
                  title={model.label}
                  meta={model.provider}
                  onSelect={() => {
                    setDraft(null);
                    setPane("config");
                    setOpenId(model.id);
                    setOpenProfileId(null);
                  }}
                  actions={
                    isCustomModel(model.id) ? (
                      <Tooltip label="删除模型">
                        <Button
                          variant="quiet"
                          size="xs"
                          icon
                          aria-label={`删除模型 ${model.label}`}
                          onClick={() => setDropModel(model)}
                        >
                          <Trash size={10} />
                        </Button>
                      </Tooltip>
                    ) : undefined
                  }
                />
              );
            })}
          </List>
        </section>

        {/* 右列：这个模型的配置。没选模型时整列不出现，不留空壳 */}
        {editing && (
          <section className={cx(styles.col, styles.colRight)}>
            <div className={styles.colHead}>
              <span className={styles.colTitle}>{isDraft ? "添加模型" : editing.label}</span>
              {!isDraft && <span className={styles.colNote}>{kindLabel(kindOf(editing))}</span>}
            </div>

            <div className={styles.colTabs} role="tablist">
              <button
                type="button"
                role="tab"
                aria-selected={isDraft || pane === "config"}
                onClick={() => setPane("config")}
              >
                配置
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={!isDraft && pane === "model"}
                disabled={isDraft}
                onClick={() => setPane("model")}
              >
                模型
              </button>
            </div>

            <div className={styles.colBody}>
              {isDraft ? (
                <DraftProfile draft={draft!} onCommit={commitDraft} />
              ) : pane === "model" ? (
                modelFields
              ) : (
                <>
                  <List>
                    <ListAdd label="添加配置" onClick={addProfile} />
                    {profiles.map((profile) => {
                      const used = profileUsedBy(editing.id, profile.id);
                      return (
                        <ListRow
                          key={profile.id}
                          active={profile.id === selectedProfile?.id}
                          leading={used.length > 0 ? <Check size={10} /> : undefined}
                          title={profile.label}
                          meta={profile.baseUrl || "沿用网关"}
                          /* 点配置行 = 看这一组的字段；换用途是「模型选择」页的事 */
                          onSelect={() => setOpenProfileId(profile.id)}
                          actions={
                            <Tooltip label="删除这组配置">
                              <Button
                                variant="quiet"
                                size="xs"
                                icon
                                aria-label={`删除配置 ${profile.label}`}
                                onClick={() => setDropProfile({ model: editing, profile })}
                              >
                                <Trash size={10} />
                              </Button>
                            </Tooltip>
                          }
                        />
                      );
                    })}
                    {profiles.length === 0 && <ListEmpty>还没有配置，点上面那行</ListEmpty>}
                  </List>

                  {/* 只显示选中那一组的字段：四组配置全摊开时，改一个网关要滚半屏 */}
                  {selectedProfile && (
                    <ProfileFields
                      key={selectedProfile.id}
                      model={editing}
                      profile={selectedProfile}
                      live={profileUsedBy(editing.id, selectedProfile.id).length > 0}
                      onPatch={(patch) => patchProfile(selectedProfile, patch)}
                    />
                  )}
                </>
              )}
            </div>
          </section>
        )}
      </div>

      {dropModel && (
        <Confirm
          open
          title={`删除模型「${dropModel.label}」？`}
          description="模型和它下面的所有配置都会从这台机器上移除；用到它的用途会回退。"
          onCancel={() => setDropModel(null)}
          onConfirm={() => drop(dropModel)}
        />
      )}

      {dropProfileDialog}
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
        <Field
          label="压缩预留"
          hint="从窗口里先扣掉，留给压缩提示词；剩下的才是历史能用的部分"
        >
          <Select
            ariaLabel="压缩预留"
            value={String(profile.reserve ?? DEFAULT_RESERVE)}
            options={reserveOptions(profile.reserve ?? DEFAULT_RESERVE)}
            onChange={(value) => onPatch({ reserve: Number(value) })}
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
          <Field
            label="压缩预留"
            hint="窗口减去它，才是历史能占的 token"
          >
            <Select
              ariaLabel="压缩预留"
              value={String(profile.reserve ?? DEFAULT_RESERVE)}
              options={reserveOptions(profile.reserve ?? DEFAULT_RESERVE)}
              onChange={(value) => setProfile({ ...profile, reserve: Number(value) })}
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

/** 压缩预留的档位，同 contextOptions：存量值不在预设里就补一条 */
function reserveOptions(value: number) {
  const options: { value: string; label: string }[] = RESERVE_PRESETS.map((item) => ({
    value: String(item.value),
    label: String(item.label),
  }));
  if (!options.some((item) => item.value === String(value))) {
    options.push({ value: String(value), label: `${Math.round(value / 1000)}K` });
  }
  return options;
}
