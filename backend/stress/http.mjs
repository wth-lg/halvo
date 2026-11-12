import http from "node:http";

export function call(port, method, path, body, headers = {}) {
  return new Promise((resolve, reject) => {
    const sent = body === undefined ? headers : { ...headers, "content-type": "application/json", "content-length": Buffer.byteLength(body) };
    const request = http.request({ host: "127.0.0.1", port, path, method, agent: false, headers: sent }, (response) => {
      const parts = [];
      response.on("data", (part) => parts.push(part));
      response.on("end", () => resolve({ status: response.statusCode, text: Buffer.concat(parts).toString("utf8") }));
      response.on("error", reject);
    });
    request.on("error", reject);
    request.setTimeout(10_000, () => request.destroy(new Error(`${method} ${path} timed out`)));
    request.end(body);
  });
}

export async function mustCall(port, method, path, body, headers) {
  const answer = await call(port, method, path, body, headers);
  if (answer.status !== 200) throw new Error(`${method} ${path} answered ${answer.status}: ${answer.text.slice(0, 200)}`);
  return answer.text;
}
