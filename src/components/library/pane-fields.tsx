import { useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { useToast } from "@/components/ui/toast-context";
import { Close, Plus } from "@/components/ui/icons";
import { FieldsEditor } from "@/components/ui/fields";
import { AssetsEditor } from "@/components/ui/assets";
import { displayName, type LibraryItem, type Person } from "@/types";
import { uid } from "@/lib/uid";
import { PaneHead } from "./parts";
import styles from "./library.module.css";

/**
 * 库面板：条目 = 一组字段 + 一段详实记录。
 * 没有条目类型（经历/作品/教育/证书），用户想加什么就是什么。
 */
export function FieldsPane({ person }: { person: Person }) {
  const { dispatch } = useWorkspace();
  const toast = useToast();
  const [openId, setOpenId] = useState<string | null>(null);

  const write = (items: LibraryItem[]) =>
    dispatch({ type: "libraryItems", personId: person.id, items });

  const add = () => {
    const item: LibraryItem = { id: uid("it"), fields: [], notes: "" };
    write([...person.items, item]);
    setOpenId(item.id);
  };

  return (
    <div className={styles.pane}>
      <PaneHead
        title="库"
        desc={`${displayName(person.fields)} 名下共享：一条 = 一组字段 + 一段记录`}
        action={
          <button type="button" className={styles.add} onClick={add}>
            <Plus size={10} />
            新增一条
          </button>
        }
      />

      {person.items.length === 0 && (
        <div className={styles.empty}>还没有条目。点右上「新增一条」，然后自己起字段。</div>
      )}

      {person.items.map((item, index) => {
        const open = openId === item.id;
        return (
          <div className={styles.item} key={item.id}>
            <div className={styles.itemHead}>
              <span className={styles.idx}>{String(index + 1).padStart(2, "0")}</span>
              <button
                type="button"
                className={styles.title}
                onClick={() => setOpenId(open ? null : item.id)}
              >
                {displayName(item.fields)}
              </button>
              <span className={styles.end}>
                <span className={styles.acts}>
                  <button
                    type="button"
                    aria-label="删除"
                    className={styles.rm}
                    onClick={() => {
                      write(person.items.filter((row) => row.id !== item.id));
                      toast("已删除该条目");
                    }}
                  >
                    <Close size={10} />
                  </button>
                </span>
              </span>
            </div>

            {open && (
              <div className={styles.itemBody}>
                <FieldsEditor
                  fields={item.fields}
                  onChange={(fields) =>
                    write(person.items.map((row) => (row.id === item.id ? { ...row, fields } : row)))
                  }
                />
                <div className={styles.block}>
                  <span className={styles.blockK}>材料 · 证书扫描件、作品截图、外链</span>
                  <AssetsEditor
                    assets={item.assets ?? []}
                    onChange={(assets) =>
                      write(
                        person.items.map((row) => (row.id === item.id ? { ...row, assets } : row)),
                      )
                    }
                  />
                </div>

                <label className={styles.noteField}>
                  <span className={styles.noteK}>详实记录 · 只存在这里，不会直接出现在简历上</span>
                  <textarea
                    rows={5}
                    value={item.notes}
                    spellCheck={false}
                    placeholder="想到什么写什么：背景、你负责哪块、做过什么尝试、结果、和别人怎么配合、踩过什么坑。"
                    onChange={(event) =>
                      write(
                        person.items.map((row) =>
                          row.id === item.id ? { ...row, notes: event.target.value } : row,
                        ),
                      )
                    }
                  />
                </label>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
