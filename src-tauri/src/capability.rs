//! 模型能力：这个模型到底能不能收图。
//!
//! 为什么不能靠「设置里勾一个开关」：勾的是**用户以为**的能力，不是**这一条链路实测**的
//! 能力。用户换了 key、换了网关、换了模型，那个勾还留在原地 —— 于是要么明明能看图却一直
//! 只发元数据（用户看到的症状：照片永远进不了简历），要么明明不能看却把图发过去换回一个
//! 400。所以这里只认实测：探针答对了才算支持，协议层明确拒收才算不支持，其余一律「没测出来」。
//!
//! 三态与两条不对称的代价：
//!   - 误判成 `unsupported` → 从此不给图、工具永久拒绝，用户再也拿不回看图能力，
//!     而界面上还写着「这个模型看不了图」—— 那是我们编的。**不可接受**。
//!   - 长期显示「没测出来」→ 难看，最坏是用户多点一次测试。**可接受**。
//! 所以拿不准的一律 `unknown`。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// settings 表里的键。一条记录装下全部模型（按指纹索引），
/// 换 key / 换模型 / 换网关都只是「指纹换了」，不需要清理逻辑。
pub const KEY: &str = "capability:v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Supported,
    Unsupported,
    /// 没测出来 —— 既不能当支持，也不能当不支持
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    /// 没有异常，`state` 自己就说明白了
    None,
    /// 协议层明确拒图：强证据，但**可逆**（后写胜）
    ProtocolReject,
    /// 密钥不对 / 没有权限
    Auth,
    /// 余额或配额用完（不是能力问题，别让人以为模型看不了图）
    Quota,
    BadRequest,
    Offline,
    Timeout,
    /// 没测过，或者测了但答错 —— 两者结论相同（不知道），数据上分开只是为了界面能说
    /// 「3 天前测过、答错了」而不是「从没测过」
    NoProbe,
}

