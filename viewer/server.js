// Vida viewer: the Simulations panel, shown when the page comes from Vida's
// web server (python -m server; see server/README.md). From it you can
// start runs, pause them, run them a cycle at a time and stop them, and
// watch a run in the viewer while it runs, each cycle shown as soon as Vida
// has saved it.
//
// It talks to the server's API (the addresses starting api/) and uses the
// viewer's own functions from viewer.js. Opened any other way (from the
// disk, or from another web server) there is no api/, so the panel stays
// hidden and the viewer works as before.
//
// The main pieces, in order:
//   talking to the server   callServer, and finding the server
//   the form                filling it in from Vida.ini, and starting a run
//   the list of runs        their state, and the buttons to control them
//   watching a run          a stream of its cycles, added to the viewer as they come
//   saved simulations       Output folders that can be opened

"use strict";

var serverChoices = null;    // api/choices: Vida.ini's defaults and the input files
var serverRuns = [];         // api/runs: every run the server has started, newest first
var runsTimer = null;        // set while a run is going, to keep the list up to date
var runsShape = "";          // the runs and their states when the list was last built
var openLogs = {};           // the runs whose log is open: their last lines
var liveSource = null;       // the stream of the run being watched (an EventSource)
var liveName = null;         // its name
var liveRun = null;          // the run built from the stream (shown once it has a cycle)
var liveStatus = null;       // its last state, from the server
var liveDrawWaiting = false; // a redraw is waiting for the next frame

var RUNS_SECONDS = 1.5;      // how often the list is updated while a run is going
var LOG_LINES = 40;          // how much of a run's log is shown
var LIVE_STATES = ["starting", "running", "paused", "stopping"];

// The words shown for each state, with a sign so it isn't told by colour alone.
var STATE_WORDS = {
  starting: "… starting",
  running: "▶ running",
  paused: "❚❚ paused",
  stopping: "… stopping",
  finished: "✓ finished",
  stopped: "■ stopped",
  failed: "✕ failed",
};

// ---------------------------------------------------------------------------
// Talking to the server
// ---------------------------------------------------------------------------

async function callServer(method, path, body) {
  // Ask the server something. Gives back what it answered (JSON or text),
  // or throws an Error saying what went wrong.
  var options = { method: method, headers: {} };
  if (body !== undefined) {
    options.headers["Content-Type"] = "application/json";
    options.body = JSON.stringify(body);
  }
  var response = await fetch("api/" + path, options);
  if (!response.ok) {
    var problem = response.status + " " + response.statusText;
    try {
      problem = problemText((await response.json()).detail) || problem;
    } catch (error) {
      // not JSON: keep the status
    }
    throw new Error(problem);
  }
  if (response.status === 204) {
    return null;
  }
  var type = response.headers.get("Content-Type") || "";
  if (type.indexOf("application/json") >= 0) {
    return await response.json();
  }
  return await response.text();
}

function problemText(detail) {
  // The server's explanation of a problem. FastAPI gives a list for settings
  // it couldn't accept: [{loc: [..., "worldSize"], msg: "..."}, ...]
  if (typeof detail === "string") {
    return detail;
  }
  if (Array.isArray(detail)) {
    var parts = [];
    for (var i = 0; i < detail.length; i++) {
      var where = detail[i].loc ? detail[i].loc[detail[i].loc.length - 1] : "";
      parts.push(where + ": " + detail[i].msg);
    }
    return parts.join("; ");
  }
  return "";
}

function runPath(name) {
  return "runs/" + encodeURIComponent(name);
}

function isLive(described) {
  return LIVE_STATES.indexOf(described.state) >= 0;
}

async function findServer() {
  // Show the panel if the page came from Vida's web server.
  var info = null;
  try {
    info = await callServer("GET", "health");
  } catch (error) {
    return;
  }
  if (!info || info.server !== "vida") {
    return;
  }
  document.getElementById("server-panel").hidden = false;
  document.getElementById("welcome").hidden = true;
  document.getElementById("server-note").textContent = "Vida " + info.vida + ", in " + info.folder;
  try {
    serverChoices = await callServer("GET", "choices");
    fillRunForm(serverChoices);
  } catch (error) {
    showFormMessage("Could not read Vida's settings: " + error.message);
  }
  await refreshRuns();
  await refreshSaved();
  if (serverRuns.length === 0) {
    document.getElementById("new-run").open = true;
  }
  // index.html?run=name watches that run straight away
  var name = new URLSearchParams(window.location.search).get("run");
  if (name) {
    watchRun(name);
  }
}

