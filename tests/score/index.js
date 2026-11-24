// `node --test tests/score/`: Node 24 resolves a directory argument as a module (it no longer scans it for
// test files), so this entry loads them. Running the files by glob (tests/score/*.test.mjs) works too.
import './scores.test.mjs';
import './weights.test.mjs';
import './price.test.mjs';
import './full.test.mjs';
