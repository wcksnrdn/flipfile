import { useCallback, useEffect, useRef, useState } from "react";

/** A value that shows for `ms` and then clears itself; showing it again restarts the clock */
export function useTransient<T>(ms: number) {
  const [value, setValue] = useState<T | null>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const show = useCallback(
    (next: T) => {
      window.clearTimeout(timer.current);
      setValue(next);
      timer.current = window.setTimeout(() => setValue(null), ms);
    },
    [ms]
  );

  return [value, show] as const;
}
