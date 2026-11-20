import { HashRouter } from "@solidjs/router";
import Layout from "./components/Layout";
import { routeDefinitions } from "./routes";

/**
 * Hash-based routing: works the same under the Vite dev server and Tauri's asset protocol,
 * with no server-side fallback needed.
 */
export default function App() {
  return <HashRouter root={Layout}>{routeDefinitions}</HashRouter>;
}
