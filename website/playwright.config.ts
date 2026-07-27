import { defineConfig, devices } from "@playwright/test";

const port = Number(process.env.PORT ?? 4321);
const baseURL = `http://127.0.0.1:${port}/caelix/`;

export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  use: {
    baseURL,
    trace: "on-first-retry",
  },
  webServer: {
    command: "bun run preview",
    url: baseURL,
    reuseExistingServer: !process.env.CI,
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    {
      name: "mobile",
      use: { ...devices["iPhone 13"], browserName: "chromium" },
    },
  ],
});
