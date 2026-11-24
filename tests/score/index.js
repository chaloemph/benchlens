// `node --test tests/score/`: Node 24 resolves a directory argument as a module (it no longer scans it for
// test files), so this entry loads every *.test.mjs next to it. Running the files by glob
// (node --test 'tests/score/*.test.mjs') works too. package.json marks this file as an ES module.
import fs from 'node:fs';

for (const f of fs.readdirSync(import.meta.dirname).filter((n) => n.endsWith('.test.mjs')).sort()) await import('./' + f);
