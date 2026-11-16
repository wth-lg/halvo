(function () {
  "use strict";
  var w = Halvo.words;
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
  var saving = false;
  var saveButton;
  var storage;
  var evicting;
  var evictionStore = null;
  var NO_STATUS_WORDS = {
    not_running: w.CAP_NOT_REACHED,
    held_unreachable: w.CAP_NOT_REACHED,
    unsupported: w.CAP_NOT_CHANGED,
    status_refused: w.CAP_UNKNOWN_OUTCOME,
  };

  function parse(text) {
    var value = String(text).trim();
    var gb = Number(value);
    if (value === "" || !isFinite(gb)) return { error: w.CAP_ENTER };
    if (gb < MIN_GB || gb > MAX_GB) return { error: w.CAP_RANGE.replace("{min}", String(MIN_GB)).replace("{max}", String(MAX_GB)) };
    return { gb: gb, bytes: Math.round(gb * GB) };
  }

  function closeConfirm() {
    pending = null;
    confirmRow.classList.add("hidden");
  }

  function capField(s) {
    return typeof s.store_cap_bytes === "number" ? String(Math.round(s.store_cap_bytes / GB)) : "";
  }

  function send(bytes) {
    if (saving) return Promise.resolve();
    closeConfirm();
    saving = true;
    saveButton.disabled = true;
    Halvo.showError("");
    var sentText = field.value;
    return Halvo.ipc
      .call("set_storage_cap", { capBytes: bytes })
      .then(function (s) {
        if (s.service !== "running") {
          dirty = true;
          clearEviction();
          Halvo.takeMutationReply(s);
          message.textContent = NO_STATUS_WORDS[s.service] || w.CAP_UNKNOWN_OUTCOME;
          return null;
        }
        Halvo.takeMutationReply(s);
        if (field.value === sentText) {
          dirty = false;
          field.value = capField(s);
        }
        clearEviction();
        if (s.evicting === true) {
          evicting.textContent = w.CAP_EVICTING;
          evictionStore = s.store && s.store.id ? s.store.id : null;
        }
        return Halvo.feed("inventory").refresh();
      })
      .catch(function (e) {
        if (e && typeof e === "object" && e.outcome_not_known === true) {
          dirty = true;
          clearEviction();
          message.textContent = Halvo.errorWords(e) + " " + w.CAP_ANSWER_LOST;
          return;
        }
        Halvo.showError(e);
      })
      .then(function () {
        saving = false;
        saveButton.disabled = false;
      });
  }

  function clearEviction() {
    evicting.textContent = "";
    evictionStore = null;
  }

  function evictionEndsAt(s) {
    if (s.service !== "running") return true;
    if (typeof s.store_bytes !== "number" || typeof s.store_cap_bytes !== "number") return true;
    if (!s.store || s.store.id !== evictionStore) return true;
    return s.store_bytes <= s.store_cap_bytes;
  }

  function save() {
    var cap = parse(field.value);
    if (cap.error) {
      message.textContent = cap.error;
      closeConfirm();
      return Promise.resolve();
    }
    if (stored === null) {
      message.textContent = w.CAP_NOT_KNOWN_YET;
      return Promise.resolve();
    }
    message.textContent = "";
    if (stored > cap.bytes) {
      pending = cap.bytes;
      question.textContent = w.CAP_LOWER.replace("{gb}", String(cap.gb)).replace("{amount}", Halvo.formatBytes(stored - cap.bytes));
      confirmRow.classList.remove("hidden");
      return Promise.resolve();
    }
    return send(cap.bytes);
  }

  Halvo.views.add({
    id: "storage-cap",
    order: 5,
    heading: w.STORAGE_CAP,
    mount: function (el) {
      field = Halvo.el("input", { id: "cap-gb", type: "number", min: String(MIN_GB), max: String(MAX_GB), step: "1" });
      field.addEventListener("input", function () {
        dirty = true;
        closeConfirm();
      });
      message = Halvo.el("span", { id: "cap-message", class: "field-message" });
      var button = Halvo.el("button", { id: "save-cap", text: w.SAVE });
      button.onclick = save;
      saveButton = button;
      storage = Halvo.el("p", { id: "cap-storage", class: "muted" });
      evicting = Halvo.el("p", { id: "cap-evicting" });
      question = Halvo.el("p", { id: "cap-question" });
      var confirm = Halvo.el("button", { id: "confirm-cap", class: "danger", text: w.LOWER_CAP });
      var cancel = Halvo.el("button", { id: "cancel-cap", text: w.KEEP_CAP });
      confirm.onclick = function () {
        return pending === null ? Promise.resolve() : send(pending);
      };
      cancel.onclick = closeConfirm;
      confirmRow = Halvo.el("div", { id: "cap-confirm-row", class: "confirm-row hidden" }, [question, Halvo.el("div", { class: "row" }, [confirm, cancel])]);
      el.append(
        Halvo.el("div", { class: "row" }, [
          Halvo.el("label", { for: "cap-gb", text: w.KEEP_AT_MOST }),
          field,
          Halvo.el("span", { text: w.UNIT_GB }),
          button,
          message,
        ]),
        storage,
        evicting,
        confirmRow,
      );
      Halvo.feed("status").on(function (s) {
        if (evicting.textContent !== "" && evictionEndsAt(s)) clearEviction();
        stored = typeof s.store_bytes === "number" ? s.store_bytes : null;
        storage.textContent = s.texts && s.texts.storage ? s.texts.storage : "";
        if (!dirty && document.activeElement !== field) field.value = capField(s);
      });
    },
    show: function () {},
    hide: closeConfirm,
  });
})();
