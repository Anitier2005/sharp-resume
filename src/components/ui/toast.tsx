import { useCallback, useRef, useState, type ReactNode } from "react";
import { ToastContext } from "./toast-context";

interface ToastItem {
  id: number;
  text: string;
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const seq = useRef(0);

  const push = useCallback((text: string) => {
    const id = ++seq.current;
    setItems((list) => [...list, { id, text }]);
    window.setTimeout(() => {
      setItems((list) => list.filter((item) => item.id !== id));
    }, 2200);
  }, []);

  return (
    <ToastContext.Provider value={push}>
      {children}
      <div className="toasts" aria-live="polite">
        {items.map((item) => (
          <div key={item.id} className="toast">
            {item.text}
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}
