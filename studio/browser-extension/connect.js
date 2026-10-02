(async () => {
  "use strict";
  const api = globalThis.browser || globalThis.chrome;
  const token = new URLSearchParams(location.hash.slice(1)).get("token");
  history.replaceState(null, "", location.pathname);
  const status = document.getElementById("studio-console-status");
  if (!token) {
    if (status) status.textContent = "Start a new sync from Studio.";
    return;
  }
  try {
    const result = await api.runtime.sendMessage({ type: "connect", token });
    if (status) status.textContent = result?.ok
      ? "Connected. Open your app list in Play Console to sync icons."
      : "Could not connect. Start a new sync from Studio.";
  } catch {
    if (status) status.textContent = "Could not connect. Start a new sync from Studio.";
  }
})();
