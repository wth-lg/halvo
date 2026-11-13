import { unstated } from "unstated";
import { native } from "native";
import { both } from "@scope/both";

export default {
  fetch() {
    return new Response(unstated + native + both);
  },
};
