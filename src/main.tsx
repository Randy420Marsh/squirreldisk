import ReactDOM from "react-dom/client";
import App from "./App";
import "./style.css";
import { type } from "@tauri-apps/api/os";

// Default to "Linux" so the non-Windows branch (base-1000 byte units) is used
// while the async platform lookup resolves. The real value is filled in below
// before the first render.
window.OS_TYPE = "Linux";

const render = () => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    // <React.StrictMode> commenting since react-beautiful-dnd crashes with
    // this issue: https://github.com/atlassian/react-beautiful-dnd/issues/2396
    <App />
    // </React.StrictMode>
  );
};

type()
  .then((t) => {
    window.OS_TYPE = t;
  })
  .finally(render);
