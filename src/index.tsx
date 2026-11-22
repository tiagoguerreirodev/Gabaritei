/* @refresh reload */
import { render } from "solid-js/web";
import App from "./App";
import { startAppearance } from "./appearance";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("Elemento #root não encontrado.");

void startAppearance();
render(() => <App />, root);