// ---------------------------------------------------------------------------
// The form
// ---------------------------------------------------------------------------

function fillSelect(select, firstText, values) {
  // a menu: firstText (meaning "none", sent as nothing), then the values
  select.replaceChildren(makeElement("option", "", firstText));
  select.firstChild.value = "";
  for (var i = 0; i < values.length; i++) {
    var option = makeElement("option", "", values[i]);
    option.value = values[i];
    select.appendChild(option);
  }
}

function fillRunForm(choices) {
  // Vida.ini's defaults go in the boxes as grey hints: a box left empty
  // uses them, as on the command line.
  var form = document.getElementById("run-form");
  var defaults = choices.defaults;
  var hints = { worldSize: "theWorldSize", seeds: "startPopulationSize", cycles: "maxCycles",
    maxPopulation: "maxPopulation", waterLevel: "waterLevel" };
  for (var name in hints) {
    if (defaults[hints[name]] !== undefined) {
      form.elements[name].placeholder = defaults[hints[name]] + " (Vida.ini)";
    }
  }
  fillSelect(form.elements.placementFile, "no: plant the seeds above", choices.placementFiles);
  fillSelect(form.elements.eventFile, "none", choices.eventFiles);
  fillSelect(form.elements.terrain, "none: flat ground", choices.terrainFiles);
  fillSelect(form.elements.shade, (defaults.shadingModel || "classic") + " (Vida.ini)", ["classic", "sunmap"]);
  var note = "Vida grows every species whose .yml file is in the Species folder";
  if (choices.species.length > 0) {
    note += ": " + choices.species.join(", ");
  }
  document.getElementById("species-note").textContent = note + ".";
}

function showFormMessage(text) {
  document.getElementById("run-form-message").textContent = text;
}

function formSettings(form) {
  // What the form says, as the settings the server takes (see RunSettings
  // in server/runs.py). Empty boxes are left out.
  var settings = { name: form.elements.name.value.trim() || "web", placement: form.elements.placement.value,
    paused: form.elements.paused.checked, extra: [] };
  var numbers = ["worldSize", "seeds", "cycles", "maxPopulation", "rngStart", "waterLevel"];
  for (var i = 0; i < numbers.length; i++) {
    var value = form.elements[numbers[i]].value.trim();
    if (value !== "") {
      settings[numbers[i]] = Number(value);
    }
  }
  var choices = ["placementFile", "eventFile", "terrain", "shade"];
  for (var j = 0; j < choices.length; j++) {
    if (form.elements[choices[j]].value !== "") {
      settings[choices[j]] = form.elements[choices[j]].value;
    }
  }
  var extra = form.elements.extra.value.trim();
  if (extra !== "") {
    settings.extra = extra.split(/\s+/);
  }
  return settings;
}

async function onStartRun(event) {
  event.preventDefault();
  var form = event.target;
  var button = form.querySelector("button[type=submit]");
  button.disabled = true;
  showFormMessage("Starting…");
  try {
    var started = await callServer("POST", "runs", formSettings(form));
    showFormMessage("Started " + started.name + ".");
    // fold the form away, so the viewer is in sight
    document.getElementById("new-run").open = false;
    await refreshRuns();
    watchRun(started.name);
  } catch (error) {
    showFormMessage("Could not start it: " + error.message);
  }
  button.disabled = false;
}

// ---------------------------------------------------------------------------
// The list of runs
// ---------------------------------------------------------------------------

async function refreshRuns() {
  // Ask the server for every run, show them, and look again soon if any is going.
  if (runsTimer !== null) {
    window.clearTimeout(runsTimer);
    runsTimer = null;
  }
  try {
    serverRuns = await callServer("GET", "runs");
  } catch (error) {
    return;
  }
  var names = Object.keys(openLogs);
  for (var i = 0; i < names.length; i++) {
    await refreshLog(names[i]);
  }
  drawRuns();
  for (var r = 0; r < serverRuns.length; r++) {
    if (isLive(serverRuns[r])) {
      runsTimer = window.setTimeout(refreshRuns, RUNS_SECONDS * 1000);
      break;
    }
  }
}

async function refreshLog(name) {
  try {
    openLogs[name] = await callServer("GET", runPath(name) + "/log?lines=" + LOG_LINES);
  } catch (error) {
    openLogs[name] = "(" + error.message + ")";
  }
}

