(function () {
  "use strict";
  var w = Halvo.words;
  var onboarding = Halvo.feed("onboarding");
  var SOURCE_OF = { input_monitoring: ["input"], accessibility: ["foreground"], screen_and_system_audio_recording: ["screen"] };
  var ASK_ERRORS = { unsupported_op: w.ASK_UNSUPPORTED };
  var sources = [];
  var shown = null;
  var kept = {};
  var section;
  var banner;
  var list;
  var walkback;
  var steps;
  var outcome;

  function button(text, onclick) {
    var b = Halvo.el("button", { text: text });
    b.onclick = onclick;
    return b;
  }

  function openPane(permission) {
    return function () {
      return Halvo.ipc.call("open_permission_pane", { permission: permission }).catch(Halvo.showError);
    };
  }

  function paneOf(permission) {
    var row = shown && shown.permissions.filter(function (p) {
      return p.permission === permission;
    })[0];
    return row ? row.pane_name : permission;
  }

  function askWords(e) {
    var code = e && typeof e === "object" ? e.error : null;
    return Object.prototype.hasOwnProperty.call(ASK_ERRORS, code) ? ASK_ERRORS[code] : Halvo.errorWords(e);
  }

  function ask(step) {
    return function () {
      return Halvo.ipc.call("request_permission", { permission: step.permission }).then(
        function (answer) {
          delete kept[step.permission];
          outcome.textContent = (answer === "already_granted" ? w.ASK_ALREADY_ON : w.ASK_REQUESTED).replace("{pane}", paneOf(step.permission));
          return refresh();
        },
        function (e) {
          kept[step.permission] = step;
          outcome.textContent = askWords(e);
          if (shown) render(shown);
        },
      );
    };
  }

  function putAway(permission) {
    return function () {
      return Halvo.ipc.call("acknowledge_lost_permission", { permission: permission }).then(onboarding.set, function (e) {
        outcome.textContent = Halvo.errorWords(e);
      });
    };
  }

  function withKept(o) {
    var all = o.walkback.slice();
    Object.keys(kept).forEach(function (permission) {
      var row = o.permissions.filter(function (p) {
        return p.permission === permission;
      })[0];
      if (!row || row.grant === "granted") {
        delete kept[permission];
        return;
      }
      var asks = all.some(function (s) {
        return s.permission === permission && s.action === "request_from_system";
      });
      if (asks) return;
      var at = all.findIndex(function (s) {
        return s.permission === permission;
      });
      all.splice(at < 0 ? all.length : at, 0, kept[permission]);
    });
    return all;
  }

  function render(o) {
    shown = o;
    var needed = o.permissions.length > 0;
    section.classList.toggle("hidden", !needed);
    if (!needed) return;
    if (o.lost_grants.length > 0) {
      var names = o.permissions
        .filter(function (p) {
          return p.lost;
        })
        .map(function (p) {
          return p.pane_name;
        })
        .join(", ");
      var lost = o.permissions.filter(function (p) {
        return p.lost;
      });
      var stillRunning = lost.some(function (p) {
        return (SOURCE_OF[p.permission] || []).some(function (name) {
          return sources.some(function (src) {
            return src.name === name && (src.state === "running" || src.state === "impaired");
          });
        });
      });
      banner.textContent = (stillRunning ? w.LOST_RUNNING : w.LOST_STOPPED).replace("{names}", names);
      banner.classList.remove("hidden");
    } else {
      banner.classList.add("hidden");
    }
    list.replaceChildren.apply(
      list,
      o.permissions.map(function (p) {
        var granted = p.grant === "granted";
        var badge = Halvo.el("span", {
          class: "badge " + (granted ? "granted" : "missing"),
          text: granted ? w.GRANT_ON : p.lost ? w.GRANT_LOST : p.asked ? w.GRANT_OFF : w.GRANT_NOT_ASKED,
        });
        var row = Halvo.el("div", { class: "row" }, [badge, Halvo.el("strong", { text: p.pane_name }), Halvo.el("span", { class: "muted why", text: p.why })]);
        if (p.lost) row.appendChild(button(w.LEAVE_OFF, putAway(p.permission)));
        return row;
      }),
    );
    var all = withKept(o);
    walkback.classList.toggle("hidden", all.length === 0);
    steps.replaceChildren.apply(
      steps,
      all.map(function (s) {
        var li = Halvo.el("li", { text: s.text + " " });
        if (s.action === "request_from_system") {
          li.appendChild(button(w.ASK_NOW, ask(s)));
        } else if (s.action === "open_settings_pane") {
          li.appendChild(button(w.OPEN_SETTINGS, openPane(s.permission)));
        }
        return li;
      }),
    );
  }

  function refresh() {
    return Halvo.ipc.call("onboarding_state").then(onboarding.set).catch(Halvo.showError);
  }

  Halvo.views.add({
    id: "permissions",
    order: 3,
    heading: w.PERMISSIONS_HEADING,
    mount: function (el) {
      section = el.parentNode;
      section.classList.add("hidden");
      banner = Halvo.el("div", { id: "lost-banner", class: "banner hidden" });
      list = Halvo.el("div", { id: "permissions" });
      steps = Halvo.el("ol", { id: "walkback-steps" });
      outcome = Halvo.el("p", { id: "ask-outcome", class: "muted" });
      walkback = Halvo.el("div", { id: "walkback", class: "hidden" }, [
        Halvo.el("p", { class: "muted walkback-intro", text: w.WALKBACK_INTRO }),
        steps,
      ]);
      el.append(banner, list, walkback, outcome);
      onboarding.on(render);
      Halvo.feed("status").on(function (s) {
        sources = s.sources || [];
      });
      onboarding.source(refresh);
      refresh();
      setInterval(refresh, 3000);
    },
    show: function () {},
    hide: function () {},
  });
})();
