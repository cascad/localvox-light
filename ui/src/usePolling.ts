import { useCallback, useEffect, useRef, useState } from "react";

/** One in-flight read per mounted view, with cancellation and bounded retry delay. */
export function usePolling<T>(key: string, load: (signal: AbortSignal) => Promise<T>, interval = 3000) {
  const [snapshot, setSnapshot] = useState<{ key: string; data: T | null; error: string | null }>({ key, data: null, error: null });
  const refreshRef = useRef<(() => void) | null>(null);
  const refresh = useCallback(() => { refreshRef.current?.(); }, []);
  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let controller: AbortController | null = null;
    let generation = 0;
    let failures = 0;
    setSnapshot({ key, data: null, error: null });
    const poll = async (current: number, signal: AbortSignal) => {
      try {
        const data = await load(signal);
        if (!alive || current !== generation) return;
        setSnapshot({ key, data, error: null }); failures = 0;
      } catch (error) {
        if (!alive || current !== generation) return;
        setSnapshot(previous => ({ key, data: previous.key === key ? previous.data : null, error: (error as Error).message }));
        failures++;
      }
      controller = null;
      timer = setTimeout(start, Math.min(30_000, interval * 2 ** Math.min(failures, 4)));
    };
    const start = () => {
      if (!alive) return;
      const current = ++generation;
      clearTimeout(timer);
      controller?.abort();
      controller = new AbortController();
      const signal = controller.signal;
      queueMicrotask(() => { if (alive && current === generation) void poll(current, signal); });
    };
    refreshRef.current = () => { failures = 0; start(); };
    start();
    return () => {
      alive = false;
      generation++;
      refreshRef.current = null;
      clearTimeout(timer);
      controller?.abort();
    };
    // The key names the resource. Changing incidental render closures must not restart a poll.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, interval]);
  return { data: snapshot.key === key ? snapshot.data : null, error: snapshot.key === key ? snapshot.error : null, refresh };
}