function stateBadge(state) {
  var badge = makeElement("span", "state-badge state-" + state, STATE_WORDS[state] || state);
  return badge;
}

function progressText(described) {
  // "12 of 101 cycles, 3.2 s"
  var text = described.cyclesDone.toLocaleString();
  if (described.lastCycle !== null) {
    text += " of " + (described.lastCycle + 1).toLocaleString();
  }
  text += described.cyclesDone === 1 ? " cycle" : " cycles";
  if (described.seconds !== null) {
    text += ", " + described.seconds.toFixed(1) + " s";
  }
  return text;
}

function runButton(text, name, action) {
  var button = makeElement("button", "", text);
  button.type = "button";
  button.dataset.run = name;
  button.dataset.action = action;
  return button;
}

function runRow(described) {
  // one run in the list: its name, state, how far it has got, and buttons
  var row = makeElement("li", "run-row");
  row.dataset.run = described.name;
  if (described.name === liveName) {
    row.classList.add("watching");
  }
  var title = makeElement("div", "run-title");
  title.appendChild(makeElement("span", "run-name", described.name));
  title.appendChild(stateBadge(described.state));
  row.appendChild(title);

  var progress = makeElement("div", "run-progress");
  var bar = document.createElement("progress");
  bar.max = described.lastCycle === null ? 1 : described.lastCycle + 1;
  bar.value = described.cyclesDone;
  bar.setAttribute("aria-label", "Cycles done");
  progress.appendChild(bar);
  progress.appendChild(makeElement("span", "run-count", progressText(described)));
  row.appendChild(progress);

  var buttons = makeElement("div", "run-buttons");
  if (described.name === liveName) {
    buttons.appendChild(runButton("Watching", described.name, "watch"));
    buttons.lastChild.disabled = true;
  } else {
    buttons.appendChild(runButton("Watch", described.name, "watch"));
  }
  if (isLive(described) && described.state !== "stopping") {
    if (described.state === "paused") {
      buttons.appendChild(runButton("Resume", described.name, "resume"));
    } else {
      buttons.appendChild(runButton("Pause", described.name, "pause"));
    }
    buttons.appendChild(runButton("Step", described.name, "step"));
    buttons.appendChild(runButton("Stop", described.name, "stop"));
  }
  buttons.appendChild(runButton(openLogs[described.name] !== undefined ? "Hide log" : "Log", described.name, "log"));
  if (!isLive(described)) {
    buttons.appendChild(runButton("Forget", described.name, "forget"));
  }
  row.appendChild(buttons);

  var command = makeElement("div", "run-command note", described.command);
  row.appendChild(command);
  if (openLogs[described.name] !== undefined) {
    row.appendChild(makeElement("pre", "run-log", openLogs[described.name]));
  }
  return row;
}

function showLogEnd(list) {
  // logs open at their newest lines
  var logs = list.querySelectorAll(".run-log");
  for (var i = 0; i < logs.length; i++) {
    logs[i].scrollTop = logs[i].scrollHeight;
  }
}

function drawRuns() {
  // Build the list again only if a run has come, gone or changed state (so
  // a button isn't replaced while it is being pressed); otherwise just move
  // the progress bars on.
  var list = document.getElementById("runs-list");
  document.getElementById("runs-empty").hidden = serverRuns.length > 0;
  var shape = [];
  for (var i = 0; i < serverRuns.length; i++) {
    shape.push(serverRuns[i].name + ":" + serverRuns[i].state);
  }
  shape.push("watching:" + liveName, "logs:" + Object.keys(openLogs).join(","));
  shape = shape.join("|");
  if (shape !== runsShape) {
    runsShape = shape;
    list.replaceChildren();
    for (var r = 0; r < serverRuns.length; r++) {
      list.appendChild(runRow(serverRuns[r]));
    }
    showLogEnd(list);
    return;
  }
  for (var j = 0; j < serverRuns.length; j++) {
    var row = list.children[j];
    row.querySelector("progress").value = serverRuns[j].cyclesDone;
    row.querySelector(".run-count").textContent = progressText(serverRuns[j]);
    var log = row.querySelector(".run-log");
    if (log && log.textContent !== openLogs[serverRuns[j].name]) {
      log.textContent = openLogs[serverRuns[j].name];
      log.scrollTop = log.scrollHeight;
    }
  }
}

function updateRunInList(described) {
  // a run's new state (from its stream), shown without asking the server
  for (var i = 0; i < serverRuns.length; i++) {
    if (serverRuns[i].name === described.name) {
      serverRuns[i] = described;
      drawRuns();
      return;
    }
  }
}

