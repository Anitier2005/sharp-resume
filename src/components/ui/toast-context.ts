import { createContext, useContext } from "react";

export type ToastFn = (text: string) => void;

export const ToastContext = createContext<ToastFn>(() => {});

export function useToast(): ToastFn {
  return useContext(ToastContext);
}
