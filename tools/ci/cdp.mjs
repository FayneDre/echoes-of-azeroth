// Minimal Chrome DevTools Protocol client over Node's built-in WebSocket,
// to drive the launcher's WebView2 (debug build whose window sets
// `additionalBrowserArgs` to `--remote-debugging-port=<port>`). No
// dependency: used by the UI smoke test and for local checks.

/** Connects to the first page target on `port` (retries while the app starts). */
export async function connect(port, { timeoutMs = 60_000 } = {}) {
  const deadline = Date.now() + timeoutMs;
  let targets = [];
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`);
      targets = (await response.json()).filter((t) => t.type === "page");
      if (targets.length > 0) break;
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  if (targets.length === 0) throw new Error(`no page on the debugging port ${port}`);
  const socket = new WebSocket(targets[0].webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.onopen = resolve;
    socket.onerror = () => reject(new Error("cannot open the DevTools socket"));
  });
  let next = 1;
  const waiting = new Map();
  const listeners = new Map();
  socket.onmessage = (message) => {
    const data = JSON.parse(message.data);
    if (data.method) {
      for (const listener of listeners.get(data.method) ?? []) listener(data.params);
      return;
    }
    const pending = data.id ? waiting.get(data.id) : undefined;
    if (!pending) return;
    waiting.delete(data.id);
    if (data.error) pending.reject(new Error(data.error.message));
    else pending.resolve(data.result);
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = next++;
      waiting.set(id, { resolve, reject });
      socket.send(JSON.stringify({ id, method, params }));
    });

  const page = {
    send,
    /** Calls `listener(params)` for each `method` event (enable its domain first). */
    on(method, listener) {
      listeners.set(method, [...(listeners.get(method) ?? []), listener]);
    },
    /** Value of a JS expression in the page (promises awaited). */
    async evaluate(expression) {
      const result = await send("Runtime.evaluate", {
        expression,
        awaitPromise: true,
        returnByValue: true,
      });
      if (result.exceptionDetails) {
        throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
      }
      return result.result.value;
    },
    /** Waits until `expression` is truthy; returns its value. */
    async waitFor(expression, { timeoutMs = 30_000, what = expression } = {}) {
      const end = Date.now() + timeoutMs;
      while (Date.now() < end) {
        const value = await page.evaluate(expression).catch(() => undefined);
        if (value) return value;
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
      throw new Error(`timed out waiting for ${what}`);
    },
    /** Clicks the first button or link whose text is `text`. */
    async click(text) {
      const done = await page.evaluate(`(() => {
        const el = [...document.querySelectorAll("button, a")].find((e) => e.textContent.trim() === ${JSON.stringify(text)});
        if (!el) return false;
        el.click();
        return true;
      })()`);
      if (!done) throw new Error(`no button "${text}"`);
    },
    /** PNG bytes of the page as shown. */
    async screenshot() {
      const { data } = await send("Page.captureScreenshot", { format: "png" });
      return Buffer.from(data, "base64");
    },
    close: () => socket.close(),
  };
  return page;
}
