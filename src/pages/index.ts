/** The one Page union. Every navigation target is listed here and nowhere else. */
export const PAGES = ['canvas', 'settings'] as const;
export type Page = (typeof PAGES)[number];
