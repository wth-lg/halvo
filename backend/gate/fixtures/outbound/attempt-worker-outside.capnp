using Workerd = import "/workerd/workerd.capnp";

const config :Workerd.Config = (
  services = [
    (name = "main", worker = (
      modules = [(name = "worker", esModule = embed "attempt-worker.mjs")],
      compatibilityDate = "2026-01-01",
      bindings = [(name = "TARGET", text = "http://192.0.2.1:9/")],
    )),
    (name = "internet", network = (allow = ["192.0.2.0/24"])),
  ],
);
