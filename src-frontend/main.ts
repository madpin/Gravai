import App from "./App.svelte";
import CaptionOverlay from "./CaptionOverlay.svelte";
import { mount } from "svelte";

if (import.meta.env.DEV) {
  document.title = "Gravai Dev";
}

const isCaptionWindow = new URLSearchParams(location.search).get("captions") === "1";
if (isCaptionWindow) {
  document.documentElement.classList.add("live-captions");
  document.body.classList.add("live-captions");
}
const app = mount(isCaptionWindow ? CaptionOverlay : App, { target: document.getElementById("app")! });

export default app;
