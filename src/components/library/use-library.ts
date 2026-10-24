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

/** 勾选 / 取消「本页」引用某段经历或作品 */
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
