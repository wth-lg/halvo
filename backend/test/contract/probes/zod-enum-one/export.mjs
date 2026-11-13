import * as z from "zod";

const rejected = z.enum(["rejected"]);
const alone = z.toJSONSchema(rejected, { io: "input" });
const inObject = z.toJSONSchema(z.strictObject({ result: rejected }), { io: "input" });
console.log(JSON.stringify({ alone, inObject }, null, 2));
