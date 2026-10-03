"""Tests for Vida's web server (server/).

Most of them start a real server on this computer, with a copy of Vida in
a temporary folder (with the settings and species the characterization
tests use), and run small simulations through it, as the viewer does.

They are skipped if FastAPI and uvicorn aren't installed
(pip install -r requirements-server.txt).
"""

import json
import shutil
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

import pytest

pytest.importorskip("fastapi")
uvicorn = pytest.importorskip("uvicorn")

from server import runs as serverRuns  # noqa: E402
from server.app import makeApp  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
INPUTS = REPO / "tests" / "characterization" / "inputs"

# the server is on this computer: never go through a proxy to reach it
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))

# a small run: a 20 m world with 30 seeds, cycles 0 to 6
SMALL = {"worldSize": 20, "seeds": 30, "cycles": 6, "rngStart": 7}
SMALL_OPTIONS = ["-w", "20", "-t", "6", "-s", "30", "-rngstart", "7"]
# a run that would go on for a long time
LONG = {"worldSize": 20, "seeds": 30, "cycles": 5000, "rngStart": 7}

ENDED = ("finished", "stopped", "failed")


def makeVidaFolder(folder):
    # a copy of Vida to run: the code, with the frozen settings and species
    # the characterization tests use, and a viewer page
    shutil.copy2(REPO / "Vida.py", folder / "Vida.py")
    shutil.copytree(REPO / "Vida_Data", folder / "Vida_Data", ignore=shutil.ignore_patterns("__pycache__"))
    for name in ["Vida.ini", "Vida World Preferences.yml"]:
        shutil.copy2(INPUTS / name, folder / name)
    (folder / "Species").mkdir()
    for path in (INPUTS / "species" / "generic").glob("*.yml"):
        shutil.copy2(path, folder / "Species" / path.name)
    shutil.copytree(INPUTS / "events", folder / "Event_Files")
    shutil.copytree(INPUTS / "placement", folder / "Placement_Files")
    (folder / "viewer").mkdir()
    shutil.copy2(REPO / "viewer" / "index.html", folder / "viewer" / "index.html")
    return folder


class LiveServer:
    """The web server, running in a thread, on a port of its own."""

    def __init__(self, folder):
        self.folder = folder
        self.server = uvicorn.Server(uvicorn.Config(makeApp(folder, maxRunning=4), log_level="warning"))
        self.socket = socket.socket()
        self.socket.bind(("127.0.0.1", 0))
        self.url = "http://127.0.0.1:%d" % self.socket.getsockname()[1]
        self.thread = threading.Thread(target=self.serve, daemon=True)

    def serve(self):
        self.server.run(sockets=[self.socket])

    def start(self):
        self.thread.start()
        deadline = time.time() + 30
        while not self.server.started:
            assert time.time() < deadline, "the server didn't start"
            time.sleep(0.05)

    def stop(self):
        self.server.should_exit = True
        self.thread.join(30)


@pytest.fixture(scope="module")
def vida(tmp_path_factory):
    server = LiveServer(makeVidaFolder(tmp_path_factory.mktemp("vida")))
    server.start()
    yield server
    server.stop()


def call(server, method, path, body=None):
    # (status, answer) for one request to the API
    data = None
    headers = {}
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(server.url + "/api/" + path, data=data, method=method, headers=headers)
    try:
        with OPENER.open(request, timeout=60) as response:
            return response.status, answerOf(response.read(), response.headers.get("Content-Type", ""))
    except urllib.error.HTTPError as error:
        return error.code, answerOf(error.read(), error.headers.get("Content-Type", ""))


def answerOf(data, contentType):
    if "application/json" in contentType and data:
        return json.loads(data)
    return data.decode("utf-8")


def start(server, settings):
    status, run = call(server, "POST", "runs", settings)
    assert status == 201, run
    return run


