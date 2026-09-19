import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { listen } from '@tauri-apps/api/event';
import { useTauriEvent } from './useTauriEvent';

type Listener = (event: { payload: unknown }) => void;

describe('useTauriEvent', () => {
  beforeEach(() => {
    vi.mocked(listen).mockReset();
  });

  it('subscribes once, forwards payloads to the latest handler, unlistens on unmount', async () => {
    const unlisten = vi.fn();
    let captured: Listener | undefined;
    vi.mocked(listen).mockImplementation((_event, handler) => {
      captured = handler as Listener;
      return Promise.resolve(unlisten);
    });

    const first = vi.fn();
    const second = vi.fn();
    const { result, rerender, unmount } = renderHook(
      ({ handler }: { handler: (payload: string) => void }) => {
        return useTauriEvent<string>('terrace://test', handler);
      },
      { initialProps: { handler: first } }
    );

    await waitFor(() => {
      expect(result.current).toBe(true);
    });
    expect(listen).toHaveBeenCalledTimes(1);

    rerender({ handler: second });
    captured?.({ payload: 'hello' });
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith('hello');

    unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('unlistens even when the component unmounted before listen resolved', async () => {
    const unlisten = vi.fn();
    let resolve: ((fn: () => void) => void) | undefined;
    let captured: Listener | undefined;
    const handler = vi.fn();
    vi.mocked(listen).mockImplementation((_event, callback) => {
      captured = callback as Listener;
      return new Promise((done) => {
        resolve = done;
      });
    });

    const { unmount } = renderHook(() => {
      useTauriEvent('terrace://late', handler);
    });
    unmount();
    captured?.({ payload: 'late event' });
    expect(handler).not.toHaveBeenCalled();
    await act(async () => {
      resolve?.(unlisten);
      await Promise.resolve();
    });
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
