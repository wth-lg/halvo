export default {
  async test(_controller, env) {
    try {
      await fetch(env.TARGET);
      console.log("worker fetch: answered");
    } catch (error) {
      console.log("worker fetch:", error.message);
    }
  },
};