async function controlRun(name, action) {
  // pause, resume, step or stop a run (from the list or the viewer's buttons)
  try {
    var described = await callServer("POST", runPath(name) + "/" + action);
    updateRunInList(described);
    if (name === liveName) {
      liveStatus = described;
      drawLiveBox();
    }
  } catch (error) {
    showMessage("Could not " + action + " " + name + ": " + error.message);
  }
  refreshRuns();
}

async function onRunButton(event) {
  var button = event.target.closest("button");
  if (!button || !button.dataset.action) {
    return;
  }
  var name = button.dataset.run;
  var action = button.dataset.action;
  if (action === "watch") {
    watchRun(name);
  } else if (action === "log") {
    if (openLogs[name] !== undefined) {
      delete openLogs[name];
    } else {
      await refreshLog(name);
    }
    drawRuns();
  } else if (action === "forget") {
    try {
      await callServer("DELETE", runPath(name));
    } catch (error) {
      showMessage("Could not forget " + name + ": " + error.message);
    }
    delete openLogs[name];
    refreshRuns();
  } else {
    controlRun(name, action);
  }
}

// ---------------------------------------------------------------------------
// Watching a run as it runs
// ---------------------------------------------------------------------------

function watchRun(name) {
  // Show a run in the viewer, a cycle at a time as Vida saves them. The
  // server sends its viewer.jsonl as a stream of events: "header" (the first
  // line), "cycle" (each line after it), "status" (when its state changes)
  // and "end" (when it has ended and every line has been sent).
  stopWatching();
  liveName = name;
  showMessage("Waiting for " + name + " to save its first cycle…");
  liveSource = new EventSource("api/" + runPath(name) + "/stream");
  liveSource.addEventListener("header", onLiveHeader);
  liveSource.addEventListener("cycle", onLiveCycle);
  liveSource.addEventListener("status", onLiveStatus);
  liveSource.addEventListener("end", onLiveEnd);
  liveSource.addEventListener("error", onLiveError);
  drawRuns();
}

function stopWatching() {
  // (also called by viewer.js when a file is opened)
  if (liveSource !== null) {
    liveSource.close();
    liveSource = null;
  }
  liveName = null;
  liveRun = null;
  liveStatus = null;
  document.getElementById("live-box").hidden = true;
  drawRuns();
}

function onLiveHeader(event) {
  liveRun = startRun(JSON.parse(event.data));
  liveRun.coloursGiven = 0;
  liveRun.speciesOrder = [];
}

function giveLiveColours(theRun) {
  // While a run is growing, each species keeps the colour it gets when it is
  // first seen, the first three getting the three colours. (A finished
  // file gives the colours to the most common species instead.)
  var given = 0;
  for (var s = 0; s < theRun.coloursGiven; s++) {
    if (theRun.species[s].slot >= 0) {
      given += 1;
    }
  }
  for (var n = theRun.coloursGiven; n < theRun.species.length; n++) {
    if (given < SPECIES_SLOTS) {
      theRun.species[n].slot = given;
      given += 1;
    }
  }
  theRun.coloursGiven = theRun.species.length;
  // the species in the menus and tables: those with colours, then the rest
  var order = [];
  for (var k = 0; k < theRun.species.length; k++) {
    if (theRun.species[k].slot >= 0) {
      order.push(k);
    }
  }
  for (var m = 0; m < theRun.species.length; m++) {
    if (theRun.species[m].slot < 0) {
      order.push(m);
    }
  }
  theRun.speciesOrder = order;
}

function onLiveCycle(event) {
  if (liveRun === null) {
    return;
  }
  var speciesBefore = liveRun.species.length;
  addCycle(liveRun, JSON.parse(event.data));
  if (liveRun.species.length !== speciesBefore) {
    giveLiveColours(liveRun);
  }
  if (run !== liveRun) {
    // its first cycle: show it
    run = liveRun;
    showMessage("");
    startViewing(liveName);
    drawLiveBox();
    return;
  }
  // If the newest cycle was on show (and it isn't playing), show the new one.
  // If you have gone back to look at an earlier cycle, it stays there.
  if (playTimer === null && cycleIndex === run.cycles.length - 2) {
    cycleIndex = run.cycles.length - 1;
  }
  if (run.speciesOrder.length !== document.getElementById("highlight-select").options.length - 1) {
    fillHighlightSelect();
  }
  if (!liveDrawWaiting) {
    // draw at most once a frame, however fast the cycles come
    liveDrawWaiting = true;
    window.requestAnimationFrame(drawLive);
  }
}

