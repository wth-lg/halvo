const sqlite = process.getBuiltinModule("node:sqlite");
const prepare = sqlite.DatabaseSync.prototype.prepare;
const prepared = new WeakMap();
sqlite.DatabaseSync.prototype.prepare = function prepareOnce(query) {
  let statements = prepared.get(this);
  if (statements === undefined) {
    statements = new Map();
    prepared.set(this, statements);
  }
  let statement = statements.get(query);
  if (statement === undefined) {
    statement = prepare.call(this, query);
    statements.set(query, statement);
  }
  return statement;
};

const { propertyRun, searchCases, searchOne } = await import("./property.ts");
const cases = searchCases();

async function run(job) {
  const started = performance.now();
  if (job.kind === "seed") {
    const result = await propertyRun({ seed: job.seed, orderings: job.orderings });
    return { ...result, seconds: (performance.now() - started) / 1000 };
  }
  const searchCase = cases[job.index];
  if (searchCase === undefined) throw new RangeError(`no search setting ${job.index}`);
  const result = await searchOne(searchCase);
  return { ...result, seconds: (performance.now() - started) / 1000 };
}

process.on("message", (job) => {
  if (job.kind === "exit") {
    process.disconnect();
    return;
  }
  run(job).then(
    (result) => process.send({ id: job.id, result }),
    (error) => process.send({ id: job.id, error: `${error?.name ?? "Error"}: ${String(error?.message ?? error).split("\n")[0]}` }),
  );
});
process.send({ ready: cases.length });
