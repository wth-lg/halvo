(function () {
  "use strict";
  var GB = 1024 * 1024 * 1024;
  var field;

  function save() {
    var gb = Number(field.value);
    return Halvo.ipc
      .call("set_storage_cap", { capBytes: Math.round(gb * GB) })
      .then(function (s) {
        Halvo.feed("status").set(s);
        return Halvo.feed("inventory").refresh();
      })
      .then(function () {
        Halvo.showError("");
      })
      .catch(Halvo.showError);
  }

  Halvo.views.add({
    id: "storage-cap",
    order: 5,
    heading: "Storage cap",
    mount: function (el) {
      field = Halvo.el("input", { id: "cap-gb", type: "number", min: "1", step: "1" });
      var button = Halvo.el("button", { id: "save-cap", text: "Save" });
      button.onclick = save;
      el.append(
        Halvo.el("div", { class: "row" }, [
          Halvo.el("label", { for: "cap-gb", text: "Keep at most" }),
          field,
          Halvo.el("span", { text: "GB" }),
          button,
          Halvo.el("span", { class: "muted", text: "When the cap is reached the oldest days are deleted first." }),
        ]),
      );
      Halvo.feed("status").on(function (s) {
        if (document.activeElement !== field) field.value = Math.round(s.store_cap_bytes / GB);
      });
    },
    show: function () {},
    hide: function () {},
  });
})();
