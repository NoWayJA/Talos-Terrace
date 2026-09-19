import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useEffect, useRef, useState } from 'react';
import { describeError } from '@/services/errors';

/**
 * Subscribe to a Tauri event for the lifetime of the component.
 *
 * The mounted guard is lifted from Modz (NoWayJA/Modz, MIT) App.tsx: `listen` resolves
 * asynchronously, so a component that unmounts before it resolves must still unlisten,
 * otherwise the handler leaks. The handler is read through a ref so callers can pass
 * a fresh closure on every render without re-subscribing.
 */
// eslint-disable-next-line @typescript-eslint/no-unnecessary-type-parameters -- T names the payload type the caller expects
export function useTauriEvent<T>(event: string, handler: (payload: T) => void): boolean {
  const handlerRef = useRef(handler);
  const [readyEvent, setReadyEvent] = useState<string | null>(null);

  useEffect(() => {
    handlerRef.current = handler;
  });

  useEffect(() => {
    let mounted = true;
    let unlisten: UnlistenFn | undefined;

    listen<T>(event, (received) => {
      if (mounted) handlerRef.current(received.payload);
    })
      .then((fn) => {
        if (mounted) {
          unlisten = fn;
          setReadyEvent(event);
        } else {
          fn();
        }
      })
      .catch((error: unknown) => {
        console.error(`listen(${event}) failed: ${describeError(error)}`);
      });

    return () => {
      mounted = false;
      unlisten?.();
    };
  }, [event]);
  return readyEvent === event;
}
