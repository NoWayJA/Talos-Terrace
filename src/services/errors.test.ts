import { describe, expect, it } from 'vitest';
import { ConsoleError, describeError, describeErrorForLog, toConsoleError } from './errors';

describe('toConsoleError', () => {
  it('keeps the code and message the Rust side serialised', () => {
    const error = toConsoleError(
      { code: 'NOT_CONNECTED', message: 'no kubeconfig' },
      'cluster_summary'
    );
    expect(error).toBeInstanceOf(ConsoleError);
    expect(error.code).toBe('NOT_CONNECTED');
    expect(error.message).toBe('no kubeconfig');
    expect(error.command).toBe('cluster_summary');
  });

  it('wraps plain strings and Errors as IPC failures', () => {
    expect(toConsoleError('denied', 'x').code).toBe('IPC');
    expect(toConsoleError(new Error('boom'), 'x').message).toBe('boom');
  });

  it('never throws on unexpected shapes', () => {
    expect(toConsoleError(undefined, 'x').code).toBe('UNKNOWN');
    expect(toConsoleError({ nope: true }, 'x').code).toBe('UNKNOWN');
  });

  it('returns an existing ConsoleError untouched', () => {
    const original = new ConsoleError('A', 'b', 'c');
    expect(toConsoleError(original, 'other')).toBe(original);
  });
});

describe('describeError', () => {
  it('gives people the message and logs the command too', () => {
    const error = new ConsoleError('X', 'failed', 'talos_version');
    expect(describeError(error)).toBe('failed');
    expect(describeErrorForLog(error)).toBe('talos_version: failed');
  });

  it('handles strings, Errors, serialised errors and unknowns', () => {
    expect(describeError('plain')).toBe('plain');
    expect(describeError({ code: 'TIMEOUT', message: 'timed out' })).toBe('timed out');
    expect(describeError(new Error('e'))).toBe('e');
    expect(describeError(42)).toBe('Unknown error');
  });
});
