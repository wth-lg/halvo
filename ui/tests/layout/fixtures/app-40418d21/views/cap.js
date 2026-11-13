(function () {
  "use strict";
  var GB = 1024 * 1024 * 1024;
  var MIN_GB = 1;
  var MAX_GB = 1024;
  var field;
  var message;
  var confirmRow;
  var question;
  var dirty = false;
  var stored = null;
  var pending = null;

  function parse(text) {
    var value = String(text).trim();
    var gb = Number(value);
    if (value === "" || !isFinite(gb)) return { error: "Enter the cap in GB." };
    if (gb < MIN_GB) return { error: "The cap must be at least " + MIN_GB + " GB." };
    if (gb > MAX_GB) return { error: "The cap can be at most " + MAX_GB + " GB." };
    return { gb: gb, bytes: Math.round(gb * GB) };
  }

  function closeConfirm() {
    pending = null;
    confirmRow.classList.add("hidden");
  }

  function send(bytes) {
    closeConfirm();
    return Halvo.ipc
      .call("set_storage_cap", { capBytes: bytes })
      .then(function (s) {
        dirty = false;
        Halvo.feed("status").set(s);
        field.value = Math.round(s.store_cap_bytes / GB);
        return Halvo.feed("inventory").refresh();
      })
      .then(function () {
        Halvo.showError("");
      })
      .catch(Halvo.showError);
  }

  function save() {
    var cap = parse(field.value);
    if (cap.error) {
      message.textContent = cap.error;
      closeConfirm();
      return Promise.resolve();
    }
    if (stored === null) {
      message.textContent = "The stored size is not known yet. Try again in a moment.";
      return Promise.resolve();
    }
    message.textContent = "";
    if (stored > cap.bytes) {
      pending = cap.bytes;
      question.textContent =
        "A cap of " + cap.gb + " GB deletes about " + Halvo.formatBytes(stored - cap.bytes) + " of the oldest capture now. This cannot be undone.";
      confirmRow.classList.remove("hidden");
      return Promise.resolve();
    }
    return send(cap.bytes);
  }

  Halvo.views.add({
    id: "storage-cap",
    order: 5,
    heading: "Storage cap",
    mount: function (el) {
      field = Halvo.el("input", { id: "cap-gb", type: "number", min: String(MIN_GB), max: String(MAX_GB), step: "1" });
      field.addEventListener("input", function () {
        dirty = true;
        closeConfirm();
      });
      message = Halvo.el("span", { id: "cap-message", class: "field-message" });
      var button = Halvo.el("button", { id: "save-cap", text: "Save" });
      button.onclick = save;
      question = Halvo.el("p", { id: "cap-question" });
      var confirm = Halvo.el("button", { id: "confirm-cap", class: "danger", text: "Lower the cap" });
      var cancel = Halvo.el("button", { id: "cancel-cap", text: "Keep the current cap" });
      confirm.onclick = function () {
        return pending === null ? Promise.resolve() : send(pending);
      };
      cancel.onclick = closeConfirm;
      confirmRow = Halvo.el("div", { id: "cap-confirm-row", class: "confirm-row hidden" }, [question, Halvo.el("div", { class: "row" }, [confirm, cancel])]);
      el.append(
        Halvo.el("div", { class: "row" }, [
          Halvo.el("label", { for: "cap-gb", text: "Keep at most" }),
          field,
          Halvo.el("span", { text: "GB" }),
          button,
          message,
        ]),
        Halvo.el("p", { class: "muted", text: "When the cap is reached the oldest days are deleted first." }),
        confirmRow,
      );
      Halvo.feed("status").on(function (s) {
        stored = s.store_bytes;
        if (!dirty && document.activeElement !== field) field.value = Math.round(s.store_cap_bytes / GB);
      });
    },
    show: function () {},
    hide: closeConfirm,
  });
})();
