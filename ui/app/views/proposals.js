(function () {
  "use strict";
  var w = Halvo.words;
  var categories = {
    utility_bill_payment: w.CATEGORY_UTILITY_BILL_PAYMENT,
    tenant_rent: w.CATEGORY_TENANT_RENT,
    lease_renewal_or_change: w.CATEGORY_LEASE_RENEWAL_OR_CHANGE,
    tenant_application_review: w.CATEGORY_TENANT_APPLICATION_REVIEW,
    email: w.CATEGORY_EMAIL,
    spreadsheet_fill: w.CATEGORY_SPREADSHEET_FILL,
    shop_order: w.CATEGORY_SHOP_ORDER,
    calendar_cleanup: w.CATEGORY_CALENDAR_CLEANUP,
    ride_order: w.CATEGORY_RIDE_ORDER,
    daily_chart_check: w.CATEGORY_DAILY_CHART_CHECK,
  };
  var MAX_PROPOSALS = 4;
  /** The one text of each state that has no list. */
  var withoutList = {
    not_running: w.NOT_LOOKING,
    store_unavailable: w.STORE_UNAVAILABLE,
    deleting: w.STORE_DELETING,
    delete_unfinished: w.STORE_DELETE_UNFINISHED,
  };
  var stateLine;
  var nothingFound;
  var looksWhen;
  var list;
  var storedLine;
  var stored;
  var byHand;
  var view;
  var searchBox;
  var searchToggle;
  var searchLine;
  /** The search for repeated tasks as the newest status says it, "on" or "off"; null beside a service that says nothing of it. */
  var search = null;
  /** The newest reply that the view drew, which a new word of the status draws again. */
  var lastReply = null;
  var turning = false;
  var rowsMade = 0;
  var reads = 0;
  /** While the newest reply reads deleting, the view reads again after this many ms. */
  var DELETING_READ_MS = 3000;
  var readAgain = null;
  /** While the window shows, the view reads again every this many ms, the period of the table of what is stored; unlike that table, never while the window is hidden. */
  var SHOWN_READ_MS = 15000;
  var reading = null;

  function known(text) {
    return typeof text === "string" && text !== "" ? text : null;
  }

  /** Each placeholder is filled in one pass, so a value is never read as a placeholder. */
  function fill(template, valueOf) {
    return template.replace(/\{([^{}]+)\}/g, function (all, name) {
      return valueOf(name);
    });
  }

  /** What was seen: the category's text, then the app and the site; a part that is not known is left out with its word. */
  function seenText(row) {
    var parts = { category: categories[row.category], app: known(row.app), site: known(row.site) };
    var template = w.SEEN_WHERE;
    if (parts.app === null) template = template.replace(/ [^ {}]+ \{app\}/, "");
    if (parts.site === null) template = template.replace(/ [^ {}]+ \{site\}/, "");
    return fill(template, function (name) {
      return parts[name];
    });
  }

  function checked(row, withCount) {
    var ok =
      row &&
      typeof row === "object" &&
      typeof row.id === "string" &&
      typeof row.category === "string" &&
      Object.prototype.hasOwnProperty.call(categories, row.category) &&
      (!withCount || (Number.isInteger(row.count) && row.count >= 0));
    if (!ok) throw new Error("workflows");
    return row;
  }

  /** A text of a row with an id of its own, which each button of the row names in aria-describedby. */
  function rowText(tag, text) {
    rowsMade += 1;
    return Halvo.el(tag, { id: "workflow-text-" + rowsMade, text: text });
  }

  function describedBy(buttons, textEl) {
    buttons.forEach(function (b) {
      b.setAttribute("aria-describedby", textEl.id);
    });
  }

  /** Reads the workflows and draws the reply, unless a later read has started since; a reply equal to the one drawn draws nothing, so an open question and the focus stay. */
  function read() {
    reads += 1;
    var mine = reads;
    clearTimeout(readAgain);
    readAgain = null;
    return Halvo.ipc
      .call("workflows")
      .then(function (reply) {
        if (mine !== reads) return;
        if (lastReply === null || JSON.stringify(reply) !== JSON.stringify(lastReply)) render(reply);
        if (reply.state === "deleting") readAgain = setTimeout(read, DELETING_READ_MS);
        Halvo.readSucceeded("proposals");
      })
      .catch(function (e) {
        Halvo.readFailed("proposals", e);
      });
  }
  /** The delete view reads the list again behind an answer of a delete that is not a finished one. */
  Halvo.readWorkflows = read;

  /** After a decision that succeeded the view reads again; after one that failed the row stays. */
  function decide(command, id, buttons) {
    buttons.forEach(function (b) {
      b.disabled = true;
    });
    return Halvo.ipc
      .call(command, { workflow: id })
      .then(read, Halvo.showError)
      .then(function () {
        buttons.forEach(function (b) {
          b.disabled = false;
        });
      });
  }

  /** The line and the button of the set-up by hand; a press that succeeded reads the list again, as a decision does. */
  function byHandOffer() {
    var line = Halvo.el("p", { id: "by-hand-line", text: w.BY_HAND_LINE });
    var setUp = Halvo.el("button", { text: w.BY_HAND_SET_UP });
    setUp.setAttribute("aria-describedby", line.id);
    setUp.onclick = function () {
      setUp.disabled = true;
      return Halvo.ipc
        .call("set_up_sheet_agent")
        .then(read, Halvo.showError)
        .then(function () {
          setUp.disabled = false;
        });
    };
    return [line, setUp];
  }

  /** A delete that is held back while an agent uses the workflow has a sentence of its own. */
  function notDeleted(e) {
    Halvo.showError(e && e.error === "in_use" ? w.WORKFLOW_IN_USE : e);
  }

  /** A proposal whose task this version cannot do (the row's cannot_do) says so and offers Not now alone. */
  function proposalRow(row) {
    var seen = rowText("p", seenText(row));
    var count = fill(w.SEEN_COUNT, function () {
      return String(row.count);
    });
    var cannot = row.cannot_do === true;
    var approve = Halvo.el("button", { class: "primary", text: w.APPROVE });
    var notNow = Halvo.el("button", { text: w.NOT_NOW });
    var buttons = cannot ? [notNow] : [approve, notNow];
    describedBy(buttons, seen);
    approve.onclick = function () {
      return decide("approve_workflow", row.id, buttons);
    };
    notNow.onclick = function () {
      return decide("dismiss_workflow", row.id, buttons);
    };
    var parts = [seen, Halvo.el("p", { class: "muted", text: count })];
    if (cannot) parts.push(Halvo.el("p", { text: w.CANNOT_DO_TASK }));
    parts.push(Halvo.el("div", { class: "row" }, buttons));
    return Halvo.el("div", { class: "proposal" }, parts);
  }

  /** Delete opens the question on a row below its own and stays where it is, so a second click lands on Delete again. */
  function storedRow(row) {
    var seen = rowText("span", seenText(row));
    var start = Halvo.el("button", { text: w.DELETE_WORKFLOW });
    var yes = Halvo.el("button", { class: "danger", text: w.DELETE_WORKFLOW_YES });
    var cancel = Halvo.el("button", { text: w.CANCEL });
    describedBy([start, yes, cancel], seen);
    var confirmRow = Halvo.el("div", { class: "confirm-row" }, [
      Halvo.el("p", { text: w.DELETE_WORKFLOW_QUESTION }),
      Halvo.el("div", { class: "row" }, [yes, cancel]),
    ]);
    confirmRow.classList.add("hidden");
    start.onclick = function () {
      confirmRow.classList.remove("hidden");
    };
    cancel.onclick = function () {
      confirmRow.classList.add("hidden");
    };
    yes.onclick = function () {
      confirmRow.classList.add("hidden");
      return Halvo.ipc.call("delete_workflow", { workflow: row.id }).then(read, notDeleted);
    };
    return Halvo.el("div", { class: "stored-workflow" }, [Halvo.el("div", { class: "row" }, [seen, start]), confirmRow]);
  }

  /** One turn of the switch: one set_search, whose answer the status takes; a refusal shows its sentence and turns nothing. */
  function onSearchTurn(event) {
    if (turning || search === null) return Promise.resolve();
    if (event && event.detail > 1) return Promise.resolve();
    turning = true;
    searchToggle.disabled = true;
    Halvo.showError("");
    return Halvo.ipc
      .call("set_search", { on: search !== "on" })
      .then(function (answer) {
        Halvo.takeMutationReply(answer);
      })
      .catch(Halvo.showError)
      .then(function () {
        turning = false;
        searchToggle.disabled = false;
      });
  }

  function onStatus(status) {
    var said = status && (status.search === "on" || status.search === "off") ? status.search : null;
    if (said === search) return;
    search = said;
    if (lastReply !== null) render(lastReply);
  }

  function clear() {
    searchBox.remove();
    [stateLine, nothingFound, looksWhen, storedLine].forEach(function (el) {
      el.classList.add("hidden");
    });
    list.replaceChildren();
    stored.replaceChildren();
    byHand.replaceChildren();
  }

  function render(reply) {
    clear();
    lastReply = null;
    if (reply && Object.prototype.hasOwnProperty.call(withoutList, reply.state)) {
      stateLine.textContent = withoutList[reply.state];
      stateLine.classList.remove("hidden");
      lastReply = reply;
      return;
    }
    if (!reply || reply.state !== "running" || !Array.isArray(reply.proposals) || !Array.isArray(reply.stored)) {
      throw new Error("workflows");
    }
    var proposals = reply.proposals.slice(0, MAX_PROPOSALS).map(function (row) {
      return checked(row, true);
    });
    var all = reply.stored.map(function (row) {
      return checked(row, false);
    });
    var setUpByHand = all.filter(function (row) {
      return row.by_hand === true;
    });
    var noticed = proposals.length > 0 || all.length > setUpByHand.length;
    if (search === null) {
      nothingFound.classList.toggle("hidden", noticed);
      looksWhen.classList.toggle("hidden", noticed);
    } else {
      searchToggle.setAttribute("aria-checked", search === "on" ? "true" : "false");
      searchLine.textContent = search === "on" ? w.LOOKS_WHEN : noticed ? w.SEARCH_OFF_KEPT : w.SEARCH_OFF;
      view.insertBefore(searchBox, stateLine);
      nothingFound.classList.toggle("hidden", noticed || search !== "on");
    }
    list.replaceChildren.apply(list, proposals.map(proposalRow));
    stored.replaceChildren.apply(stored, all.map(storedRow));
    storedLine.classList.toggle("hidden", all.length === 0);
    var parts = setUpByHand.length === 0 ? byHandOffer() : [Halvo.el("p", { text: w.BY_HAND_HELD })];
    byHand.replaceChildren.apply(byHand, parts);
    lastReply = reply;
  }

  Halvo.views.add({
    id: "proposals",
    order: 40,
    heading: w.NOTICED_HEADING,
    mount: function (el) {
      stateLine = Halvo.el("p", { id: "workflows-state", class: "muted" });
      nothingFound = Halvo.el("p", { id: "workflows-nothing-found", class: "muted", text: w.NOTHING_FOUND });
      looksWhen = Halvo.el("p", { id: "workflows-looks-when", class: "muted", text: w.LOOKS_WHEN });
      list = Halvo.el("div", { id: "proposals-list" });
      storedLine = Halvo.el("p", { id: "stored-workflows-line", text: w.STORED_WORKFLOWS });
      stored = Halvo.el("div", { id: "stored-workflows" });
      byHand = Halvo.el("div", { id: "by-hand" });
      searchToggle = Halvo.el("button", { id: "look-for-tasks", role: "switch", text: w.LOOK_FOR_TASKS });
      searchToggle.onclick = onSearchTurn;
      searchLine = Halvo.el("p", { id: "search-line", class: "muted" });
      searchToggle.setAttribute("aria-describedby", searchLine.id);
      searchBox = Halvo.el("div", { id: "search-control" }, [searchToggle, searchLine]);
      view = el;
      el.append(stateLine, nothingFound, looksWhen, list, storedLine, stored, byHand);
      Halvo.feed("status").on(onStatus);
      clear();
    },
    show: function () {
      read();
      if (reading === null && document.visibilityState !== "hidden") reading = setInterval(read, SHOWN_READ_MS);
    },
    hide: function () {
      clearInterval(reading);
      reading = null;
    },
  });
})();
