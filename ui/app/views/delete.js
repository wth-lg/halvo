(function () {
  "use strict";
  var ARM_DELAY_MS = 500;
  var DISARM_AFTER_MS = 10000;
  var start;
  var confirmRow;
  var confirm;
  var cancel;
  var result;
  var armedAt = null;
  var disarmTimer = null;

  function disarm() {
    if (disarmTimer !== null) clearTimeout(disarmTimer);
    disarmTimer = null;
    armedAt = null;
    start.disabled = false;
    confirmRow.classList.add("hidden");
  }

  function arm() {
    if (armedAt !== null) return;
    armedAt = performance.now();
    start.disabled = true;
    confirmRow.classList.remove("hidden");
    result.textContent = "";
    disarmTimer = setTimeout(disarm, DISARM_AFTER_MS);
  }

  function remove() {
    confirm.disabled = true;
    return Halvo.ipc
      .call("delete_all")
      .then(function (r) {
        result.textContent = "Deleted. " + r.segments_shredded + " segments shredded, " + r.keys_destroyed + " keys destroyed.";
        Halvo.showError("");
        return Halvo.feed("status")
          .refresh()
          .then(function () {
            return Halvo.feed("inventory").refresh();
          });
      })
      .catch(function (e) {
        result.textContent = "";
        Halvo.showError(e);
      })
      .then(function () {
        confirm.disabled = false;
      });
  }

  function onConfirm(event) {
    if (armedAt === null) return Promise.resolve();
    if (event && event.detail > 1) return Promise.resolve();
    var since = performance.now() - armedAt;
    if (since < ARM_DELAY_MS) return Promise.resolve();
    if (since >= DISARM_AFTER_MS) {
      disarm();
      return Promise.resolve();
    }
    disarm();
    return remove();
  }

  Halvo.views.add({
    id: "delete-everything",
    order: 6,
    heading: "Delete everything",
    mount: function (el) {
      start = Halvo.el("button", { id: "delete", class: "danger", text: "Delete everything" });
      confirm = Halvo.el("button", { id: "confirm-delete", class: "danger", text: "Yes, delete everything" });
      cancel = Halvo.el("button", { id: "cancel-delete", text: "Cancel" });
      result = Halvo.el("span", { id: "delete-result", class: "muted" });
      confirmRow = Halvo.el("div", { id: "delete-confirm-row", class: "confirm-row hidden" }, [
        Halvo.el("p", {
          id: "delete-question",
          text: "Delete everything that Halvo has recorded on this computer? This cannot be undone. Copies that Halvo made next to your own files stay.",
        }),
        Halvo.el("div", { class: "row" }, [confirm, cancel]),
      ]);
      start.onclick = arm;
      cancel.onclick = disarm;
      confirm.onclick = onConfirm;
      el.append(
        Halvo.el("p", { class: "muted", text: "Destroys the keys to every stored capture so nothing can be read back. This cannot be undone." }),
        Halvo.el("div", { class: "row" }, [start, result]),
        confirmRow,
      );
    },
    show: function () {},
    hide: disarm,
  });
})();
