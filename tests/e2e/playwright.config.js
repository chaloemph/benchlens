// Playwright config for the benchlens end-to-end suite. Run it through scripts/e2e.sh, which starts
// `benchlens serve` on a temporary database and sets BENCHLENS_URL.
import { defineConfig, devices } from '@playwright/test';

const baseURL = process.env.BENCHLENS_URL;
if (!baseURL) throw new Error('BENCHLENS_URL is not set: run scripts/e2e.sh (it starts the server)');

export default defineConfig({
  testDir: './tests',
  outputDir: './test-results',
  // one server, one database: the tag test writes, so the files run one after the other
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  reporter: process.env.CI ? [['list'], ['github']] : 'list',
  use: {
    ...devices['Desktop Chrome'],
    baseURL: baseURL.replace(/\/?$/, '/'),
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
  },
});