impl Reason {
    /// 只给【模型能力】事实块用的两种说法。**不要**在这里加第三种：
    /// 模型看到的原因越花，它越容易自己发明一句「因为你的模型是老版本」。
    pub fn text(self) -> &'static str {
        match self {
            Reason::ProtocolReject => "模型在协议层明确拒收图像",
            // unsupported 只该有两种原因；其余的落到这里只是为了穷尽 match
            _ => "还没测过",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub state: State,
    pub reason: Reason,
    /// unix 秒。只用于界面说「3 天前测过」
    #[serde(default)]
    pub checked_at: Option<i64>,
}

impl Default for Capability {
    fn default() -> Self {
        Self {
            state: State::Unknown,
            reason: Reason::NoProbe,
            checked_at: None,
        }
    }
}

impl Capability {
    pub fn can_see(&self) -> bool {
        self.state == State::Supported
    }
}

/* ==========================================================================
   库：本模块自己开一条连接
   ========================================================================== */

/// 与主库同文件的第二条连接（WAL 模式，`db::open` 已开 busy_timeout）。
///
/// 为什么不从命令参数把 `db::Shared` 传下来：能力判定要在三处用到（材料组装、
/// 工具出站、协议层改判），其中**协议层在 `net.rs` 里，那里根本没有 AppHandle**。
/// 为了一行 `- 你能看图吗` 的判定去给 `net::stream` 加参数、再让 `turn.rs` /
/// `layout.rs` 全部跟着改，是把一个纯查询变成贯穿调用栈的管道。这里放一个进程级
/// 句柄，只做 settings 的读与写，不碰任何业务表。
static DB: OnceLock<crate::db::Shared> = OnceLock::new();

/// 启动时调一次（`lib.rs` 的 setup）。重复调用无副作用。
pub fn init(path: PathBuf) {
    let _ = DB.set(crate::db::Shared::new(path));
}

fn with_db<T>(f: impl FnOnce(&mut rusqlite::Connection) -> Result<T, String>) -> Option<T> {
    let Some(db) = DB.get() else {
        eprintln!("[capability] 数据库没初始化（setup 没跑到？）");
        return None;
    };
    match db.with(f) {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!("[capability] 设置读写失败：{error}");
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    state: State,
    reason: Reason,
    checked_at: i64,
    model: String,
    base_url: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    items: BTreeMap<String, Entry>,
}

fn load() -> Store {
    with_db(|conn| {
        let all = crate::db::settings_all(conn)?;
        Ok(all
            .into_iter()
            .find(|(key, _)| key == KEY)
            .map(|(_, value)| value))
    })
    .flatten()
    .and_then(|text| serde_json::from_str::<Store>(&text).ok())
    .unwrap_or_default()
}

/// 读-改-写的互斥锁。
///
/// 一条 settings 记录里装着**所有**模型，所以每次写入都是「读全量 → 改一条 → 写回」。
/// 没有这把锁，两轮对话同时触发探针时后写的一方会把先写的那条整个冲掉 ——
/// 症状是「明明测过，下次打开又变成没测出来」，而且偶发、几乎不可能复现。
/// 锁只在进程内有效；跨进程（同时开两个实例）仍然是后写胜，那已经是最轻的损失：
/// 丢一条能力结论只会让用户再测一次，不会让数据坏掉。
static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn store(fingerprint: &str, cap: &Capability, base_url: &str, model: &str) {
    let _guard = WRITE.lock();
    let mut all = load();
    all.version = 1;
    all.items.insert(
        fingerprint.to_string(),
        Entry {
            state: cap.state,
            reason: cap.reason,
            checked_at: cap.checked_at.unwrap_or_else(now),
            model: model.to_string(),
            base_url: base_url.to_string(),
        },
    );
    if let Ok(text) = serde_json::to_string(&all) {
        let _ = with_db(|conn| crate::db::settings_put(conn, KEY, &text));
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/* ==========================================================================
   指纹与判定
   ========================================================================== */

/// 能力查询/探测的三元组（网关 + 模型 + 密钥）。
///
/// 为什么用一个结构体当命令参数而不是三个散参数：散参数在 IPC 上要靠
/// camelCase↔snake_case 的自动转换约定，写错一个字段名不会报错，只会静默拿到
/// 空字符串 —— 那会算出一个错误的指纹，而错误的指纹看起来和「没测过」一模一样。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conf {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

/// `hex(sha256("{baseUrl}\n{model}\n{apiKey}"))[..16]`
///
/// 为什么不存密钥本体：这是设置表，会被用户拷来拷去、也可能随日志外泄。
/// 16 hex = 64 bit，做「同一组配置」的索引足够，碰撞也只会让两个模型的结论串台 ——
/// 而串台的后果是「把 A 模型的探针结论套给 B」，这仍然是**实测值**，不是编的。
///
/// baseUrl 也进指纹：同一个模型走不同网关，能收图与否完全可能不同。
pub fn fingerprint(base_url: &str, model: &str, api_key: &str) -> String {
    let digest = crate::pool::sha256_hex(
        format!("{}\n{}\n{}", base_url.trim(), model.trim(), api_key.trim()).as_bytes(),
    );
    digest.chars().take(16).collect()
}

/// **唯一**的「这个模型能不能看图」判定入口。只读设置，不发请求。
///
/// 把探测塞进这里会让每轮对话都可能多一次模型调用 —— 而探测是显式动作
/// （设置页那个「测一次多模态」按钮，或收图时撞上协议层拒图的一次改判）。
pub fn resolve(base_url: &str, model: &str, api_key: &str) -> Capability {
    let fp = fingerprint(base_url, model, api_key);
    match load().items.get(&fp) {
        Some(entry) => Capability {
            state: entry.state,
            reason: entry.reason,
            checked_at: Some(entry.checked_at),
        },
        // 读不到 / 表空 / 解析失败 / 指纹不命中，全部走这里。
        // **绝不能**回退成 Unsupported —— 那会让「换了 key 还没测」变成「不能看图」。
        None => Capability::default(),
    }
}

/// 这一轮该不该自动探一次。
///
/// 为什么需要这个判断：`unknown` 不发图，于是「没测过」会让能看图的模型永远只收到
/// 元数据 —— 那就是这条链路坏了几版本的病根，所以第一次需要图片时必须自动测。
/// 但不能每轮都测：结论是「额度用完」时每轮再测一次只是白花一次请求，
/// 而且会拖慢每一轮对话。规则分三种：
///   - 从没测过 → 测；
///   - 上次是网络/超时（一分钟后）→ 再测，因为那是暂时的；
///   - 上次有结论、或者卡在配置问题（密钥/额度/模型名）→ 不测，等用户去点按钮。
pub fn should_auto_probe(cap: &Capability) -> bool {
    match cap.checked_at {
        None => true,
        Some(at) => {
            let stale = now().saturating_sub(at) > 60;
            stale && matches!(cap.reason, Reason::Offline | Reason::Timeout)
        }
    }
}

/// 协议层明确拒图时改判。**可逆**：后写胜，探针拿到 `supported` 就覆盖它。
///
/// 为什么单独一个入口而不是 `store(state=Unsupported)`：改判与探针的证据强度不同，
/// 混在一起以后就分不清「这是模型拒的」还是「这是我们推的」。
pub fn mark_protocol_reject(base_url: &str, model: &str, api_key: &str) -> Capability {
    let cap = Capability {
        state: State::Unsupported,
        reason: Reason::ProtocolReject,
        checked_at: Some(now()),
    };
    store(&fingerprint(base_url, model, api_key), &cap, base_url, model);
    cap
}

/* ==========================================================================
   探针
   ========================================================================== */

/// 探针用的六个色名，与图上的 RGB 一一对应。
/// 顺序即索引：`TRUTH[i]` 与 `COLORS[i]` 同一行。
const COLORS: [(&str, [u8; 3]); 6] = [
    ("红", [214, 40, 40]),
    ("绿", [26, 158, 66]),
    ("蓝", [32, 64, 214]),
    ("黄", [244, 224, 48]),
    ("黑", [16, 16, 16]),
    ("白", [248, 248, 248]),
];

const BLOCK: u32 = 64;
const GAP: u32 = 4;
const GRID: u32 = 3;

/// 画一张 3×3 色块图，返回（9 个真值色名，png data URL）。
///
/// 为什么 ground truth 随机：写死一张图会让模型背答案，也会让「缓存了这张图的网关」
/// 显得能看图 —— 那测的就不是这条链路了。
///
/// 取色方式：先把 6 个颜色洗牌，再按 `i % 6` 铺 9 格。这保证六种颜色每种至少出现一次、
/// 最多两次。纯 `rand % 6` 会出现「九宫格全红」那种图，猜都能猜对一半；
/// 这个铺法必须真的按位置读色块才能全对。
fn probe_image() -> (Vec<&'static str>, String) {
    let mut order = [0usize, 1, 2, 3, 4, 5];
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15)
        | 1;
    // Fisher-Yates，随机源用 xorshift64*（只为打乱六个元素，不需要密码学强度；
    // 为此拉进一个 rand 依赖不值得）
    for index in (1..order.len()).rev() {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        let pick = (seed.wrapping_mul(0x2545_f491_4f6c_dd1d) % (index as u64 + 1)) as usize;
        order.swap(index, pick);
    }

    let truth: Vec<&'static str> = (0..(GRID * GRID) as usize)
        .map(|index| COLORS[order[index % COLORS.len()]].0)
        .collect();

    let side = GRID * BLOCK + (GRID + 1) * GAP;
    // 底色用浅灰而不是纯白：白块和白底之间那条 4px 缝隙才看得出来
    let mut img = image::RgbImage::from_pixel(side, side, image::Rgb([236, 236, 236]));
    for (index, name) in truth.iter().enumerate() {
        let rgb = COLORS
            .iter()
            .find(|(label, _)| label == name)
            .map(|(_, rgb)| *rgb)
            .unwrap_or([0, 0, 0]);
        let row = index as u32 / GRID;
        let col = index as u32 % GRID;
        let x0 = GAP + col * (BLOCK + GAP);
        let y0 = GAP + row * (BLOCK + GAP);
        for y in y0..y0 + BLOCK {
            for x in x0..x0 + BLOCK {
                img.put_pixel(x, y, image::Rgb(rgb));
            }
        }
    }

    let mut png: Vec<u8> = Vec::new();
    // `write_image` 在 ImageEncoder trait 上，不在 PngEncoder 上
    use image::ImageEncoder;
    let encoded = image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            img.as_raw(),
            side,
            side,
            image::ExtendedColorType::Rgb8,
        )
        .is_ok();

    // 编码失败不该让探针「判不支持」—— 那会把编码器的问题算到模型头上。
    // 返回空串，调用方看到空串直接给 unknown。
    if !encoded {
        return (truth, String::new());
    }
    (truth, crate::pdfimage::data_url_of("image/png", &png))
}

/// 从模型的回答里按**出现顺序**抽出颜色词，取前 9 个。
///
/// 按顺序扫而不是按逗号切：模型会答成「第一行：红、绿、蓝」、也可能加换行、
/// 也可能答英文。找词比猜格式稳。
fn parse_colors(text: &str) -> Vec<&'static str> {
    const SYNONYMS: [(&str, usize); 18] = [
        ("红色", 0),
        ("红", 0),
        ("red", 0),
        ("绿色", 1),
        ("绿", 1),
        ("green", 1),
        ("蓝色", 2),
        ("蓝", 2),
        ("blue", 2),
        ("黄色", 3),
        ("黄", 3),
        ("yellow", 3),
        ("黑色", 4),
        ("黑", 4),
        ("black", 4),
        ("白色", 5),
        ("白", 5),
        ("white", 5),
    ];
    let lowered = text.to_lowercase();
    let mut hits: Vec<(usize, usize)> = Vec::new(); // (位置, 颜色索引)
    for (word, index) in SYNONYMS {
        let mut from = 0;
        while let Some(found) = lowered[from..].find(word) {
            let at = from + found;
            hits.push((at, index));
            // 「红色」命中后不要再让「红」在同一位置命中一次
            from = at + word.len();
        }
    }
    hits.sort_by_key(|(at, _)| *at);
    // 同一位置取最长的那个词：`红色` 与 `红` 起点相同，只算一次
    let mut out: Vec<&'static str> = Vec::new();
    let mut last_end = 0usize;
    for (at, index) in hits {
        if at < last_end {
            continue;
        }
        out.push(COLORS[index].0);
        last_end = at + 1;
        if out.len() == (GRID * GRID) as usize {
            break;
        }
    }
    out
}

const PROBE_PROMPT: &str = "这张图是 3 行 3 列的色块，按「第1行从左到右, 第2行从左到右, 第3行从左到右」的顺序，用 红/绿/蓝/黄/黑/白 六个词回答，逗号分隔，不要解释。";

/// 探一次，写回设置，返回结论。
///
/// 探针必须走**与线上完全一致**的多模态消息形状（`user_with_images` 走的
/// `content: [{type:text},{type:image_url}]`）—— 否则测的是另一条路，
/// 探针说「能看图」而真实请求被拒，等于白测。
pub async fn probe(base_url: &str, model: &str, api_key: &str) -> Capability {
    let (truth, data_url) = probe_image();
    if data_url.is_empty() {
        let cap = Capability {
            state: State::Unknown,
            reason: Reason::BadRequest,
            checked_at: Some(now()),
        };
        store(&fingerprint(base_url, model, api_key), &cap, base_url, model);
        return cap;
    }

    let request = crate::net::AiRequest {
        base_url: base_url.to_string(),
        api_key: api_key.to_string(),
        model: model.to_string(),
        messages: vec![crate::net::ChatMessage::user_with_images(
            PROBE_PROMPT,
            &[data_url],
        )],
        temperature: Some(0.0),
        reasoning: None,
        max_tokens: Some(60),
        tools: None,
    };

    let cap = match crate::net::chat_once_status(request).await {
        Ok((raw, _tokens)) => {
            let got = parse_colors(&raw);
            if got.len() == truth.len() && got == truth {
                Capability {
                    state: State::Supported,
                    reason: Reason::None,
                    checked_at: Some(now()),
                }
            } else {
                // 探针答错 ≠ 不支持，必须保持 unknown。
                // 三种情况在协议层长得一模一样：模型没有视觉 / 网关把 image_url 吞了 /
                // 模型偶发降智数错了色块。
                // 误判成 unsupported 的代价：materials 从此不再给图、view_asset 永久拒绝，
                // 用户再也拿不回看图能力，而且界面上写的是"这个模型看不了图"——那是我们编的。
                // 长期显示"没测出来"的代价：难看，最坏是用户多点一次测试。
                // 两者代价不对称，所以宁可显示没测出来。
                // 唯一的例外是协议层明确拒绝（classify_error 的 ImageRejected）：强证据，且必须可逆。
                if got.len() >= 6 && overlap(&got, &truth) * 2 >= got.len() {
                    // 半对只多记一条日志：用来判断"能看图但数不清"还是"完全看不见"。
                    // 不引入新状态 —— 结论仍然是没测出来。
                    eprintln!("[capability] 半对：got={got:?} truth={truth:?}");
                } else {
                    eprintln!(
                        "[capability] 答非所图：model={model} got={got:?} raw={}",
                        raw.chars().take(120).collect::<String>()
                    );
                }
                Capability {
                    state: State::Unknown,
                    reason: Reason::NoProbe,
                    checked_at: Some(now()),
                }
            }
        }
        Err((status, body)) => {
            let kind = crate::net::classify_error(status, &body);
            // 结论之外还要留一条能 grep 的痕迹：状态、类别、错误体摘要。
            // 探针「没测出来」时用户只会问「为什么」，这条就是答案。
            eprintln!("[capability] 探针失败：status={status} kind={kind:?} body={}", body.chars().take(200).collect::<String>());
            let reason = match kind {
                // 协议层明确拒图：唯一的强证据，且可逆
                crate::net::ErrorKind::ImageRejected => Reason::ProtocolReject,
                // 网络与服务端故障都不判能力：它们下一分钟可能就好了
                crate::net::ErrorKind::Offline => Reason::Offline,
                crate::net::ErrorKind::Timeout => Reason::Timeout,
                // 密钥不对、额度用完、模型名写错 —— 都是配置问题，不是能力问题
                crate::net::ErrorKind::Auth => Reason::Auth,
                crate::net::ErrorKind::Quota => Reason::Quota,
                _ => Reason::BadRequest,
            };
            Capability {
                state: if kind == crate::net::ErrorKind::ImageRejected {
                    State::Unsupported
                } else {
                    State::Unknown
                },
                reason,
                checked_at: Some(now()),
            }
        }
    };

    // unknown 也写：否则界面只能显示「没测过」，说不出「上次测了但没得出结论」
    store(&fingerprint(base_url, model, api_key), &cap, base_url, model);
    cap
}

fn overlap(a: &[&'static str], b: &[&'static str]) -> usize {
    a.iter().zip(b.iter()).filter(|(x, y)| x == y).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一次性初始化到临时库。`OnceLock` 只能 set 一次，所以整个测试模块共用一份；
    /// 各条测试用**不同的 model 名**来获得不同指纹，互不干扰。
    fn init_once() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let dir = std::env::temp_dir().join(format!("sharpresume-cap-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            init(dir.join("app.db"));
        });
    }

