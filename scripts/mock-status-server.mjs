#!/usr/bin/env node
import http from "node:http";

const PORT = Number(process.env.PORT || 8787);

const state = {
  value: "no",
  configured: true,
  updatedAt: Date.now(),
  autoResetHours: 20,
  noSubtitles: ["Mock server"],
  resetAt: null,
};

function json(res, code, payload) {
  res.writeHead(code, {
    "content-type": "application/json",
    "cache-control": "no-store",
  });
  res.end(JSON.stringify(payload));
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url || "/", `http://${req.headers.host}`);

  if (req.method === "GET" && url.pathname === "/api/status") {
    return json(res, 200, {
      autoResetHours: state.autoResetHours,
      configured: state.configured,
      noSubtitles: state.noSubtitles,
      resetAt: state.resetAt,
      state: state.value,
      updatedAt: state.updatedAt,
    });
  }

  if (req.method === "POST" && url.pathname === "/admin/set") {
    let body = "";
    req.on("data", (chunk) => {
      body += chunk;
    });
    req.on("end", () => {
      try {
        const payload = body ? JSON.parse(body) : {};
        if (typeof payload.state === "string") {
          state.value = payload.state;
          state.updatedAt = Date.now();
        }
        if (typeof payload.configured === "boolean") {
          state.configured = payload.configured;
        }
        json(res, 200, { ok: true, state });
      } catch (error) {
        json(res, 400, { ok: false, error: String(error) });
      }
    });
    return;
  }

  if (req.method === "POST" && url.pathname === "/admin/fail") {
    return json(res, 503, { error: "forced failure" });
  }

  if (req.method === "GET" && url.pathname === "/") {
    return json(res, 200, {
      ok: true,
      endpoints: {
        status: "GET /api/status",
        set: "POST /admin/set { state: 'no'|'yes'|'anything-not-no', configured?: boolean }",
        fail: "POST /admin/fail",
      },
      current: state,
    });
  }

  return json(res, 404, { error: "not found" });
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`Mock status server listening on http://127.0.0.1:${PORT}`);
  console.log("Use POST /admin/set to flip state.");
});
