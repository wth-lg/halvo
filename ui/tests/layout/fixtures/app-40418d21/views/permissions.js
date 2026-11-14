(function () {
  "use strict";
  var section;
  var banner;
  var list;
  var walkback;
  var steps;

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

  function render(o) {
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
      banner.textContent = "macOS removed Halvo's access to: " + names + ". Capture of those signals has stopped until you turn it back on below.";
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
          text: granted ? "On" : p.lost ? "Lost" : p.grant === "not_determined" ? "Not asked yet" : "Off",
        });
        var row = Halvo.el("div", { class: "row" }, [
          badge,
          Halvo.el("strong", { text: p.pane_name }),
          Halvo.el("span", { class: "muted why", text: p.why }),
        ]);
        if (!granted) row.appendChild(button("Open System Settings", openPane(p.permission)));
        return row;
      }),
    );
    walkback.classList.toggle("hidden", o.walkback.length === 0);
    steps.replaceChildren.apply(
      steps,
      o.walkback.map(function (s) {
        var li = Halvo.el("li", { text: s.text + " " });
        if (s.action === "request_from_system") {
          li.appendChild(
            button("Ask now", function () {
              return Halvo.ipc.call("request_permission", { permission: s.permission }).then(refresh).catch(Halvo.showError);
            }),
          );
        } else if (s.action === "open_settings_pane") {
          li.appendChild(button("Open System Settings", openPane(s.permission)));
        }
        return li;
      }),
    );
  }

  function refresh() {
    return Halvo.ipc.call("onboarding_state").then(render).catch(Halvo.showError);
  }

  Halvo.views.add({
    id: "permissions",
    order: 3,
    heading: "Permissions",
    mount: function (el) {
      section = el.parentNode;
      section.classList.add("hidden");
      banner = Halvo.el("div", { id: "lost-banner", class: "banner hidden" });
      list = Halvo.el("div", { id: "permissions" });
      steps = Halvo.el("ol", { id: "walkback-steps" });
      walkback = Halvo.el("div", { id: "walkback", class: "hidden" }, [
        Halvo.el("p", { class: "muted walkback-intro", text: "To get capture working again:" }),
        steps,
      ]);
      el.append(banner, list, walkback);
      refresh();
      setInterval(refresh, 3000);
    },
    show: function () {},
    hide: function () {},
  });
})();
