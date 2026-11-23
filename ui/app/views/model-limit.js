(function () {
  "use strict";
  var Halvo = window.Halvo;
  var w = Halvo.words;

  /**
   * The part of the section AI model that shows what Halvo has counted against the person's limit for hosted models,
   * with ONE control that sets the limit (r7 8.1L). The app answers finished texts (model_limit_state): the page writes
   * no figure of its own, only the day of the period's end on the person's own clock. A press sends the field's text
   * in ONE command (set_model_limit) and names no body: what the app sends is the app's choice.
   */

  /** The sentences of the part by their keys. */
  var SENTENCE = {
    limit: w.MODEL_LIMIT_LIMIT,
    spent: w.MODEL_LIMIT_SPENT,
    held: w.MODEL_LIMIT_HELD,
    period: w.MODEL_LIMIT_PERIOD,
    bill: w.MODEL_LIMIT_BILL,
    pay: w.MODEL_LIMIT_PAY,
    near: w.MODEL_LIMIT_NEAR,
    reached: w.MODEL_LIMIT_REACHED,
    none: w.MODEL_LIMIT_NONE,
    no_limit: w.MODEL_LIMIT_NO_LIMIT,
    unread: w.MODEL_LIMIT_UNREAD,
    not_known: w.MODEL_LIMIT_NOT_KNOWN,
    elsewhere: w.MODEL_LIMIT_ELSEWHERE,
    delete: w.MODEL_LIMIT_DELETE,
  };

  /** The sentences that each state shows, by their keys, in their order. */
  var SHOWN = {
    ok: ["limit", "spent", "held", "period", "bill", "pay", "elsewhere", "delete"],
    near: ["limit", "spent", "held", "near", "period", "bill", "pay", "elsewhere", "delete"],
    reached: ["limit", "spent", "held", "reached", "period", "bill", "pay", "elsewhere", "delete"],
    none: ["none", "bill", "pay", "elsewhere", "delete"],
    no_limit: ["no_limit", "spent", "held"],
    unread: ["unread"],
  };

  var host = null;
  var reply = null;
  var lines, label, field, setButton, said, controlRow;
  var flying = 0;

  function localOf(ms) {
    return new Date(ms - new Date(ms).getTimezoneOffset() * 60000);
  }

  /** The day of `ms` on the person's own clock, as the view of Delete everything writes a day. */
  function dateOf(ms) {
    return localOf(ms).toLocaleDateString(undefined, { timeZone: "UTC", day: "numeric", month: "long", year: "numeric" });
  }

  function filled(words) {
    var end = reply && typeof reply.period_end_ms === "number" && isFinite(reply.period_end_ms) ? dateOf(reply.period_end_ms) : "";
    var texts = {
      "{host}": host || "",
      "{cap}": (reply && reply.cap) || "",
      "{amount}": (reply && reply.cap) || "",
      "{spent}": (reply && reply.spent) || "",
      "{held}": (reply && reply.held) || "",
      "{period}": (reply && reply.period) || "",
      "{date}": end,
    };
    return Object.keys(texts).reduce(function (text, key) {
      return text.split(key).join(texts[key]);
    }, words);
  }

  /** The keys of the sentences of the state of `reply`; the state not_known where the reply holds no known state. */
  function shownKeys() {
    var state = reply && typeof reply.state === "string" && SHOWN.hasOwnProperty(reply.state) ? reply.state : null;
    if (state === null) return ["not_known"];
    return SHOWN[state].filter(function (key) {
      return state !== "no_limit" || (key !== "spent" && key !== "held") || typeof reply.spent === "string";
    });
  }

  function draw() {
    var keys = shownKeys();
    lines.replaceChildren();
    keys.forEach(function (key) {
      lines.appendChild(Halvo.el("p", { id: "model-limit-" + key.split("_").join("-"), text: filled(SENTENCE[key]) }));
    });
    var known = keys[0] !== "not_known";
    controlRow.classList.toggle("hidden", !known);
    label.textContent = known ? filled(w.MODEL_LIMIT_FIELD) : "";
    controls();
  }

  /** While a command of the part is in flight the control sends nothing; it also waits for a text in the field. */
  function controls() {
    setButton.disabled = flying > 0 || field.value === "";
  }

  function inFlight(on) {
    flying += on ? 1 : -1;
    controls();
  }

  /** Reads the state from the app; a failed read shows the state not_known. */
  function read() {
    inFlight(true);
    return Halvo.ipc.call("model_limit_state").then(
      function (answer) {
        inFlight(false);
        reply = answer && typeof answer === "object" ? answer : null;
        draw();
      },
      function () {
        inFlight(false);
        reply = null;
        draw();
      },
    );
  }

  function failed() {
    said.textContent = w.MODEL_LIMIT_FAILED;
    read();
  }

  /** Sends the field's text in ONE command; the answer names the new limit, or the one sentence of its failure. */
  function onSet() {
    if (flying > 0 || field.value === "") return;
    said.textContent = "";
    inFlight(true);
    Halvo.ipc.call("set_model_limit", { amount: field.value }).then(
      function (answer) {
        inFlight(false);
        if (answer && (answer.answer === "set" || answer.answer === "set_new") && typeof answer.amount === "string") {
          var words = answer.answer === "set" ? w.MODEL_LIMIT_SET : w.MODEL_LIMIT_SET_NEW;
          said.textContent = words.split("{amount}").join(answer.amount).split("{period}").join(String(answer.period || ""));
          field.value = "";
          read();
        } else if (answer && answer.error === "no_amount") {
          said.textContent = w.MODEL_LIMIT_NO_AMOUNT;
        } else {
          failed();
        }
      },
      function () {
        inFlight(false);
        failed();
      },
    );
  }

  Halvo.modelSection.add({
    mount: function (element, named) {
      host = named;
      var heading = Halvo.el("h3", { id: "model-limit-heading", text: w.MODEL_LIMIT_HEADING });
      lines = Halvo.el("div", { id: "model-limit-lines" });
      label = Halvo.el("label", { id: "model-limit-field-label", for: "model-limit-field" });
      field = Halvo.el("input", { id: "model-limit-field" });
      field.setAttribute("type", "text");
      field.setAttribute("inputmode", "decimal");
      field.setAttribute("autocomplete", "off");
      field.addEventListener("input", controls);
      setButton = Halvo.el("button", { id: "model-limit-set", text: w.MODEL_LIMIT_CONTROL });
      setButton.onclick = onSet;
      controlRow = Halvo.el("div", { id: "model-limit-control" }, [label, Halvo.el("div", { class: "row" }, [field, setButton])]);
      said = Halvo.el("p", { id: "model-limit-said" });
      said.setAttribute("role", "status");
      element.setAttribute("id", "model-limit");
      [heading, lines, controlRow, said].forEach(function (child) {
        element.appendChild(child);
      });
      draw();
      read();
    },
  });
})();
