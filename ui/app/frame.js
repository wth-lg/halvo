(function () {
  "use strict";
  var Halvo = window.Halvo || (window.Halvo = {});
  var entries = [];
  var feeds = {};
  var order = [];
  var holding = false;

  Halvo.views = {
    add: function (view) {
      entries.push(view);
    },
    window: function (state) {
      entries.forEach(function (view) {
        if (state === "shown") view.show();
        else view.hide();
      });
    },
    /** While held, no feed draws: the newest reply of each feed is kept and drawn at the release, in the order of the views. */
    hold: function () {
      holding = true;
    },
    release: function () {
      holding = false;
      order.forEach(function (name) {
        feeds[name].flush();
      });
    },
  };

  Halvo.feed = function (name) {
    if (!feeds[name]) {
      var listeners = [];
      var reader = null;
      var kept = null;
      var deliver = function (value) {
        listeners.forEach(function (fn) {
          fn(value);
        });
      };
      order.push(name);
      feeds[name] = {
        on: function (fn) {
          listeners.push(fn);
        },
        set: function (value) {
          if (holding) kept = { value: value };
          else deliver(value);
        },
        flush: function () {
          if (kept === null) return;
          var value = kept.value;
          kept = null;
          try {
            deliver(value);
          } catch (e) {
            Halvo.showError(e);
          }
        },
        source: function (fn) {
          reader = fn;
        },
        refresh: function () {
          return reader ? reader() : Promise.resolve();
        },
      };
    }
    return feeds[name];
  };

  Halvo.el = function (tag, props, children) {
    var element = document.createElement(tag);
    Object.keys(props || {}).forEach(function (key) {
      if (key === "text") element.textContent = props[key];
      else if (key === "class") element.className = props[key];
      else if (key === "for") element.htmlFor = props[key];
      else element[key] = props[key];
    });
    (children || []).forEach(function (child) {
      element.append(child);
    });
    return element;
  };

  function wordsOf(e) {
    var w = Halvo.words;
    if (!e) return "";
    if (typeof e === "object" && "error" in e) return Halvo.errorWords(e);
    if (typeof e === "string" && Object.keys(w).some(function (k) { return w[k] === e; })) return e;
    if (e instanceof Error) return w.ERROR_BROKEN;
    return w.ERROR_OTHER;
  }

  Halvo.showError = function (e) {
    var text = wordsOf(e);
    document.getElementById("error").textContent = text;
    document.getElementById("dismiss-error").classList.toggle("hidden", text === "");
  };

  Halvo.showConnection = function (text) {
    document.getElementById("connection").textContent = text || "";
  };

  var answers = 0;

  Halvo.markAnswer = function () {
    answers += 1;
  };

  Halvo.answersSoFar = function () {
    return answers;
  };

  Halvo.loadAgain = function (mark) {
    window.location.hash = mark;
    window.location.reload();
  };

  Halvo.takeLoadMark = function () {
    var mark = String(window.location.hash || "").replace(/^#/, "");
    if (mark && window.history && typeof window.history.replaceState === "function") {
      window.history.replaceState(null, "", window.location.pathname + window.location.search);
    }
    return mark;
  };

  Halvo.formatBytes = function (n) {
    var w = Halvo.words;
    if (n < 1024) return n + w.UNIT_B;
    var units = [w.UNIT_KB, w.UNIT_MB, w.UNIT_GB, w.UNIT_TB];
    var v = n / 1024;
    var i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return (v < 10 ? v.toFixed(1) : Math.round(v)) + " " + units[i];
  };

  /** The region whose form the page writes a date and a count in, whatever region the system is set to: `26 November 2026`, `1,234`. */
  var FORM = "en-GB";

  /** The day of `ms` on the clock of this computer, as the core writes a day: a day number, a month name and a year. */
  Halvo.dateOf = function (ms) {
    var local = new Date(ms - new Date(ms).getTimezoneOffset() * 60000);
    return local.toLocaleDateString(FORM, { timeZone: "UTC", day: "numeric", month: "long", year: "numeric" });
  };

  Halvo.formatCount = function (n) {
    return Number(n).toLocaleString(FORM);
  };

  function start() {
    var main = document.getElementById("views");
    var dismiss = document.getElementById("dismiss-error");
    dismiss.textContent = Halvo.words.DISMISS;
    dismiss.onclick = function () {
      Halvo.showError("");
    };
    entries
      .slice()
      .sort(function (a, b) {
        return a.order - b.order;
      })
      .forEach(function (view) {
        var heading = Halvo.el("h2", { id: view.id + "-heading", text: view.heading });
        var body = Halvo.el("div", { class: "view-body" });
        var section = Halvo.el("section", { id: view.id + "-section" }, [heading, body]);
        section.setAttribute("aria-labelledby", heading.id);
        main.appendChild(section);
        view.mount(body);
      });
    entries.forEach(function (view) {
      view.show();
    });
    Halvo.ipc
      .call("plugin:app|version")
      .then(function (version) {
        document.getElementById("version").textContent = version;
      })
      .catch(function () {});
  }

  document.addEventListener("visibilitychange", function () {
    Halvo.views.window(document.visibilityState === "visible" ? "shown" : "hidden");
  });

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else setTimeout(start, 0);
})();
