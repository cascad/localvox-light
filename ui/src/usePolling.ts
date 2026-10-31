import { useEffect, useState } from "react";

/** One in-flight read per mounted view, with cancellation and bounded retry delay. */
export function usePolling<T>(key: string, load: (signal: AbortSignal) => Promise<T>, interval = 3000) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const controller = new AbortController();
    let failures = 0;
    setData(null); setError(null);
    const poll = async () => {
      try {
        const value = await load(controller.signal);
        if (!alive) return;
        setData(value); setError(null); failures = 0;
      } catch (error) {
        if (!alive) return;
        setError((error as Error).message); failures++;
      }
      if (alive) timer = setTimeout(() => void poll(), Math.min(30_000, interval * 2 ** Math.min(failures, 4)));
    };
    queueMicrotask(() => { if (alive) void poll(); });
    return () => { alive = false; clearTimeout(timer); controller.abort(); };
    // The key names the resource. Changing incidental render closures must not restart a poll.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, interval]);
  return { data, error };
}
