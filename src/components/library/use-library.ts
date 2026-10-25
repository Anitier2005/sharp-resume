import { useCallback } from "react";
import type { Person } from "@/types";
import { useWorkspace } from "@/store/workspace";
import type { PageRefField } from "./helpers";

/** 更新候选人自身字段（姓名、经历列表等） */
export function usePatchPerson(person: Person) {
  const { dispatch } = useWorkspace();
  return useCallback(
    (next: Partial<Person>) => {
      dispatch({ type: "patchPerson", personId: person.id, patch: next });
    },
    [dispatch, person.id],
  );
}

/** 勾选 / 取消「本页」引用某件作品（经历已改由 entries 承载，见 useEntryToggle） */
export function usePageToggle() {
  const { person, resume, page, dispatch } = useWorkspace();
  return useCallback(
    (field: PageRefField, itemId: string) => {
      if (!person || !resume || !page) return;
      dispatch({
        type: "togglePageItem",
        personId: person.id,
        resumeId: resume.id,
        pageId: page.id,
        field,
        itemId,
      });
    },
    [dispatch, person, resume, page],
  );
}

/** 把一段共享经历挂到 / 摘出当前页 —— 挂上时同时开一条空措辞 */
export function useEntryToggle() {
  const { person, resume, page, dispatch } = useWorkspace();
  return useCallback(
    (experienceId: string) => {
      if (!person || !resume || !page) return;
      const entry = page.entries.find((item) => item.experienceId === experienceId);
      if (entry) {
        dispatch({
          type: "removeEntry",
          personId: person.id,
          resumeId: resume.id,
          pageId: page.id,
          entryId: entry.id,
        });
        return;
      }
      dispatch({
        type: "addEntry",
        personId: person.id,
        resumeId: resume.id,
        pageId: page.id,
        experienceId,
      });
    },
    [dispatch, person, resume, page],
  );
}
