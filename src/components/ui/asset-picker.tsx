import { useEffect, useRef, useState } from "react";
import type { Asset } from "@/types";
import { Close, Image as ImageIcon, Plus } from "@/components/ui/icons";
import { dropAsset, resolveUrl, saveAsset } from "@/lib/assets";
import { useToast } from "@/components/ui/toast-context";
import styles from "./asset-picker.module.css";

/**
 * 字段里的图片位。
 *
 * 存在的理由：证件照、作品截图、证书扫描件在用户眼里就是「页面上的一项内容」。
 * 以前只在材料区上传、由生成器猜哪张当头像 —— 猜错时用户没有任何办法纠正。
 *
 * 交互约定：
 *   - 没有图时是一个小图标按钮，不占行宽
 *   - 有图时按钮本身就是缩略图，一眼能看出这一项是图
 *   - 选图、换图、去掉都在浮层里完成，不跳页
 */

/** 已解析过的地址缓存：同一个文件不重复读盘 */
const cache = new Map<string, string>();

function AssetThumb({ asset, className }: { asset: Asset; className?: string }) {
  const [url, setUrl] = useState<string | null>(() => cache.get(asset.stored) ?? null);

  useEffect(() => {
    // 缓存命中就直接返回：初始化时已经取过一份，不必再 setState 一次
    if (cache.has(asset.stored)) return;
    let alive = true;
    void resolveUrl(asset).then((value) => {
      if (!alive || !value) return;
      cache.set(asset.stored, value);
      setUrl(value);
    });
    return () => {
      alive = false;
    };
  }, [asset]);

  if (!url) return <span className={styles.thumbEmpty}>图</span>;
  return <img className={className} src={url} alt={asset.name} />;
}

export function AssetPicker({
  assets,
  value,
  onPick,
  onAdd,
  disabled,
}: {
  /** 这个人名下的全部材料；只有图片会出现在候选里 */
  assets: Asset[];
  /** 当前引用，空表示这一项不是图片 */
  value?: string;
  onPick: (assetId: string | undefined) => void;
  /** 上传新图后把新材料交出去，由上层并进 state */
  onAdd: (assets: Asset[]) => void;
  disabled?: boolean;
}) {
  const toast = useToast();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const boxRef = useRef<HTMLDivElement | null>(null);
  const fileRef = useRef<HTMLInputElement | null>(null);

  // 点外面收起。用 pointerdown 而不是 click：拖动字段行时不会误收起
  useEffect(() => {
    if (!open) return;
    const onDown = (event: PointerEvent) => {
      if (!boxRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const images = assets.filter((asset) => asset.kind === "image");
  const current = value ? images.find((asset) => asset.id === value) : undefined;

  const upload = async (files: FileList | null) => {
    if (!files?.length) return;
    setBusy(true);
    try {
      const built: Asset[] = [];
      for (const file of Array.from(files)) built.push(await saveAsset(file));
      const picked = built.find((asset) => asset.kind === "image");
      onAdd(built);
      if (picked) {
        onPick(picked.id);
        setOpen(false);
      } else {
        toast("这些文件不是图片，已经放进材料区");
      }
    } catch (error) {
      toast(error instanceof Error ? error.message : "加入图片失败");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={styles.wrap} ref={boxRef}>
      <button
        type="button"
        className={current ? styles.thumbBtn : styles.trigger}
        data-on={open || undefined}
        disabled={disabled}
        title={current ? `已插入「${current.name}」` : "把这一项换成图片"}
        aria-label={current ? "更换图片" : "插入图片"}
        onClick={() => setOpen((prev) => !prev)}
      >
        {current ? (
          <AssetThumb asset={current} />
        ) : (
          <ImageIcon size={11} />
        )}
      </button>

      {open && (
        <div className={styles.pop} role="dialog" aria-label="选择图片">
          {images.length === 0 ? (
            <p className={styles.empty}>
              还没有图片。
              <br />
              上传一张，它就是这一项的内容。
            </p>
          ) : (
            <div className={styles.grid}>
              {images.map((asset) => (
                <button
                  key={asset.id}
                  type="button"
                  className={styles.cell}
                  data-on={asset.id === value || undefined}
                  title={asset.name}
                  onClick={() => {
                    onPick(asset.id);
                    setOpen(false);
                  }}
                >
                  <AssetThumb asset={asset} />
                </button>
              ))}
            </div>
          )}

          <div className={styles.foot}>
            <button
              type="button"
              className={styles.btn}
              disabled={busy}
              onClick={() => fileRef.current?.click()}
            >
              <Plus size={9} /> {busy ? "正在加入…" : "上传图片"}
            </button>
            {value && (
              <button
                type="button"
                className={styles.btn}
                onClick={() => {
                  const asset = assets.find((item) => item.id === value);
                  // 只摘引用，不删文件：同一张图可能还被别的页用着
                  if (asset) void dropAsset(asset);
                  onPick(undefined);
                  setOpen(false);
                }}
              >
                <Close size={9} /> 去掉
              </button>
            )}
          </div>
        </div>
      )}

      <input
        ref={fileRef}
        type="file"
        accept="image/*"
        hidden
        onChange={(event) => {
          void upload(event.target.files);
          event.target.value = "";
        }}
      />
    </div>
  );
}
