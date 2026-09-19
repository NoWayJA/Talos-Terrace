#!/usr/bin/env node
// Verifies that every documentation link in src/pages/Canvas/docs.ts resolves: an HTTP 200
// after redirects and a page title that is not "Page not found" (some sites answer 200 for
// missing pages). Needs the network, so it is `npm run links:check`, not part of `npm run
// check`; CI runs it on a schedule. Documentation sites move: Talos's did in 2025.
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/pages/Canvas/docs.ts', import.meta.url), 'utf8');
const constants = Object.fromEntries(
  [...source.matchAll(/const (\w+) = '([^']+)'/g)].map((match) => [match[1], match[2]])
);
const urls = [...source.matchAll(/url: (`[^`]+`|'[^']+')/g)].map((match) =>
  match[1].slice(1, -1).replace(/\$\{(\w+)\}/g, (_, name) => constants[name] ?? '')
);

let failures = 0;
for (const url of urls) {
  try {
    const response = await fetch(url, {
      redirect: 'follow',
      signal: AbortSignal.timeout(20_000),
      headers: { 'user-agent': 'terrace-link-check' },
    });
    const html = await response.text();
    const title = /<title>(.*?)<\/title>/is.exec(html)?.[1]?.trim() ?? '';
    const broken = !response.ok || /not found/i.test(title);
    if (broken) failures += 1;
    const moved = response.url !== url ? ` -> ${response.url}` : '';
    console.log(
      `${broken ? 'BAD' : 'ok '} ${String(response.status)} ${url}${moved} | ${title.slice(0, 60)}`
    );
  } catch (error) {
    failures += 1;
    console.log(`BAD --- ${url} | ${error instanceof Error ? error.message : String(error)}`);
  }
}
console.log(`${String(urls.length - failures)} of ${String(urls.length)} links resolve`);
process.exit(failures === 0 ? 0 : 1);
