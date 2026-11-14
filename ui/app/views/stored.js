(function () {
  "use strict";
  var inventory = Halvo.feed("inventory");
  var modalityNames = {
    screen_frame: "Screen",
    keystroke: "Typing",
    pointer: "Mouse",
    foreground_app: "App in front",
    window_title: "Window titles",
    system_audio: "System audio",
    microphone: "Microphone",
    camera: "Camera",
  };
  var total;
  var cap;
  var backend;
  var empty;
  var table;
  var body;

  function renderStatus(s) {
    total.textContent = Halvo.formatBytes(s.store_bytes);
    cap.textContent = Halvo.formatBytes(s.store_cap_bytes);
    backend.textContent = s.store_backend ? "(" + s.store_backend + ")" : "";
  }

  function render(inv) {
    if (!inv || !Array.isArray(inv.rows)) {
      empty.classList.add("hidden");
      table.classList.add("hidden");
      throw new Error("the inventory reply has no list of rows");
    }
    var rows = inv.rows;
    empty.classList.toggle("hidden", rows.length > 0);
    table.classList.toggle("hidden", rows.length === 0);
    body.replaceChildren.apply(
      body,
      rows.map(function (r) {
        var cells = [
          [r.day, ""],
          [modalityNames[r.modality] || r.modality, ""],
          [r.events.toLocaleString(), "num"],
          [Halvo.formatBytes(r.bytes), "num"],
        ];
        return Halvo.el(
          "tr",
          {},
          cells.map(function (cell) {
            var td = Halvo.el("td", { text: cell[0] });
            if (cell[1]) td.className = cell[1];
            return td;
          }),
        );
      }),
    );
  }

  function refresh() {
    return Halvo.ipc.call("inventory").then(inventory.set).catch(Halvo.showError);
  }

  function heading(text, cls) {
    return Halvo.el("th", cls ? { class: cls, text: text } : { text: text });
  }

  Halvo.views.add({
    id: "stored",
    order: 4,
    heading: "Stored on this computer",
    mount: function (el) {
      total = Halvo.el("span", { id: "total", text: "0 B" });
      cap = Halvo.el("span", { id: "cap", text: "—" });
      backend = Halvo.el("span", { id: "backend", class: "muted" });
      empty = Halvo.el("div", { id: "empty", class: "muted", text: "Nothing stored yet." });
      body = Halvo.el("tbody");
      table = Halvo.el("table", { id: "inventory", class: "hidden" }, [
        Halvo.el("thead", {}, [Halvo.el("tr", {}, [heading("Day"), heading("What"), heading("Events", "num"), heading("Size", "num")])]),
        body,
      ]);
      el.append(Halvo.el("p", {}, [total, " of a ", cap, " cap. ", backend]), empty, table);
      Halvo.feed("status").on(renderStatus);
      inventory.on(render);
      inventory.source(refresh);
      refresh();
      setInterval(refresh, 15000);
    },
    show: function () {},
    hide: function () {},
  });
})();