def waitFor(server, name, states=None, cycles=None, seconds=120):
    # the run, once it is in one of these states (and has done this many cycles)
    deadline = time.time() + seconds
    while True:
        status, run = call(server, "GET", "runs/" + name)
        assert status == 200
        if (states is None or run["state"] in states) and (cycles is None or run["cyclesDone"] >= cycles):
            return run
        assert time.time() < deadline, "waited too long: %s" % run
        time.sleep(0.1)


def readEvents(server, path):
    # the server-sent events of a stream, up to its "end" event
    events = []
    event = {}
    with OPENER.open(server.url + "/api/" + path, timeout=120) as response:
        for rawLine in response:
            line = rawLine.decode("utf-8").rstrip("\n")
            if line == "":
                if event:
                    events.append(event)
                    if event.get("event") == "end":
                        break
                event = {}
            elif not line.startswith(":"):
                key, separator, value = line.partition(": ")
                event[key] = value
    return events


# ---------------------------------------------------------------------------
# Settings, without a server
# ---------------------------------------------------------------------------


def test_settings_become_vidas_options(tmp_path):
    (tmp_path / "Event_Files").mkdir()
    (tmp_path / "Event_Files" / "flood.yml").write_text("")
    settings = serverRuns.RunSettings(name="dry", worldSize=50, cycles=20, seeds=100, placement="hex", rngStart=3,
                                      shade="sunmap", eventFile="Event_Files/flood.yml", saveData="n",
                                      graphics=["bs", "glb"], extra=["-iwaterstyle", "none"])
    assert serverRuns.commandOptions(settings, tmp_path) == [
        "-w", "50", "-t", "20", "-sh", "100", "-rngstart", "3", "-shade", "sunmap", "-e", "Event_Files/flood.yml",
        "-f", "n", "-g", "bs", "glb", "-iwaterstyle", "none"]
    # nothing given: everything from Vida.ini
    assert serverRuns.commandOptions(serverRuns.RunSettings(), tmp_path) == []


def test_files_must_be_in_vidas_folder(tmp_path):
    (tmp_path / "vida").mkdir()
    (tmp_path / "secret.yml").write_text("")
    for path in ["../secret.yml", "Event_Files/missing.yml"]:
        with pytest.raises(serverRuns.SettingsError):
            serverRuns.commandOptions(serverRuns.RunSettings(eventFile=path), tmp_path / "vida")


def test_the_server_sets_some_options_itself(tmp_path):
    for option in ["-n", "-j", "-control", "-x"]:
        with pytest.raises(serverRuns.SettingsError):
            serverRuns.commandOptions(serverRuns.RunSettings(extra=[option, "2"]), tmp_path)


def test_names_are_made_safe_and_never_reused(tmp_path):
    runs = serverRuns.RunList(tmp_path)
    assert runs.uniqueName("my forest/../x") == "my-forest-..-x"
    assert runs.uniqueName("") == "web"
    (tmp_path / "Output-forest").mkdir()
    assert runs.uniqueName("forest") == "forest-2"
    (tmp_path / "Output-forest-2.log").write_text("")
    assert runs.uniqueName("forest") == "forest-3"


# ---------------------------------------------------------------------------
# Through the server
# ---------------------------------------------------------------------------


def test_health_choices_and_the_viewer(vida):
    status, health = call(vida, "GET", "health")
    assert status == 200
    assert health["server"] == "vida"
    assert health["vida"]
    status, choices = call(vida, "GET", "choices")
    assert choices["species"] == ["Generic Angiosperm", "Generic Gymnosperm"]
    assert "Event_Files/species_event.yml" in choices["eventFiles"]
    assert "Placement_Files/mixed.csv" in choices["placementFiles"]
    assert choices["defaults"]["theWorldSize"]
    with OPENER.open(vida.url + "/", timeout=30) as response:
        assert b"Vida viewer" in response.read()


