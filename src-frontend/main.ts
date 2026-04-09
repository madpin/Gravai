import App from "./App.svelte";
import { mount } from "svelte";

if (import.meta.env.DEV) {
  document.title = "Gravai Dev";
}

const app = mount(App, { target: document.getElementById("app")! });

export default app;
