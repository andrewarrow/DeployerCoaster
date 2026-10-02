(() => {
  "use strict";
  const api = globalThis.browser || globalThis.chrome;
  window.addEventListener("message", event => {
    if (event.source !== window || event.origin !== "https://play.google.com" || event.data?.type !== "studio-console-apps") return;
    api.runtime.sendMessage({ type: "apps", snapshot: event.data.snapshot }).catch(() => {});
  });
})();
