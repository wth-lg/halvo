const TICK_MS = 500;
const RUN_MS = 60_000;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function work(id, writer) {
  const began = Date.now();
  let gone = null;
  const encoder = new TextEncoder();
  while (Date.now() - began < RUN_MS) {
    const at = Date.now() - began;
    if (gone === null) {
      try {
        await writer.write(encoder.encode(`data: ${at}\n\n`));
      } catch {
        gone = at;
        console.log(JSON.stringify({ probe: id, gone: at }));
      }
    }
    console.log(JSON.stringify({ probe: id, tick: at }));
    await sleep(TICK_MS);
  }
  console.log(JSON.stringify({ probe: id, end: Date.now() - began }));
}

export default {
  fetch(request, _env, ctx) {
    const url = new URL(request.url);
    if (url.pathname !== "/start") return new Response("ready\n");
    const { readable, writable } = new TransformStream();
    ctx.waitUntil(work(url.searchParams.get("id") ?? "probe", writable.getWriter()));
    return new Response(readable, { headers: { "content-type": "text/event-stream" } });
  },
};