def test_a_run_gives_the_same_results_as_the_command_line(vida):
    run = start(vida, dict(SMALL, name="same"))
    assert run["name"] == "same"
    assert run["command"].endswith("-j -control Output-same.control")
    run = waitFor(vida, "same", ENDED)
    assert run["state"] == "finished"
    assert run["exitCode"] == 0
    assert run["cyclesDone"] == 7
    # the control file is gone; the log stays, next to the output folder
    assert not (vida.folder / "Output-same.control").exists()
    assert "Simulation Complete" in (vida.folder / "Output-same.log").read_text()

    subprocess.run([sys.executable, "Vida.py", "-n", "cli"] + SMALL_OPTIONS + ["-j"], cwd=vida.folder,
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT)
    fromServer = (vida.folder / "Output-same" / "viewer.jsonl").read_text().splitlines()
    fromCommandLine = (vida.folder / "Output-cli" / "viewer.jsonl").read_text().splitlines()
    assert len(fromServer) == 8
    headerFromServer = json.loads(fromServer[0])
    headerFromCommandLine = json.loads(fromCommandLine[0])
    assert headerFromServer.pop("name") == "same"
    assert headerFromCommandLine.pop("name") == "cli"
    assert headerFromServer == headerFromCommandLine
    assert fromServer[1:] == fromCommandLine[1:]


def test_pause_step_and_resume(vida):
    run = start(vida, dict(SMALL, name="steps", paused=True))
    assert run["state"] == "paused"
    assert run["pauseAt"] == 0
    time.sleep(1.0)
    assert waitFor(vida, "steps")["cyclesDone"] == 0

    status, run = call(vida, "POST", "runs/steps/step?cycles=2")
    assert status == 200
    assert run["pauseAt"] == 2
    run = waitFor(vida, "steps", ["paused"], cycles=2)
    assert run["cyclesDone"] == 2
    # it really waits
    time.sleep(1.0)
    assert waitFor(vida, "steps")["cyclesDone"] == 2
    # pausing a paused run doesn't let another cycle run
    status, run = call(vida, "POST", "runs/steps/pause")
    assert run["pauseAt"] == 2

    status, run = call(vida, "POST", "runs/steps/resume")
    assert run["pauseAt"] is None
    run = waitFor(vida, "steps", ENDED)
    assert run["state"] == "finished"
    assert run["cyclesDone"] == 7


def test_stopping_a_run(vida):
    start(vida, dict(LONG, name="stopped"))
    waitFor(vida, "stopped", cycles=2)
    status, run = call(vida, "POST", "runs/stopped/stop")
    assert status == 200
    assert run["state"] in ("stopping", "stopped")
    run = waitFor(vida, "stopped", ENDED)
    assert run["state"] == "stopped"
    # Vida ended the run itself, and saved what it saves at the end
    assert run["exitCode"] == 0
    assert "Stopped before cycle" in (vida.folder / "Output-stopped.log").read_text()
    for line in (vida.folder / "Output-stopped" / "viewer.jsonl").read_text().splitlines():
        json.loads(line)
    # a run that has ended can't be paused or stopped again
    status, run = call(vida, "POST", "runs/stopped/pause")
    assert run["state"] == "stopped"


def test_stopping_a_run_now(vida):
    start(vida, dict(LONG, name="ended"))
    waitFor(vida, "ended", cycles=1)
    status, run = call(vida, "POST", "runs/ended/stop?now=true")
    run = waitFor(vida, "ended", ENDED)
    assert run["state"] == "stopped"
    assert run["exitCode"] != 0


