(function () {
  "use strict";
  var w = Halvo.words;
  var inventory = Halvo.feed("inventory");
  var modalityNames = {
    screen_frame: w.SCREEN_ACTIVITY,
    keystroke: "Typing",
    pointer: "Mouse",
    foreground_app: "App in front",
    window_title: "Window titles",
    system_audio: "System audio",
    microphone: "Microphone",
    camera: "Camera",
  };
  var RETRIES = { io: true, locked: true };
  var total;
  var logical;
  var cap;
  var storeLine;
  var storeText;
  var custody;
  var unavailable;
  var empty;
  var table;
  var body;
  var dayLine;
  var running = false;
  var lastInventory = null;

  function storeWords(store) {
    if (!store) return ["", ""];
    var retry = store.retry_in_s !== null && store.retry_in_s !== undefined && RETRIES[store.error] ? " " + w.STORE_RETRY.replace("{seconds}", String(store.retry_in_s)) : "";
    switch (store.state) {
      case "open":
        return ["", ""];
      case "opening":
        return [w.STORE_OPENING, ""];
      case "deleting":
        return [w.STORE_DELETING, ""];
      case "sealed":
        return [store.error === "custody_kind" ? w.STORE_SEALED_CUSTODY : w.STORE_SEALED, ""];
      case "no_release_custody":
        return [w.STORE_NO_RELEASE_CUSTODY, ""];
      case "unsupported_version":
        return [store.error === "newer" ? w.STORE_NEWER : w.STORE_OLDER, ""];
      case "unavailable":
        if (store.error === "obstructed") return [w.OBSTRUCTED_HEADING, w.OBSTRUCTED_TEXT];
        if (store.error === "locked") return [w.STORE_LOCKED + retry, ""];
        if (store.error === "root_denied") return [w.STORE_ROOT_DENIED, ""];
        return [w.STORE_UNAVAILABLE + retry, ""];
      default:
        return [w.STORE_UNKNOWN + " " + String(store.state), ""];
    }
  }

  function bytes(n) {
    return typeof n === "number" ? Halvo.formatBytes(n) : "—";
  }

  function renderStatus(s) {
    running = s.service === "running";
    total.textContent = bytes(s.store_bytes);
    logical.textContent = bytes(s.store_logical_bytes);
    cap.textContent = typeof s.store_cap_bytes === "number" ? Halvo.formatBytes(s.store_cap_bytes) : running ? w.CAP_UNKNOWN : "—";
    var words = storeWords(s.store);
    storeLine.textContent = words[0];
    storeText.textContent = words[1];
    custody.textContent = s.texts && s.texts.custody ? s.texts.custody : "";
    if (!running) showUnavailable();
    else if (lastInventory !== null) render(lastInventory);
  }

  function showUnavailable() {
    unavailable.classList.remove("hidden");
    empty.classList.add("hidden");
    table.classList.add("hidden");
    body.replaceChildren();
  }

  function render(inv) {
    lastInventory = null;
    if (!inv || !Array.isArray(inv.rows)) {
      showUnavailable();
      throw new Error("the inventory reply has no list of rows");
    }
    if (!running || inv.available !== true) {
      lastInventory = inv.available === true ? inv : null;
      showUnavailable();
      return;
    }
    lastInventory = inv;
    unavailable.classList.add("hidden");
    var rows = inv.rows;
    empty.classList.toggle("hidden", rows.length > 0);
    table.classList.toggle("hidden", rows.length === 0);
    body.replaceChildren.apply(
      body,
      rows.map(function (r) {
        var cells = [
          [r.day, ""],
          [modalityNames[r.modality] || r.modality, ""],
          [Number(r.events).toLocaleString(), "num"],
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

  function localTimeOfUtcMidnight() {
    var offset = new Date(0).getTimezoneOffset();
    var minutes = (((-offset) % 1440) + 1440) % 1440;
    var hh = Math.floor(minutes / 60);
    var mm = minutes % 60;
    return (hh < 10 ? "0" : "") + hh + ":" + (mm < 10 ? "0" : "") + mm;
  }

  function refresh() {
    return Halvo.ipc.call("inventory").then(inventory.set).catch(Halvo.showError);
  }

  Halvo.clearStored = function () {
    lastInventory = null;
    body.replaceChildren();
    table.classList.add("hidden");
    total.textContent = "—";
    logical.textContent = "—";
  };

  function heading(text, cls) {
    return Halvo.el("th", cls ? { class: cls, text: text } : { text: text });
  }

  Halvo.views.add({
    id: "stored",
    order: 4,
    heading: "Stored on this computer",
    mount: function (el) {
      total = Halvo.el("span", { id: "total", text: "—" });
      logical = Halvo.el("span", { id: "logical", text: "—" });
      cap = Halvo.el("span", { id: "cap", text: "—" });
      storeLine = Halvo.el("p", { id: "store-state" });
      storeText = Halvo.el("p", { id: "store-text", class: "muted" });
      custody = Halvo.el("p", { id: "custody", class: "muted" });
      unavailable = Halvo.el("div", { id: "inventory-unavailable", class: "muted hidden", text: w.INVENTORY_UNAVAILABLE });
      empty = Halvo.el("div", { id: "empty", class: "muted hidden", text: w.NOTHING_STORED });
      dayLine = Halvo.el("p", { id: "day-start", class: "muted", text: w.DAY_STARTS.replace("{hour}", localTimeOfUtcMidnight()) });
      body = Halvo.el("tbody");
      table = Halvo.el("table", { id: "inventory", class: "hidden" }, [
        Halvo.el("thead", {}, [Halvo.el("tr", {}, [heading(w.DAY_UTC), heading("What"), heading("Events", "num"), heading("Size", "num")])]),
        body,
      ]);
      el.append(
        Halvo.el("p", {}, [w.ON_DISK + ": ", total, " " + w.OF_A_CAP + " ", cap, ". " + w.RECORDED + ": ", logical, "."]),
        Halvo.el("p", { class: "muted", text: w.GB_IS }),
        storeLine,
        storeText,
        custody,
        unavailable,
        empty,
        table,
        dayLine,
      );
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
