import { SELF } from "cloudflare:test";
import { expect, test } from "vitest";

test("the deployable entry answers a path outside the route table with 404 and the rejection body", async () => {
  const response = await SELF.fetch("http://backend.test/");
  expect(response.status).toBe(404);
  expect(await response.text()).toBe('{"result":"rejected"}');
});
