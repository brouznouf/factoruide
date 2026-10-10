import { useCallback, useRef, useState } from "react";

/** Width in pixels of the element given the returned ref, followed as it resizes: charts are
 * drawn at their real size (a fixed height and readable text on a wide screen) instead of
 * scaling a fixed drawing with the width. */
export function useWidth(fallback: number) {
  const [width, setWidth] = useState(fallback);
  const observer = useRef<ResizeObserver | null>(null);
  const ref = useCallback((el: HTMLElement | null) => {
    observer.current?.disconnect();
    observer.current = null;
    if (!el) return;
    const update = (w: number) => setWidth(Math.max(240, Math.round(w)));
    update(el.clientWidth);
    observer.current = new ResizeObserver(([entry]) => update(entry.contentRect.width));
    observer.current.observe(el);
  }, []);
  return [ref, width] as const;
}