def test_the_stream_of_cycles(vida):
    start(vida, dict(SMALL, name="stream"))
    events = readEvents(vida, "runs/stream/stream")
    lines = []
    for event in events:
        if event["event"] in ("header", "cycle"):
            assert int(event["id"]) == len(lines)
            lines.append(event["data"])
    assert json.loads(lines[0])["format"] == "vida-viewer"
    assert len(lines) == 8
    assert events[-1]["event"] == "end"
    assert json.loads(events[-1]["data"])["state"] == "finished"
    # the same as the file
    assert lines == (vida.folder / "Output-stream" / "viewer.jsonl").read_text().splitlines()
    # starting part way, as a browser does when it reconnects
    later = readEvents(vida, "runs/stream/stream?from=3")
    assert later[0]["event"] == "cycle"
    assert later[0]["id"] == "3"


def test_files_logs_and_outputs(vida):
    start(vida, dict(SMALL, name="files"))
    waitFor(vida, "files", ENDED)
    saved = (vida.folder / "Output-files" / "viewer.jsonl").read_text()
    status, text = call(vida, "GET", "runs/files/viewer")
    assert text == saved
    status, log = call(vida, "GET", "runs/files/log?lines=10")
    assert "Simulation Complete" in log
    assert len(log.splitlines()) == 10

    status, outputs = call(vida, "GET", "outputs")
    entries = {}
    for entry in outputs:
        entries[entry["folder"]] = entry
    assert entries["Output-files"]["viewer"] == "api/outputs/Output-files/viewer.jsonl"
    status, text = call(vida, "GET", "outputs/Output-files/viewer.jsonl")
    assert text == saved
    status, listing = call(vida, "GET", "outputs/Output-files")
    paths = []
    for file in listing["files"]:
        paths.append(file["path"])
    assert "viewer.jsonl" in paths
    assert "CLI_arguments.txt" in paths
    # nothing outside the Output folders
    assert call(vida, "GET", "outputs/Vida_Data")[0] == 404
    assert call(vida, "GET", "outputs/Output-files/..%2F..%2FVida.ini")[0] == 404
    assert call(vida, "GET", "outputs/Output-files/%2E%2E/Vida.ini")[0] == 404


def test_forgetting_runs_and_runs_that_dont_exist(vida):
    assert call(vida, "GET", "runs/nothing")[0] == 404
    assert call(vida, "POST", "runs/nothing/pause")[0] == 404
    start(vida, dict(SMALL, name="forget", paused=True))
    # still going: it has to be stopped first
    assert call(vida, "DELETE", "runs/forget")[0] == 409
    call(vida, "POST", "runs/forget/stop")
    waitFor(vida, "forget", ENDED)
    assert call(vida, "DELETE", "runs/forget")[0] == 204
    assert call(vida, "GET", "runs/forget")[0] == 404
    # its files stay
    assert (vida.folder / "Output-forget.log").exists()


def test_settings_that_cant_be_run(vida):
    status, answer = call(vida, "POST", "runs", {"worldSize": -5})
    assert status == 422
    status, answer = call(vida, "POST", "runs", {"eventFile": "Event_Files/missing.yml"})
    assert status == 400
    assert "missing.yml" in answer["detail"]
    status, answer = call(vida, "POST", "runs", {"extra": ["-x", "3"]})
    assert status == 400


def test_a_run_that_fails(vida):
    # an event file that isn't YAML
    (vida.folder / "Event_Files" / "broken.yml").write_text("{: [")
    start(vida, dict(SMALL, name="broken", eventFile="Event_Files/broken.yml"))
    run = waitFor(vida, "broken", ENDED)
    assert run["state"] == "failed"
    assert run["exitCode"] != 0
    assert "Traceback" in call(vida, "GET", "runs/broken/log")[1]


def test_only_so_many_runs_at_once(tmp_path):
    runs = serverRuns.RunList(makeVidaFolder(tmp_path), maxRunning=1)
    first = runs.start(serverRuns.RunSettings(name="first", paused=True, **SMALL))
    try:
        with pytest.raises(serverRuns.SettingsError):
            runs.start(serverRuns.RunSettings(name="second", **SMALL))
    finally:
        runs.stopAll()
    assert first.state() == "stopped"
