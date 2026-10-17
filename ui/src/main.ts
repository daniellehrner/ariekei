// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import { mount } from "svelte";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "@fontsource/jetbrains-mono/600.css";
import App from "./App.svelte";
import "./tokens.css";
import "./app.css";

export default mount(App, { target: document.getElementById("app")! });
