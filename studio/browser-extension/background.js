"use strict";
const api = globalThis.browser || globalThis.chrome;

function cleanSnapshot(value) {
  if (!/^\d{1,32}$/.test(value?.developer_id) || !Array.isArray(value.apps) || value.apps.length > 10000) return null;
  const apps = [];
  const seen = new Set();
  for (const app of value.apps) {
    if (typeof app.package_name !== "string" || !app.package_name.trim() || app.package_name.length > 512) return null;
    if (seen.has(app.package_name)) continue;
    seen.add(app.package_name);
    let icon_url = null;
    try {
      const url = new URL(app.icon_url);
      if (url.protocol === "https:" && !url.username && !url.password && !url.port &&
          (url.hostname === "googleusercontent.com" || url.hostname.endsWith(".googleusercontent.com"))) icon_url = url.href;
    } catch {}
    apps.push({ package_name: app.package_name, display_name: typeof app.display_name === "string" ? app.display_name.slice(0, 4096) : "", icon_url });
  }
  return { developer_id: value.developer_id, apps };
}

async function handle(message, sender) {
  let source;
  try { source = new URL(sender.url); } catch { return { ok: false }; }
  if (message.type === "connect") {
    if (source.protocol !== "http:" || source.hostname !== "127.0.0.1" || !source.port ||
        source.pathname !== "/studio-console-sync" || !/^[A-Za-z0-9_-]{32,128}$/.test(message.token)) return { ok: false };
    await api.storage.session.set({ studioPair: { origin: source.origin, token: message.token, expires: Date.now() + 300000 } });
    const tabs = await api.tabs.query({ url: "https://play.google.com/console/*" });
    const existing = tabs.find(tab => new URL(tab.url).pathname.endsWith("/app-list"));
    if (existing) {
      await api.tabs.update(existing.id, { active: true });
      await api.tabs.reload(existing.id);
    } else {
      await api.tabs.create({ url: "https://play.google.com/console/" });
    }
    return { ok: true };
  }
  if (message.type === "apps" && source.origin === "https://play.google.com" && source.pathname.startsWith("/console/")) {
    const { studioPair } = await api.storage.session.get("studioPair");
    if (!studioPair || studioPair.expires < Date.now()) return { ok: false };
    const snapshot = cleanSnapshot(message.snapshot);
    if (!snapshot) return { ok: false };
    try {
      const response = await fetch(studioPair.origin + "/icons", {
        method: "POST", headers: { "Content-Type": "application/json", Authorization: "Bearer " + studioPair.token },
        body: JSON.stringify(snapshot), credentials: "omit", redirect: "error",
      });
      if (!response.ok) return { ok: false };
      await api.storage.session.remove("studioPair");
      return { ok: true };
    } catch { return { ok: false }; }
  }
  return { ok: false };
}

api.runtime.onMessage.addListener((message, sender, sendResponse) => {
  handle(message, sender).then(sendResponse).catch(() => sendResponse({ ok: false }));
  return true;
});
