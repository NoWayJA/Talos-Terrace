/** Shape the Rust side serialises for every failed command (see src-tauri/src/error.rs). */
interface SerialisedError {
  code: string;
  message: string;
}

/** Every failure that crosses the IPC boundary becomes one of these. */
export class ConsoleError extends Error {
  readonly code: string;
  readonly command: string;

  constructor(code: string, message: string, command: string) {
    super(message);
    this.name = 'ConsoleError';
    this.code = code;
    this.command = command;
  }
}

function isSerialisedError(value: unknown): value is SerialisedError {
  if (typeof value !== 'object' || value === null) return false;
  const record = value as Record<string, unknown>;
  return typeof record.code === 'string' && typeof record.message === 'string';
}

export function toConsoleError(raw: unknown, command: string): ConsoleError {
  if (raw instanceof ConsoleError) return raw;
  if (isSerialisedError(raw)) return new ConsoleError(raw.code, raw.message, command);
  if (raw instanceof Error) return new ConsoleError('IPC', raw.message, command);
  if (typeof raw === 'string') return new ConsoleError('IPC', raw, command);
  return new ConsoleError('UNKNOWN', 'Unknown error', command);
}

/** The message a person should read: what went wrong, without the command's internal name. */
export function describeError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (isSerialisedError(error)) return error.message;
  if (typeof error === 'string') return error;
  return 'Unknown error';
}

/** The line for a log: the command that failed, then the message. */
export function describeErrorForLog(error: unknown): string {
  if (error instanceof ConsoleError) return `${error.command}: ${error.message}`;
  return describeError(error);
}
