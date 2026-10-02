(() => {
  "use strict";
  if (window.__studioConsoleObserver) return;
  window.__studioConsoleObserver = true;

  function isAppSummaries(url) {
    try {
      const endpoint = new URL(url, location.href);
      return endpoint.origin === "https://playconsoleapps-pa.clients6.google.com" &&
        /^\/v1\/developers\/\d+\/appSummaries$/.test(endpoint.pathname);
    } catch { return false; }
  }

  function publish(url, body) {
    try {
      const endpoint = new URL(url, location.href);
      const match = endpoint.pathname.match(/^\/v1\/developers\/(\d+)\/appSummaries$/);
      if (endpoint.origin !== "https://playconsoleapps-pa.clients6.google.com" || !match) return;
      const rows = typeof body === "string" ? JSON.parse(body) : body;
      if (!Array.isArray(rows?.["1"])) return;
      const apps = rows["1"].filter(app => typeof app["5"] === "string").map(app => ({
        display_name: typeof app["2"] === "string" ? app["2"] : "",
        package_name: app["5"],
        icon_url: typeof app["3"] === "string" ? app["3"] : null,
      }));
      window.postMessage({ type: "studio-console-apps", snapshot: { developer_id: match[1], apps } }, location.origin);
    } catch { /* Leave Console's requests and responses untouched. */ }
  }

  const originalFetch = window.fetch;
  window.fetch = function (...args) {
    const result = originalFetch.apply(this, args);
    const url = typeof args[0] === "string" || args[0] instanceof URL ? String(args[0]) : args[0]?.url;
    if (isAppSummaries(url)) result.then(response => {
      if (response.ok) response.clone().text().then(body => publish(url, body)).catch(() => {});
    }).catch(() => {});
    return result;
  };

  const originalOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url, ...args) {
    if (isAppSummaries(url)) this.addEventListener("load", () => {
      if (this.status >= 200 && this.status < 300) {
        try { publish(url, this.responseType === "json" ? this.response : this.responseText); } catch {}
      }
    }, { once: true });
    return originalOpen.call(this, method, url, ...args);
  };
})();
