const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const path = require("node:path");

function load(name, globals) {
  const context = vm.createContext({ URL, ...globals });
  vm.runInContext(fs.readFileSync(path.join(__dirname, name), "utf8"), context);
  return context;
}

test("bridge transfers app metadata and excludes credentials", () => {
  const context = load("background.js", { chrome: { runtime: { onMessage: { addListener() {} } } } });
  const result = context.cleanSnapshot({
    developer_id: "123",
    cookie: "must-not-transfer",
    apps: [
      { package_name: "test.app", display_name: "Test", icon_url: "https://lh3.googleusercontent.com/icon", authorization: "must-not-transfer" },
      { package_name: "other.app", icon_url: "https://googleusercontent.com.attacker.test/icon" },
      { package_name: "test.app", icon_url: "https://lh3.googleusercontent.com/duplicate" },
    ],
  });
  const transferred = JSON.parse(JSON.stringify(result));
  assert.deepEqual(transferred, { developer_id: "123", apps: [
    { package_name: "test.app", display_name: "Test", icon_url: "https://lh3.googleusercontent.com/icon" },
    { package_name: "other.app", display_name: "", icon_url: null },
  ] });
  assert.equal(context.cleanSnapshot({ developer_id: "wrong", apps: [] }), null);
});

test("observer captures the Console API response without changing requests", () => {
  const messages = [];
  class XHR {
    addEventListener(name, callback) { this.callback = callback; }
    open(...args) { this.args = args; return "original-result"; }
  }
  load("observer.js", {
    location: { href: "https://play.google.com/console/app-list", origin: "https://play.google.com" },
    window: { fetch: () => Promise.resolve({}), postMessage: value => messages.push(value) },
    XMLHttpRequest: XHR,
  });
  const xhr = new XHR();
  const url = "https://playconsoleapps-pa.clients6.google.com/v1/developers/123/appSummaries?$httpHeaders=secret";
  assert.equal(xhr.open("GET", url), "original-result");
  assert.deepEqual(xhr.args, ["GET", url]);
  xhr.status = 200;
  xhr.responseText = JSON.stringify({ "1": [{ "2": "Test", "3": "https://lh3.googleusercontent.com/icon", "5": "test.app", "secret": "must-not-transfer" }] });
  xhr.callback();
  assert.equal(messages.length, 1);
  assert.deepEqual(JSON.parse(JSON.stringify(messages[0].snapshot)), {
    developer_id: "123", apps: [{ display_name: "Test", package_name: "test.app", icon_url: "https://lh3.googleusercontent.com/icon" }],
  });
  const unrelated = new XHR();
  unrelated.open("GET", "https://attacker.test/v1/developers/123/appSummaries");
  assert.equal(unrelated.callback, undefined);
});
