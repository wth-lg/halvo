(function () {
  "use strict";
  var Halvo = window.Halvo;
  var w = Halvo.words;
  var host = null;
  var state = null;
  var section = null;
  var label, text, field, stateLine, partsBox;
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

  function draw() {
    if (section === null) return;
    section.classList.toggle("hidden", host === null);
    if (host === null) return;
    label.textContent = withHost(w.MODEL_KEY_LABEL);
    text.textContent = withHost(w.MODEL_KEY_TEXT);
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
      var save = Halvo.el("button", { id: "model-key-save", text: w.MODEL_KEY_SAVE });
      stateLine = Halvo.el("p", { id: "model-key-state" });
      partsBox = Halvo.el("div", { id: "model-key-parts" });
      [label, text, Halvo.el("div", { class: "row" }, [field, save]), stateLine, partsBox].forEach(function (child) {
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
