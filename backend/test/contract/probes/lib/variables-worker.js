export default {
  async fetch(_request, env) {
    return Response.json({ ORIGIN: env.ORIGIN ?? null, A: env.A ?? null });
  },
};
