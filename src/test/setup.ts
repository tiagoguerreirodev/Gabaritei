import "@testing-library/jest-dom/vitest";
import { cleanup } from "@solidjs/testing-library";
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach } from "vitest";

// jsdom doesn't implement scrolling; the router calls it on every navigation.
window.scrollTo = () => {};

afterEach(() => {
  cleanup();
  clearMocks();
  window.location.hash = "";
});