    #[test]
    fn fingerprint_is_stable_and_hex16() {
        let a = fingerprint("https://x/v1", "m1", "k1");
        let b = fingerprint("https://x/v1", "m1", "k1");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn fingerprint_changes_with_every_part() {
        let base = fingerprint("https://x/v1", "m1", "k1");
        assert_ne!(base, fingerprint("https://y/v1", "m1", "k1"), "换网关必须换指纹");
        assert_ne!(base, fingerprint("https://x/v1", "m2", "k1"), "换模型必须换指纹");
        assert_ne!(base, fingerprint("https://x/v1", "m1", "k2"), "换 key 必须换指纹");
    }

    #[test]
    fn unknown_when_never_measured() {
        init_once();
        let cap = resolve("https://never.test/v1", "never-measured", "k");
        assert_eq!(cap.state, State::Unknown);
        assert_eq!(cap.reason, Reason::NoProbe);
        assert!(cap.checked_at.is_none());
        assert!(!cap.can_see());
    }

    #[test]
    fn protocol_reject_is_persisted_and_read_back() {
        init_once();
        let (url, model, key) = ("https://reject.test/v1", "reject-model", "k");
        let cap = mark_protocol_reject(url, model, key);
        assert_eq!(cap.state, State::Unsupported);
        assert_eq!(cap.reason, Reason::ProtocolReject);

        let back = resolve(url, model, key);
        assert_eq!(back.state, State::Unsupported);
        assert_eq!(back.reason, Reason::ProtocolReject);
        assert!(back.checked_at.is_some(), "要能显示「3 天前测过」");
    }

    #[test]
    fn a_later_supported_overwrites_the_reject() {
        init_once();
        let (url, model, key) = ("https://flip.test/v1", "flip-model", "k");
        mark_protocol_reject(url, model, key);
        // 探针拿到 supported 时无条件覆盖（后写胜）——
        // 否则一次误判就是永久拉黑，用户再也拿不回看图能力
        store(
            &fingerprint(url, model, key),
            &Capability {
                state: State::Supported,
                reason: Reason::None,
                checked_at: Some(now()),
            },
            url,
            model,
        );
        assert_eq!(resolve(url, model, key).state, State::Supported);
    }

    #[test]
    fn a_different_key_does_not_inherit_the_verdict() {
        init_once();
        let (url, model) = ("https://key.test/v1", "key-model");
        mark_protocol_reject(url, model, "key-a");
        // 换了 key 就是另一组配置，旧结论不许套用
        assert_eq!(resolve(url, model, "key-b").state, State::Unknown);
    }

    #[test]
    fn two_models_do_not_overwrite_each_other() {
        init_once();
        let url = "https://multi.test/v1";
        mark_protocol_reject(url, "multi-a", "k");
        store(
            &fingerprint(url, "multi-b", "k"),
            &Capability {
                state: State::Supported,
                reason: Reason::None,
                checked_at: Some(now()),
            },
            url,
            "multi-b",
        );
        assert_eq!(resolve(url, "multi-a", "k").state, State::Unsupported);
        assert_eq!(resolve(url, "multi-b", "k").state, State::Supported);
    }

    /// 真连一次：探针必须能从真网关得出**结论**（而不是永远停在没测出来）。
    ///
    /// 默认不跑（要密钥、要网、要配额）。`SHARP_LIVE=1 cargo test --lib live_` 才动。
    /// 这条是整条看图链路的端到端验收：PNG 编码 → data URL → 多模态消息形状 →
    /// 解析颜色 → 判定。中间任何一环不对，这里都拿不到 `supported`。
    #[tokio::test]
    async fn live_probe_gets_a_verdict() {
        if std::env::var("SHARP_LIVE").is_err() {
            eprintln!("跳过：没有 SHARP_LIVE");
            return;
        }
        let base: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string("../testdata/ai.json").unwrap()).unwrap();
        let secret: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string("../testdata/secrets/ai.local.json").unwrap(),
        )
        .unwrap();
        init_once();
        let cap = probe(
            base["baseUrl"].as_str().unwrap(),
            base["model"].as_str().unwrap(),
            secret["apiKey"].as_str().unwrap(),
        )
        .await;
        eprintln!("[live] capability = {cap:?}");
        // 硬不变量：配置错、额度用完、网络不通，都**不许**判成「看不了图」。
        // 这一条比「能不能拿到 supported」重要 —— 误判会永久拉黑用户的看图能力。
        assert_ne!(
            cap.state,
            State::Unsupported,
            "配置/额度问题不许判成看不了图：{cap:?}"
        );
        // 拿到 supported 才算这条链路真的通了。拿不到时区分「代码不通」与「环境不通」：
        // 402/401/超时属于环境，不该让这条用例红；其余情况说明解析或消息形状有问题。
        if cap.state != State::Supported {
            match cap.reason {
                Reason::Auth | Reason::Quota | Reason::Offline | Reason::Timeout => {
                    eprintln!("[live] 没拿到 supported：{cap:?} —— 环境问题（密钥/额度/网络），不是代码失败")
                }
                other => panic!("探针链路不通（{other:?}）：消息形状或颜色解析有一环不对：{cap:?}"),
            }
        }
    }

    #[test]
    fn auto_probe_only_when_it_can_learn_something() {
        // 从没测过 → 必须测，否则能看图的模型永远收不到图
        assert!(should_auto_probe(&Capability::default()));

        let tested = |reason: Reason, ago: i64| Capability {
            state: State::Unknown,
            reason,
            checked_at: Some(now() - ago),
        };
        // 刚测过、卡在配置问题 → 不重复测（每轮再测只是白花一次请求）
        assert!(!should_auto_probe(&tested(Reason::Quota, 1)));
        assert!(!should_auto_probe(&tested(Reason::Auth, 1)));
        assert!(!should_auto_probe(&tested(Reason::NoProbe, 1)));
        // 网络问题是暂时的，一分钟后可以再试
        assert!(!should_auto_probe(&tested(Reason::Offline, 1)), "同一分钟内不重试");
        assert!(should_auto_probe(&tested(Reason::Offline, 120)));
        assert!(should_auto_probe(&tested(Reason::Timeout, 120)));
        // 已有结论（支持/不支持）就不要再测
        let supported = Capability {
            state: State::Supported,
            reason: Reason::None,
            checked_at: Some(now() - 9999),
        };
        assert!(!should_auto_probe(&supported));
    }

    #[test]
    fn probe_image_has_nine_blocks_and_nine_truths() {
        let (truth, url) = probe_image();
        assert_eq!(truth.len(), 9);
        assert!(url.starts_with("data:image/png;base64,"));
        // 洗牌铺法保证六种颜色都出现，且没有一种超过两次
        for (name, _) in COLORS {
            let count = truth.iter().filter(|t| **t == name).count();
            assert!(count >= 1, "{name} 应当至少出现一次");
            assert!(count <= 2, "{name} 最多出现两次");
        }
    }

    #[test]
    fn probe_image_is_random() {
        // 同一进程连画两张，顺序应当不同（时间戳做种子）。
        // 偶发相同不算错，但连续 8 次全同就说明种子没在变。
        let first = probe_image().0;
        let mut differed = false;
        for _ in 0..8 {
            if probe_image().0 != first {
                differed = true;
                break;
            }
        }
        assert!(differed, "探针图必须随机，否则模型可以背答案");
    }

    #[test]
    fn parse_colors_reads_in_order() {
        assert_eq!(
            parse_colors("红,绿,蓝,黄,黑,白,红,绿,蓝"),
            vec!["红", "绿", "蓝", "黄", "黑", "白", "红", "绿", "蓝"]
        );
    }

    #[test]
    fn parse_colors_survives_formatting_noise() {
        assert_eq!(
            parse_colors("第一行：红色、绿色、蓝色\n第二行：黄色，黑色，白色\n第三行：红 绿 蓝"),
            vec!["红", "绿", "蓝", "黄", "黑", "白", "红", "绿", "蓝"]
        );
    }

    #[test]
    fn parse_colors_accepts_english() {
        assert_eq!(
            parse_colors("red, green, blue, yellow, black, white, red, green, blue"),
            vec!["红", "绿", "蓝", "黄", "黑", "白", "红", "绿", "蓝"]
        );
    }

    #[test]
    fn parse_colors_counts_red_once() {
        // 「红色」同时包含「红」，不许算成两个
        assert_eq!(parse_colors("红色"), vec!["红"]);
    }

    #[test]
    fn parse_colors_gives_up_on_chatter() {
        assert!(parse_colors("抱歉，我看不到图片内容。").is_empty());
    }

    #[test]
    fn parse_colors_caps_at_nine() {
        let noisy = "红".repeat(20);
        assert_eq!(parse_colors(&noisy).len(), 9);
    }
}
