// Checks every package in package-lock.json (the whole tree, optional platform binaries included) against
// the license allowlist. Run from node/: `npm run license-check`. Exits 0 only if every package's license is
// allowed. No dependencies: the lockfile's `license` field is the one the registry reports for each package.
//
// "A OR B" passes if either side is allowed. "AND", "WITH" and anything else fails, so an unusual
// expression is reviewed by hand rather than guessed at.

import { readFileSync } from 'node:fs';

const ALLOWED = new Set(['MIT', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Unicode-3.0', 'Zlib']);

const lock = JSON.parse(readFileSync(new URL('./package-lock.json', import.meta.url), 'utf8'));

function allowed(expr) {
  return typeof expr === 'string' && expr.split(' OR ').some((alt) => ALLOWED.has(alt.trim()));
}

let bad = 0;
let count = 0;
for (const [path, pkg] of Object.entries(lock.packages)) {
  if (path === '') continue; // this package itself
  count++;
  const name = path.replace(/^.*node_modules\//, '');
  if (allowed(pkg.license)) {
    console.log(`ok   ${name}@${pkg.version}  ${pkg.license}`);
  } else {
    bad++;
    console.log(`FAIL ${name}@${pkg.version}  ${pkg.license ?? '(no license field)'}`);
  }
}
console.log(bad === 0 ? `${count} packages, all allowed` : `${bad} of ${count} packages not allowed`);
process.exit(bad === 0 ? 0 : 1);
