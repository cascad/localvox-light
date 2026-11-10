import { useEffect, useRef, useState } from "react";

interface Snapshot<T> {
  key: string;
  data: T | null;
  err: string | null;
}

/** The key identifies the session/tab/version; refresh requests never overlap. */
export function useArtifact<T>(key: string, load: (signal: AbortSignal) => Promise<T>, refresh = 0, accepts?: (next: T, previous: T) => boolean) {
  const [snapshot, setSnapshot] = useState<Snapshot<T> | null>(null);
  const request = useRef<(() => void) | null>(null);

  useEffect(() => {
    let alive = true;
    let running = false;
    let pending = false;
    const controller = new AbortController();
    const read = async () => {
      if (!alive) return;
      if (running) { pending = true; return; }
      running = true;
      try {
        // A discarded StrictMode lifecycle must not start another HTTP read.
        await Promise.resolve();
        if (!alive) return;
        const data = await load(controller.signal);
        if (alive) setSnapshot(previous => previous?.key === key && previous.data != null && accepts && !accepts(data, previous.data)
          ? previous : { key, data, err: null });
      } catch (error) {
        if (alive) setSnapshot(current => ({
          key,
          data: current?.key === key ? current.data : null,
          err: error instanceof Error ? error.message : String(error),
        }));
      } finally {
        running = false;
        if (alive && pending) { pending = false; void read(); }
      }
    };
    request.current = () => { void read(); };
    return () => {
      alive = false;
      request.current = null;
      controller.abort();
    };
    // The loader is bound to this resource identity, not incidental render closures.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  useEffect(() => { request.current?.(); }, [key, refresh]);

  // A key change must hide the old document before effects start its replacement.
  return snapshot?.key === key ? { data: snapshot.data, err: snapshot.err } : { data: null, err: null };
}
