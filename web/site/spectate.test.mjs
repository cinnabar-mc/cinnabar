import { test } from "node:test";
import assert from "node:assert/strict";
import { spectatorParams, requestedStream, noticeOrigin, STREAM_MESSAGE } from "./spectate.mjs";

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

test("stream switches are accepted only from the configured parent window", () => {
  const parent = "https://example.com", frame = {}, sibling = {};
  const message = { type: STREAM_MESSAGE, stream: "https://example.com/stream?camera=orbit" };
  const from = (origin, source, data = message) => ({ origin, source, data });
  assert.equal(requestedStream(from(parent, frame), parent, frame), message.stream);
  assert.equal(requestedStream(from("https://evil.example", frame), parent, frame), null);
  assert.equal(requestedStream(from(parent, sibling), parent, frame), null);
  assert.equal(requestedStream(from(parent, frame), parent, null), null);
  assert.equal(requestedStream(from(parent, frame), null, frame), null);
  assert.equal(requestedStream(from(parent, frame, { type: "other", stream: message.stream }), parent, frame), null);
  assert.equal(requestedStream(from(parent, frame, { type: STREAM_MESSAGE, stream: 7 }), parent, frame), null);
});

test("notices reach a valid parent even when the other values are invalid", () => {
  assert.equal(noticeOrigin(query({ parent: "https://example.com/watch", assets: "bad" })), "https://example.com");
  assert.equal(noticeOrigin(query({ parent: "http://example.com" })), null);
  assert.equal(noticeOrigin(query({ parent: "not a url" })), null);
  assert.equal(noticeOrigin(""), null);
});
