import { test } from "node:test";
import assert from "node:assert/strict";
import { spectatorParams, requestedStream, STREAM_MESSAGE } from "./spectate.mjs";

const query = (values) => "?" + new URLSearchParams(values).toString();

test("spectator page reads its asset directory, stream and parent origin", () => {
  const params = spectatorParams(query({
    assets: "https://assets.example/bundle/assets/",
    stream: "https://example.com/api/spectator/duels/7/stream?camera=pov",
    parent: "https://example.com/watch/7",
  }));
  assert.equal(params.assets.href, "https://assets.example/bundle/assets/");
  assert.equal(params.stream, "https://example.com/api/spectator/duels/7/stream?camera=pov");
  assert.equal(params.parent, "https://example.com");
});

test("spectator page allows plain HTTP only on loopback hosts", () => {
  const params = spectatorParams(query({ assets: "http://localhost:8000/assets/", stream: "http://127.0.0.1:9000/stream" }));
  assert.equal(params.parent, null);
  for (const values of [
    { assets: "http://example.com/assets/", stream: "https://example.com/stream" },
    { assets: "https://example.com/assets/", stream: "http://example.com/stream" },
    { assets: "https://example.com/assets/", stream: "https://example.com/stream", parent: "http://example.com" },
  ]) {
    assert.throws(() => spectatorParams(query(values)));
  }
});

test("spectator page rejects missing values and asset paths without a trailing slash", () => {
  assert.throws(() => spectatorParams(query({ stream: "https://example.com/stream" })));
  assert.throws(() => spectatorParams(query({ assets: "https://example.com/assets/" })));
  assert.throws(() => spectatorParams(query({ assets: "https://example.com/assets", stream: "https://example.com/stream" })));
  assert.throws(() => spectatorParams(query({ assets: "https://example.com/assets/", stream: "not a url" })));
});

test("stream switches are accepted only from the configured parent", () => {
  const parent = "https://example.com";
  const message = { type: STREAM_MESSAGE, stream: "https://example.com/stream?camera=orbit" };
  assert.equal(requestedStream({ origin: parent, data: message }, parent), message.stream);
  assert.equal(requestedStream({ origin: "https://evil.example", data: message }, parent), null);
  assert.equal(requestedStream({ origin: parent, data: message }, null), null);
  assert.equal(requestedStream({ origin: parent, data: { type: "other", stream: message.stream } }, parent), null);
  assert.equal(requestedStream({ origin: parent, data: { type: STREAM_MESSAGE, stream: 7 } }, parent), null);
});
