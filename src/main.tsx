import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
// Syntax-highlighting palette for fenced code blocks in the conversation view.
// github-dark matches the app's dark surfaces; rehype-highlight emits the
// hljs class names this stylesheet targets.
import "highlight.js/styles/github-dark.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
