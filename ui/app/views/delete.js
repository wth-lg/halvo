(function () {
  "use strict";
  var start;
  var confirm;
  var cancel;
  var result;

  function disarm() {
    start.classList.remove("hidden");
    confirm.classList.add("hidden");
    cancel.classList.add("hidden");
  }

  function arm() {
    start.classList.add("hidden");
    confirm.classList.remove("hidden");
    cancel.classList.remove("hidden");
    result.textContent = "";
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
        disarm();
      });
  }

  Halvo.views.add({
    id: "delete-everything",
    order: 6,
    heading: "Delete everything",
    mount: function (el) {
      start = Halvo.el("button", { id: "delete", class: "danger", text: "Delete everything" });
      confirm = Halvo.el("button", { id: "confirm-delete", class: "danger hidden", text: "Yes, delete everything" });
      cancel = Halvo.el("button", { id: "cancel-delete", class: "hidden", text: "Cancel" });
      result = Halvo.el("span", { id: "delete-result", class: "muted" });
      start.onclick = arm;
      cancel.onclick = disarm;
      confirm.onclick = remove;
      el.append(
        Halvo.el("p", { class: "muted", text: "Destroys the keys to every stored capture so nothing can be read back. This cannot be undone." }),
        Halvo.el("div", { class: "row" }, [start, confirm, cancel, result]),
      );
    },
    show: function () {},
    hide: function () {},
  });
})();
