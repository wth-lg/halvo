(function () {
  "use strict";
  var Halvo = window.Halvo || (window.Halvo = {});
  var entries = [];
  var feeds = {};

  /**
   * Halvo.views.add({id, order, heading, mount, show, hide}) adds a view to the
   * view list. The frame renders the views as sections in ascending `order`,
   * each under a level-2 heading that holds `heading`, calls `mount(element)`
   * with the element under the heading, and then `show()`.
   * Halvo.views.window(state) calls `show()` of every view for "shown" and
   * `hide()` for any other state.
   */
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
  };

  /**
   * Halvo.feed(name) is the shared reply of one read: `set(value)` hands the
   * value to every function given to `on`, `latest()` returns the last value,
   * and `refresh()` runs the reader that the owning view gave to `source`.
   */
  Halvo.feed = function (name) {
    if (!feeds[name]) {
      var listeners = [];
      var reader = null;
      var latest = null;
      feeds[name] = {
        on: function (fn) {
          listeners.push(fn);
        },
        set: function (value) {
          latest = value;
          listeners.forEach(function (fn) {
            fn(value);
          });
        },
        latest: function () {
          return latest;
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

  /** Halvo.el(tag, props, children) makes an element with the given properties and children. */
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

  /** Halvo.showError(e) shows the text of `e` in the error line, or clears it for a falsy `e`. */
  Halvo.showError = function (e) {
    document.getElementById("error").textContent = e ? String(e) : "";
  };

  /** Halvo.formatBytes(n) is `n` bytes as text in B, KB, MB, GB or TB. */
  Halvo.formatBytes = function (n) {
    if (n < 1024) return n + " B";
    var units = ["KB", "MB", "GB", "TB"];
    var v = n / 1024;
    var i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return (v < 10 ? v.toFixed(1) : Math.round(v)) + " " + units[i];
  };

  function start() {
    var main = document.getElementById("views");
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
      .catch(Halvo.showError);
  }

  document.addEventListener("visibilitychange", function () {
    Halvo.views.window(document.visibilityState === "visible" ? "shown" : "hidden");
  });

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else setTimeout(start, 0);
})();
