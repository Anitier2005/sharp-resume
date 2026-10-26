import { useEffect, useRef, useState } from "react";
import type { Asset } from "@/types";
import { Close, FileIcon, Link as LinkIcon, Plus } from "@/components/ui/icons";
import { addLink, dropAsset, resolveUrl, saveAsset } from "@/lib/assets";
import { useToast } from "@/components/ui/toast-context";
import styles from "./assets.module.css";

/**
 * 材料编辑器：图片、链接、文件。
 *
 * 证件照、证书扫描件、作品外链、补充材料 PDF 都走这里 ——
 * 不给它们各起一套界面，因为对用户来说都是「贴个东西进来」。
 */
export function AssetsEditor({
  assets,
  onChange,
  addLabel = "加材料",
}: {
  assets: Asset[];
  onChange: (next: Asset[]) => void;
  addLabel?: string;
}) {
  const toast = useToast();
  const fileRef = useRef<HTMLInputElement | null>(null);
  const [linkDraft, setLinkDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const add = async (files: FileList | null) => {
    if (!files?.length) return;
    setBusy(true);
    try {
      const built: Asset[] = [];
      for (const file of Array.from(files)) {
        built.push(await saveAsset(file));
      }
      onChange([...assets, ...built]);
    } catch (error) {
      toast(error instanceof Error ? error.message : "添加材料失败");
    } finally {
      setBusy(false);
    }
  };

  const remove = (asset: Asset) => {
    onChange(assets.filter((item) => item.id !== asset.id));
    void dropAsset(asset);
  };

  return (
    <div className={styles.wrap}>
      {assets.length === 0 && !linkDraft && (
        <span className={styles.empty}>还没有材料。照片、证书、作品链接都可以放进来。</span>
      )}

      {assets.length > 0 && (
        <div className={styles.grid}>
          {assets.map((asset) => (
            <AssetCell key={asset.id} asset={asset} onRemove={() => remove(asset)} />
          ))}
        </div>
      )}

      <div className={styles.row}>
        <button
          type="button"
          className={styles.add}
          disabled={busy}
          onClick={() => fileRef.current?.click()}
        >
          <Plus size={9} /> {busy ? "正在加入…" : addLabel}
        </button>
        <button type="button" className={styles.add} onClick={() => setLinkDraft("")}>
          <LinkIcon size={9} /> 加链接
        </button>
      </div>

      {linkDraft !== null && (
        <div className={styles.linkRow}>
          <input
            autoFocus
            value={linkDraft}
            spellCheck={false}
            placeholder="https://…"
            aria-label="材料链接"
            onChange={(event) => setLinkDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") setLinkDraft(null);
              if (event.key === "Enter" && linkDraft.trim()) {
                onChange([...assets, addLink(linkDraft)]);
                setLinkDraft(null);
              }
            }}
          />
          <button
            type="button"
            className={styles.ok}
            disabled={!linkDraft.trim()}
            onClick={() => {
              onChange([...assets, addLink(linkDraft)]);
              setLinkDraft(null);
            }}
          >
            加入
          </button>
          <button type="button" className={styles.add} onClick={() => setLinkDraft(null)}>
            取消
          </button>
        </div>
      )}

      <input
        ref={fileRef}
        type="file"
        multiple
        hidden
        onChange={(event) => {
          void add(event.target.files);
          event.target.value = "";
        }}
      />
    </div>
  );
}

/** 单个材料格：图片出缩略图，文件出名字，链接出可点的标题 */
function AssetCell({ asset, onRemove }: { asset: Asset; onRemove: () => void }) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    void resolveUrl(asset).then((value) => alive && setUrl(value));
    return () => {
      alive = false;
    };
  }, [asset]);

  const isImage = asset.kind === "image";

  return (
    <div className={styles.cell} title={asset.note || asset.name}>
      {isImage ? (
        url ? (
          <img className={styles.thumb} src={url} alt={asset.name} />
        ) : (
          <span className={styles.placeholder}>图</span>
        )
      ) : (
        <span className={styles.fileChip}>
          {asset.kind === "link" ? <LinkIcon size={10} /> : <FileIcon size={10} />}
          <a
            href={asset.kind === "link" ? asset.stored : url ?? undefined}
            target="_blank"
            rel="noreferrer"
            className={styles.fileName}
          >
            {asset.name}
          </a>
        </span>
      )}
      <button type="button" className={styles.rm} aria-label={`移除 ${asset.name}`} onClick={onRemove}>
        <Close size={8} />
      </button>
    </div>
  );
}
