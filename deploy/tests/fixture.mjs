// What the component's and service's port tests fetch: their own request, said
// back, on the loopback address a connection may reach over plain HTTP.
import { createServer } from "node:http";

createServer((request, response) => {
  let body = "";
  request.on("data", (chunk) => (body += chunk));
  request.on("end", () => {
    response.writeHead(200, { "x-fixture": "yes" });
    response.end(`${request.method} ${request.url} ${body}`);
  });
}).listen(8099, "127.0.0.1");
