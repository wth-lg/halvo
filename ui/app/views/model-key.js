(function () {
  "use strict";
  var Halvo = window.Halvo;
  var w = Halvo.words;
  var host = null;
  var state = null;
  var section = null;
  var label, text, field, saveButton, stateLine, said, removeButton, confirmRemove, removeRow, removeQuestion, partsBox;
  var parts = [];
  var mounted = 0;
  var lines = [];
  /** The replies of save_model_key and remove_model_key that set the state or read it again: a state read that one of them overtook changes nothing. */
  var answers = 0;
  var flying = false;
  var WAITS = [1000, 2000, 4000, 8000, 16000];
  var rows = [];

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
    /**
     * A part that reads that Halvo cannot tell asks again by itself: behind each answer that leaves it so, after
     * WAITS[0] ms, then WAITS[1] ms, and so on to the end of WAITS, and then no more. A wait begins only while the
     * window is shown; a hiding ends the pending one, and a showing begins the row anew.
     */
    asksAgain: function (ask, cannotTell) {
      var timer = null;
      var asked = 0;
      function end() {
        if (timer !== null) clearTimeout(timer);
        timer = null;
      }
      rows.push({
        end: end,
        cannotTell: cannotTell,
        anew: function () {
          end();
          asked = 0;
          ask();
        },
      });
      return function () {
        end();
        if (!cannotTell()) {
          asked = 0;
        } else if (asked < WAITS.length && document.visibilityState !== "hidden") {
          timer = setTimeout(function () {
            timer = null;
            asked += 1;
            ask();
          }, WAITS[asked]);
        }
      };
    },
    /** The page learned that the capture service runs: where the window is shown, each part that cannot tell asks at once and begins its row anew. */
    serviceBack: function () {
      if (document.visibilityState === "hidden") return;
      rows.forEach(function (row) {
        if (row.cannotTell()) row.anew();
      });
    },
  };

  /**
   * A line beneath a question that names the person's key: `words` with {host}, shown only while the state reads
   * that a key is saved. delete.js and status.js put one beneath each of their questions.
   */
  Halvo.modelKeyLine = function (id, words) {
    var line = Halvo.el("p", { id: id, class: "hidden" });
    line.classList.add("muted");
    lines.push({ element: line, words: words });
    drawLines();
    return line;
  };

  function withHost(words) {
    return words.split("{host}").join(host || "");
  }

  function drawLines() {
    var named = host !== null && state === "saved";
    lines.forEach(function (line) {
      line.element.textContent = named ? withHost(line.words) : "";
      line.element.classList.toggle("hidden", !named);
    });
  }

  function stateWords() {
    if (state === "saved") return w.MODEL_KEY_SAVED;
    if (state === "none") return w.MODEL_KEY_NONE;
    return w.MODEL_KEY_NOT_KNOWN;
  }

  function draw() {
    drawLines();
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
    var at = answers;
    return Halvo.ipc.call("model_key_state").then(
      function (reply) {
        if (at !== answers) return;
        var named = reply && typeof reply.host === "string" && reply.host !== "";
        take(named ? reply.host : null, named && (reply.state === "saved" || reply.state === "none") ? reply.state : "not_known");
      },
      function () {
        if (at !== answers) return;
        take(host, "not_known");
      },
    );
  }

  function take(namedHost, readState) {
    var stands = namedHost === host && readState === state;
    host = namedHost;
    state = readState;
    if (!stands) draw();
    later();
  }

  var later = Halvo.modelSection.asksAgain(read, function () {
    return host !== null && state === "not_known";
  });

  /** While save_model_key or remove_model_key is in flight the three controls send nothing; Save the key also waits for a text in the field. */
  function controls() {
    saveButton.disabled = flying || field.value === "";
    removeButton.disabled = flying;
    confirmRemove.disabled = flying;
  }

  /** From the press that sends save_model_key or remove_model_key until its reply or its failure. */
  function inFlight(on) {
    flying = on;
    controls();
  }

  function answered(newState) {
    answers += 1;
    state = newState;
    draw();
    later();
  }

  function failed(words) {
    answers += 1;
    said.textContent = words;
    read();
  }

  /** Sends the field's text in ONE command and empties the field at the press, before any answer. */
  function onSave() {
    var pasted = field.value;
    field.value = "";
    said.textContent = "";
    inFlight(true);
    Halvo.ipc.call("save_model_key", { key: pasted }).then(
      function (reply) {
        inFlight(false);
        if (reply && reply.state === "saved") {
          answered("saved");
        } else if (reply && reply.error === "no_key") {
          said.textContent = w.MODEL_KEY_NOT_A_KEY;
        } else {
          failed(w.MODEL_KEY_SAVE_FAILED);
        }
      },
      function () {
        inFlight(false);
        failed(w.MODEL_KEY_SAVE_FAILED);
      },
    );
  }

  function onRemove() {
    removeRow.classList.add("hidden");
    said.textContent = "";
    inFlight(true);
    Halvo.ipc.call("remove_model_key").then(
      function (reply) {
        inFlight(false);
        if (reply && reply.state === "none") {
          answered("none");
        } else {
          failed(w.MODEL_KEY_REMOVE_FAILED);
        }
      },
      function () {
        inFlight(false);
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
      field.addEventListener("input", controls);
      saveButton = Halvo.el("button", { id: "model-key-save", text: w.MODEL_KEY_SAVE });
      saveButton.onclick = onSave;
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
      confirmRemove = Halvo.el("button", { id: "model-key-confirm-remove", class: "danger", text: w.MODEL_KEY_REMOVE });
      confirmRemove.onclick = onRemove;
      removeQuestion = Halvo.el("p", { id: "model-key-remove-question" });
      removeRow = Halvo.el("div", { id: "model-key-remove-row", class: "hidden" }, [
        removeQuestion,
        Halvo.el("div", { class: "row" }, [keep, confirmRemove]),
      ]);
      removeRow.classList.add("confirm-row");
      partsBox = Halvo.el("div", { id: "model-key-parts" });
      [label, text, Halvo.el("div", { class: "row" }, [field, saveButton]), stateLine, said, removeButton, removeRow, partsBox].forEach(function (child) {
        element.appendChild(child);
      });
      controls();
      draw();
    },
    show: function () {
      rows.forEach(function (row) {
        row.anew();
      });
    },
    hide: function () {
      rows.forEach(function (row) {
        row.end();
      });
    },
  });
})();
