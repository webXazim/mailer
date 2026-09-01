import assert from 'node:assert/strict';

const base = 'http://127.0.0.1:8081';
for (const path of ['/healthz', '/', '/login', '/domains']) {
  const response = await fetch(base + path);
  assert.equal(response.status, 200, path);
}
for (const path of [
  '/internal', '/internal/v1/ses/events', '/api/internal',
  '/api/internal/v1/ses/events', '/api//internal/v1/ses/events',
  '/api/%69nternal/v1/ses/events', '/api/internal%2fv1/ses/events',
]) {
  for (const method of ['GET', 'POST']) {
    assert.equal((await fetch(base + path, { method })).status, 404, `${method} ${path}`);
  }
}
const response = await fetch(base + '/api/v1/emails?limit=1', {
  headers: {
    'CF-Connecting-IP': '203.0.113.42',
    'X-Real-IP': '198.51.100.99',
    'X-Forwarded-For': '198.51.100.99',
    Forwarded: 'for=198.51.100.99',
    Cookie: '__Host-cs_session=test-session',
    Authorization: 'Bearer test-api-key',
  },
});
const data = await response.json();
assert.equal(data.path, '/v1/emails?limit=1');
assert.equal(data.peer, '127.0.0.1');
assert.equal(data.headers['x-real-ip'], '203.0.113.42');
assert.equal(data.headers['x-forwarded-for'], undefined);
assert.equal(data.headers.forwarded, undefined);
assert.equal(data.headers['x-forwarded-proto'], 'https');
assert.equal(data.headers.cookie, '__Host-cs_session=test-session');
assert.equal(data.headers.authorization, 'Bearer test-api-key');
assert.equal(response.headers.get('strict-transport-security'), 'max-age=31536000');
for (const path of ['/api/healthz', '/api/readyz']) {
  assert.equal((await (await fetch(base + path)).json()).path, path.slice(4));
}
console.log('Proxy routing, private-route blocking, loopback trust and header sanitization passed.');