function drawLive() {
  liveDrawWaiting = false;
  if (run === null || run !== liveRun) {
    return;
  }
  document.getElementById("cycle-slider").max = String(run.cycles.length - 1);
  drawEverything();
}

function onLiveStatus(event) {
  liveStatus = JSON.parse(event.data);
  updateRunInList(liveStatus);
  drawLiveBox();
}

function onLiveEnd(event) {
  // The run has ended and every cycle has come. Its cycles stay on show.
  liveStatus = JSON.parse(event.data);
  liveSource.close();
  liveSource = null;
  drawLiveBox();
  if (liveRun === null || liveRun.cycles.length === 0) {
    showMessage(liveStatus.name + " " + liveStatus.state + " before saving a cycle. Its log, in the list above, says why.");
    openLogs[liveStatus.name] = "";
  }
  refreshRuns();
  refreshSaved();
}

function onLiveError() {
  // The browser tries again by itself (carrying on from the last cycle it
  // got) unless the server said no, for example to a run it doesn't have.
  if (liveSource !== null && liveSource.readyState === EventSource.CLOSED) {
    showMessage("Could not watch " + liveName + ": the server has no run by that name, or has stopped.");
    stopWatching();
  }
}

function drawLiveBox() {
  // the run's state and its buttons, beside the viewer's slider
  var box = document.getElementById("live-box");
  if (liveStatus === null || run === null || run !== liveRun) {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  var badge = document.getElementById("live-state");
  badge.className = "state-badge state-" + liveStatus.state;
  badge.textContent = STATE_WORDS[liveStatus.state] || liveStatus.state;
  var going = isLive(liveStatus) && liveStatus.state !== "stopping";
  if (going && liveStatus.lastCycle !== null) {
    badge.textContent += ", to cycle " + liveStatus.lastCycle;
  }
  var pauseButton = document.getElementById("live-pause");
  pauseButton.hidden = !going;
  pauseButton.textContent = liveStatus.state === "paused" ? "Resume run" : "Pause run";
  document.getElementById("live-step").hidden = !going;
  document.getElementById("live-stop").hidden = !going;
}

// ---------------------------------------------------------------------------
// Saved simulations
// ---------------------------------------------------------------------------

async function refreshSaved() {
  var outputs = [];
  try {
    outputs = await callServer("GET", "outputs");
  } catch (error) {
    return;
  }
  var list = document.getElementById("saved-list");
  list.replaceChildren();
  for (var i = 0; i < outputs.length; i++) {
    if (!outputs[i].viewer) {
      continue;
    }
    var item = makeElement("li", "saved-item");
    item.appendChild(makeElement("span", "saved-name", outputs[i].folder));
    item.appendChild(makeElement("span", "note", outputs[i].modified.replace("T", " ")));
    var button = makeElement("button", "", "Open");
    button.type = "button";
    button.dataset.url = outputs[i].viewer;
    item.appendChild(button);
    list.appendChild(item);
  }
  if (list.children.length === 0) {
    list.appendChild(makeElement("li", "note", "None yet."));
  }
}

function onSavedButton(event) {
  var button = event.target.closest("button");
  if (button && button.dataset.url) {
    loadUrl(button.dataset.url);
  }
}

// ---------------------------------------------------------------------------

function setUpServerPanel() {
  document.getElementById("run-form").addEventListener("submit", onStartRun);
  document.getElementById("runs-list").addEventListener("click", onRunButton);
  document.getElementById("saved-list").addEventListener("click", onSavedButton);
  document.getElementById("saved-box").addEventListener("toggle", onSavedToggle);
  function onSavedToggle(event) {
    if (event.target.open) {
      refreshSaved();
    }
  }
  document.getElementById("live-pause").addEventListener("click", onLivePause);
  function onLivePause() {
    if (liveStatus !== null) {
      controlRun(liveStatus.name, liveStatus.state === "paused" ? "resume" : "pause");
    }
  }
  document.getElementById("live-step").addEventListener("click", onLiveStep);
  function onLiveStep() {
    if (liveStatus !== null) {
      controlRun(liveStatus.name, "step");
    }
  }
  document.getElementById("live-stop").addEventListener("click", onLiveStop);
  function onLiveStop() {
    if (liveStatus !== null) {
      controlRun(liveStatus.name, "stop");
    }
  }
  findServer();
}

setUpServerPanel();
