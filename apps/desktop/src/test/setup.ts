import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// `globals` is disabled in vite.config.ts, so Testing Library cannot register
// its own automatic cleanup hook; register it explicitly.
afterEach(() => {
  cleanup();
});
