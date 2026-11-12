import type { Shape } from "./shape.ts";
import { SIZE } from "./shape.ts";

const shape: Shape = { label: "node-ts", size: SIZE };
console.log(`${shape.label} ${shape.size}`);
