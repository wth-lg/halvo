const PAGE =
  '<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Halvo</title></head><body><main><h1>Done</h1><p>You can close this page and go back to Halvo.</p></main></body></html>';

export function done(): Response {
  return new Response(PAGE, { status: 200, headers: { "content-type": "text/html; charset=utf-8" } });
}
