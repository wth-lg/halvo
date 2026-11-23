(function () {
  "use strict";
  var Halvo = window.Halvo;
  var w = Halvo.words;
  var host = null;
  var state = null;
  var section = null;
  var label, text, field, stateLine, said, removeButton, removeRow, removeQuestion, partsBox;
  var parts = [];
  var mounted = 0;

  /**
   * The section of the AI model, which shows only in a build whose app names a host (model_key_state), is open to
   * the parts of other views: a view file calls Halvo.modelSection.add(part) when it loads, and this view calls
   * part.mount(element, host) once, with an element of its own behind the section's own lines, when the app first
   * names a host. A part is added with no edit of this file.
   */
  Halvo.modelSection = {
    add: function (part) {
      parts.push(part);
    },
  };

  function withHost(words) {
    return words.split("{host}").join(host || "");
  }

  function stateWords() {
    if (state === "saved") return w.MODEL_KEY_SAVED;
    if (state === "none") return w.MODEL_KEY_NONE;
    return w.MODEL_KEY_NOT_KNOWN;
  }

  function draw() {
    if (section === null) return;
    section.classList.toggle("hidden", host === null);
    if (host === null) return;
    label.textContent = withHost(w.MODEL_KEY_LABEL);
    text.textContent = withHost(w.MODEL_KEY_TEXT);
    removeQuestion.textContent = withHost(w.MODEL_KEY_REMOVE_QUESTION);
    stateLine.textContent = stateWords();
    removeButton.classList.toggle("hidden", state !== "saved");
    if (state !== "saved") removeRow.classList.add("hidden");
    while (mounted < parts.length) {
      var box = Halvo.el("div", {});
      partsBox.appendChild(box);
      parts[mounted].mount(box, host);
      mounted += 1;
    }
  }

  /** Reads the host and the state from the app; a failed read leaves no host unnamed and a named one not known. */
  function read() {
    return Halvo.ipc.call("model_key_state").then(
      function (reply) {
        var named = reply && typeof reply.host === "string" && reply.host !== "";
        host = named ? reply.host : null;
        state = named && (reply.state === "saved" || reply.state === "none") ? reply.state : "not_known";
        draw();
      },
      function () {
        state = "not_known";
        draw();
      },
    );
  }

  function failed(words) {
    said.textContent = words;
    read();
  }

  function onRemove() {
    removeRow.classList.add("hidden");
    said.textContent = "";
    Halvo.ipc.call("remove_model_key").then(
      function (reply) {
        if (reply && reply.state === "none") {
          state = "none";
          draw();
        } else {
          failed(w.MODEL_KEY_REMOVE_FAILED);
        }
      },
      function () {
        failed(w.MODEL_KEY_REMOVE_FAILED);
      },
    );
  }

  Halvo.views.add({
    id: "model-key",
    order: 30,
    heading: w.MODEL_HEADING,
    mount: function (element) {
      section = element.parentNode;
      section.classList.add("hidden");
      label = Halvo.el("label", { id: "model-key-label", for: "model-key-field" });
      text = Halvo.el("p", { id: "model-key-text", class: "muted" });
      field = Halvo.el("input", { id: "model-key-field" });
      field.setAttribute("type", "password");
      field.setAttribute("autocomplete", "off");
      field.setAttribute("autocorrect", "off");
      field.setAttribute("autocapitalize", "off");
      field.setAttribute("spellcheck", "false");
      var save = Halvo.el("button", { id: "model-key-save", text: w.MODEL_KEY_SAVE });
      stateLine = Halvo.el("p", { id: "model-key-state" });
      said = Halvo.el("p", { id: "model-key-said" });
      said.setAttribute("role", "status");
      removeButton = Halvo.el("button", { id: "model-key-remove", class: "hidden", text: w.MODEL_KEY_REMOVE });
      removeButton.classList.add("danger");
      removeButton.onclick = function () {
        removeRow.classList.remove("hidden");
      };
      var keep = Halvo.el("button", { id: "model-key-keep", text: w.MODEL_KEY_KEEP });
      keep.onclick = function () {
        removeRow.classList.add("hidden");
      };
      var confirmRemove = Halvo.el("button", { id: "model-key-confirm-remove", class: "danger", text: w.MODEL_KEY_REMOVE });
      confirmRemove.onclick = onRemove;
      removeQuestion = Halvo.el("p", { id: "model-key-remove-question" });
      removeRow = Halvo.el("div", { id: "model-key-remove-row", class: "hidden" }, [
        removeQuestion,
        Halvo.el("div", { class: "row" }, [keep, confirmRemove]),
      ]);
      removeRow.classList.add("confirm-row");
      partsBox = Halvo.el("div", { id: "model-key-parts" });
      [label, text, Halvo.el("div", { class: "row" }, [field, save]), stateLine, said, removeButton, removeRow, partsBox].forEach(function (child) {
        element.appendChild(child);
      });
      draw();
    },
    show: function () {
      read();
    },
    hide: function () {},
  });
})();
