// HTTP stub for verifying Nginx routing and header sanitization in Docker.
import http from 'node:http';
http.createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json');
  res.end(JSON.stringify({ path: req.url, headers: req.headers, peer: req.socket.remoteAddress }));
}).listen(8080, '0.0.0.0');
