"""Starting Vida simulations and keeping track of them, for the web server.

Each run is its own copy of Vida (python Vida.py ...), exactly as if it had
been started from the command line, so it gives exactly the same results.
The server always adds three options of its own:
    -n         a name no other run has used, so the output folder is new
    -j         save each cycle to viewer.jsonl, which the server watches
    -control   a file the server writes to pause, step or stop the run
               (see Vida_Data/vcontrol.py)

As with tools/run_many.py, what Vida prints goes to a log file next to its
output folder (Output-<name>.log). The control file (Output-<name>.control)
is removed when the run ends.
"""

import configparser
import json
import os
import re
import shlex
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import List, Literal, Optional

from pydantic import BaseModel, Field

###Vida's own options that the server sets itself, so they can't be given
###in `extra`: -n (the name), -j and -control (how the server watches and
###controls a run) and -x (repeats, which would start the cycles again)
SERVER_OPTIONS = ("-n", "-j", "-control", "-x")

###the graphical views Vida can draw (-g)
GRAPHICAL_VIEWS = ["b", "t", "s", "ts", "st", "bs", "sb", "bt", "tb", "bts", "dxf", "glb"]

###how many bytes of a file are read at a time when looking for new lines
READ_SIZE = 8 * 1024 * 1024


class RunSettings(BaseModel):
    """What to run: Vida's options, by name. Anything left out comes from
    Vida.ini, as it does on the command line."""

    name: str = Field("web", description="Name of the simulation (-n). A number is added if it has been used.")
    worldSize: Optional[int] = Field(None, gt=0, description="Size of the world in meters (-w)")
    cycles: Optional[int] = Field(None, ge=0, description="The last cycle to run (-t)")
    maxPopulation: Optional[int] = Field(None, gt=0, description="Stop early if the population reaches this (-m)")
    seeds: Optional[int] = Field(None, ge=0, description="Seeds to start with (-s, -ss or -sh)")
    placement: Literal["random", "square", "hex"] = Field("random", description="How the starting seeds are planted")
    placementFile: Optional[str] = Field(None, description="A placement file to plant the seeds from (-sf), e.g. Placement_Files/treePattern.csv")
    rngStart: Optional[int] = Field(None, description="Starting value for the random numbers (-rngstart)")
    shade: Optional[Literal["classic", "sunmap"]] = Field(None, description="How shade is worked out (-shade)")
    shadeCell: Optional[float] = Field(None, gt=0, description="Size in meters of the squares of the sunlight map (-shadecell)")
    eventFile: Optional[str] = Field(None, description="An event file (-e), e.g. Event_Files/islands.yml")
    terrain: Optional[str] = Field(None, description="A terrain image, or a folder with one in it (-i)")
    terrainMax: Optional[float] = Field(None, description="Elevation of the brightest pixel (-imax)")
    terrainMin: Optional[float] = Field(None, description="Elevation of the darkest pixel (-imin)")
    terrainScale: Optional[float] = Field(None, description="Multiply the elevation range by this (-iscale)")
    waterLevel: Optional[float] = Field(None, description="Water level above the lowest point of the terrain (-iwater)")
    saveData: Optional[Literal["a", "e", "n", "s"]] = Field(None, description="Save data files: all cycles, end, none or start (-f)")
    archive: Optional[Literal["a", "e", "n", "s"]] = Field(None, description="Save simulation states: all cycles, end, none or start (-a)")
    graphics: Optional[List[Literal["b", "t", "s", "ts", "st", "bs", "sb", "bt", "tb", "bts", "dxf", "glb"]]] = Field(
        None, description="Graphical views to draw (-g)")
    paused: bool = Field(False, description="Start paused, before cycle 0")
    extra: List[str] = Field([], description="More of Vida's options, as on the command line, e.g. [\"-iwaterstyle\", \"none\"]")


class SettingsError(ValueError):
    """Settings that can't be run, with a message saying why."""


def cleanName(name):
    ###a name that is safe in a folder name: letters, digits, - _ and .
    name = re.sub(r"[^A-Za-z0-9._-]+", "-", name.strip()).strip("-.")
    return name[:60] or "web"


def fileInside(vidaFolder, relativePath, what):
    ###relativePath, checked to be a file or folder inside Vida's folder
    vidaFolder = Path(vidaFolder).resolve()
    path = (vidaFolder / relativePath).resolve()
    if not path.is_relative_to(vidaFolder) or not path.exists():
        raise SettingsError("No %s called '%s' in Vida's folder" % (what, relativePath))
    return str(path.relative_to(vidaFolder))


def commandOptions(settings, vidaFolder):
    """Vida's command line options for these settings (without -n, -j and
    -control, which the run adds)."""
    options = []
    if settings.worldSize is not None:
        options += ["-w", str(settings.worldSize)]
    if settings.cycles is not None:
        options += ["-t", str(settings.cycles)]
    if settings.maxPopulation is not None:
        options += ["-m", str(settings.maxPopulation)]
    if settings.placementFile:
        options += ["-sf", fileInside(vidaFolder, settings.placementFile, "placement file")]
    elif settings.seeds is not None:
        placementOption = {"random": "-s", "square": "-ss", "hex": "-sh"}[settings.placement]
        options += [placementOption, str(settings.seeds)]
    if settings.rngStart is not None:
        options += ["-rngstart", str(settings.rngStart)]
    if settings.shade is not None:
        options += ["-shade", settings.shade]
    if settings.shadeCell is not None:
        options += ["-shadecell", repr(settings.shadeCell)]
    if settings.eventFile:
        options += ["-e", fileInside(vidaFolder, settings.eventFile, "event file")]
    if settings.terrain:
        options += ["-i", fileInside(vidaFolder, settings.terrain, "terrain file")]
    if settings.terrainMax is not None:
        options += ["-imax", repr(settings.terrainMax)]
    if settings.terrainMin is not None:
        options += ["-imin", repr(settings.terrainMin)]
    if settings.terrainScale is not None:
        options += ["-iscale", repr(settings.terrainScale)]
    if settings.waterLevel is not None:
        options += ["-iwater", repr(settings.waterLevel)]
    if settings.saveData is not None:
        options += ["-f", settings.saveData]
    if settings.archive is not None:
        options += ["-a", settings.archive]
    if settings.graphics:
        options += ["-g"] + list(settings.graphics)
    for option in settings.extra:
        if option in SERVER_OPTIONS:
            raise SettingsError("The server sets %s itself; leave it out of extra" % option)
    return options + list(settings.extra)


def readDefaults(vidaFolder):
    """Vida's defaults from Vida.ini, as text (what a run uses for anything
    it isn't given)."""
    config = configparser.RawConfigParser()
    config.optionxform = str
    config.read(Path(vidaFolder) / "Vida.ini")
    defaults = {}
    if config.has_section("Vida Options"):
        for key, value in config.items("Vida Options"):
            defaults[key] = value
    return defaults


def readVidaVersion(vidaFolder):
    ###the vidaVersion = "..." line near the top of Vida.py
    try:
        text = (Path(vidaFolder) / "Vida.py").read_text(errors="replace")
    except OSError:
        return None
    found = re.search(r'^vidaVersion\s*=\s*"([^"]*)"', text, re.MULTILINE)
    if found:
        return found.group(1)
    return None


def whenText(seconds):
    ###a time as text, or None
    if seconds is None:
        return None
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(seconds))


class LineReader(object):
    """Reads the lines added to a file since it last looked. Only complete
    lines (ending in a new line) are given; the rest waits for next time."""

    def __init__(self, path):
        self.path = Path(path)
        self.offset = 0
        self.partial = b""
        self.count = 0

    def newLines(self):
        try:
            with open(self.path, "rb") as theFile:
                theFile.seek(self.offset)
                data = theFile.read(READ_SIZE)
        except FileNotFoundError:
            return []
        self.offset += len(data)
        pieces = (self.partial + data).split(b"\n")
        ###the last piece is what comes after the last new line: not finished yet
        self.partial = pieces.pop()
        lines = []
        for piece in pieces:
            lines.append(piece.decode("utf-8"))
        self.count += len(lines)
        return lines

    def countNewLines(self):
        ###how many complete lines the file has now (without keeping them)
        while True:
            try:
                with open(self.path, "rb") as theFile:
                    theFile.seek(self.offset)
                    data = theFile.read(READ_SIZE)
            except FileNotFoundError:
                return self.count
            if not data:
                return self.count
            self.offset += len(data)
            self.count += data.count(b"\n")


class Run(object):
    """One copy of Vida, started by the server."""

    def __init__(self, name, settings, options, vidaFolder):
        self.name = name
        self.settings = settings
        self.vidaFolder = Path(vidaFolder)
        self.command = [sys.executable, "Vida.py", "-n", name] + options + ["-j", "-control", "Output-%s.control" % name]
        self.outputFolder = "Output-%s" % name
        self.viewerFile = self.vidaFolder / self.outputFolder / "viewer.jsonl"
        self.logFile = self.vidaFolder / ("Output-%s.log" % name)
        self.controlFile = self.vidaFolder / ("Output-%s.control" % name)
        self.lastCycle = self.readLastCycle(options)
        ###what the control file says (see Vida_Data/vcontrol.py)
        self.pauseAt = None
        if settings.paused:
            self.pauseAt = 0
        self.stopAsked = False
        self.process = None
        self.started = None
        self.ended = None
        self.exitCode = None
        self.lineCounter = LineReader(self.viewerFile)
        self.lock = threading.Lock()
        self.controlLock = threading.Lock()

    def readLastCycle(self, options):
        ###the last cycle Vida will run: -t, or maxCycles in Vida.ini
        if self.settings.cycles is not None:
            return self.settings.cycles
        if "-t" in options[:-1]:
            try:
                return int(options[options.index("-t") + 1])
            except ValueError:
                pass
        try:
            return int(readDefaults(self.vidaFolder).get("maxCycles"))
        except (TypeError, ValueError):
            return None

    def start(self):
        self.writeControl()
        environment = dict(os.environ)
        ###so what Vida prints reaches the log straight away
        environment["PYTHONUNBUFFERED"] = "1"
        with open(self.logFile, "w") as log:
            log.write("$ %s\n\n" % self.commandText())
            log.flush()
            self.process = subprocess.Popen(self.command, cwd=str(self.vidaFolder), env=environment,
                                            stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        self.started = time.time()

    def commandText(self):
        ###the command, as it would be typed (with quotes round any file names with spaces)
        return shlex.join(["python"] + self.command[1:])

    def writeControl(self):
        ###Write the control file in one go: write a new file, then put it in
        ###place of the old one, so Vida never reads half of it. (The lock
        ###stops two requests at once writing the new file together.)
        with self.controlLock:
            text = json.dumps({"pauseAt": self.pauseAt, "stop": self.stopAsked})
            newFile = self.controlFile.with_name(self.controlFile.name + ".new")
            newFile.write_text(text)
            for attempt in range(50):
                try:
                    os.replace(newFile, self.controlFile)
                    return
                except PermissionError:
                    ###(on Windows, while Vida has the file open for a moment)
                    time.sleep(0.01)
            os.replace(newFile, self.controlFile)

    def refresh(self):
        ###notice cycles Vida has saved, and whether it has finished
        with self.lock:
            self.lineCounter.countNewLines()
            if self.process is not None and self.exitCode is None:
                exitCode = self.process.poll()
                if exitCode is not None:
                    self.exitCode = exitCode
                    self.ended = time.time()
                    self.lineCounter.countNewLines()
                    try:
                        self.controlFile.unlink()
                    except OSError:
                        pass

    def cyclesDone(self):
        ###cycles saved so far (the first line of viewer.jsonl is the world)
        return max(0, self.lineCounter.count - 1)

    def isLive(self):
        return self.exitCode is None

    def state(self):
        """starting, running, paused or stopping while Vida runs; then
        finished, stopped or failed."""
        if self.exitCode is not None:
            if self.stopAsked:
                return "stopped"
            if self.exitCode == 0:
                return "finished"
            return "failed"
        if self.stopAsked:
            return "stopping"
        ###Vida waits before running cycle pauseAt (or later), and the next
        ###cycle it runs is the one after those already saved
        if self.pauseAt is not None and self.cyclesDone() >= self.pauseAt:
            return "paused"
        if self.lineCounter.count == 0:
            return "starting"
        return "running"

    def pause(self):
        ###Wait after the cycle it is on now. (Vida has already started the
        ###cycle after those saved, so it stops before the one after that.)
        self.refresh()
        if self.isLive() and self.state() != "paused":
            self.pauseAt = self.cyclesDone() + 1
            self.writeControl()

    def resume(self):
        if self.isLive():
            self.pauseAt = None
            self.writeControl()

    def step(self, cycles=1):
        ###run this many more cycles, then wait
        self.refresh()
        if self.isLive():
            self.pauseAt = self.cyclesDone() + cycles
            self.writeControl()

    def stop(self, now=False):
        ###Ask Vida to end the run after the cycle it is on, saving what it
        ###saves at the end. With now, end it straight away instead (nothing
        ###more is saved).
        if not self.isLive():
            return
        self.stopAsked = True
        self.writeControl()
        if now and self.process is not None:
            self.process.terminate()

    def logText(self, lastLines=None):
        try:
            text = self.logFile.read_text(errors="replace")
        except OSError:
            return ""
        if lastLines is not None:
            text = "\n".join(text.splitlines()[-lastLines:]) + "\n"
        return text

    def describe(self):
        """Everything about the run, for the API."""
        self.refresh()
        seconds = None
        if self.started is not None:
            seconds = round((self.ended or time.time()) - self.started, 1)
        return {
            "name": self.name,
            "state": self.state(),
            "cyclesDone": self.cyclesDone(),
            "lastCycle": self.lastCycle,
            "pauseAt": self.pauseAt,
            "started": whenText(self.started),
            "ended": whenText(self.ended),
            "seconds": seconds,
            "exitCode": self.exitCode,
            "command": self.commandText(),
            "settings": self.settings.model_dump(exclude_defaults=True),
            "outputFolder": self.outputFolder,
            "logFile": self.logFile.name,
        }


class RunList(object):
    """Every run the server has started, by name."""

    def __init__(self, vidaFolder, maxRunning=None):
        self.vidaFolder = Path(vidaFolder).resolve()
        self.maxRunning = maxRunning or os.cpu_count() or 1
        self.runs = {}
        self.lock = threading.Lock()

    def nameInUse(self, name):
        ###a name used by a run here, or by an output folder or log already on disk
        if name in self.runs:
            return True
        for path in (self.vidaFolder / ("Output-" + name), self.vidaFolder / ("Output-%s.log" % name)):
            if path.exists():
                return True
        return False

    def uniqueName(self, name):
        name = cleanName(name)
        if not self.nameInUse(name):
            return name
        number = 2
        while self.nameInUse("%s-%d" % (name, number)):
            number += 1
        return "%s-%d" % (name, number)

    def liveCount(self):
        count = 0
        for run in self.runs.values():
            run.refresh()
            if run.isLive():
                count += 1
        return count

    def start(self, settings):
        options = commandOptions(settings, self.vidaFolder)
        with self.lock:
            if self.liveCount() >= self.maxRunning:
                raise SettingsError("%d runs are going already, the most at once; stop one or wait" % self.maxRunning)
            name = self.uniqueName(settings.name)
            run = Run(name, settings, options, self.vidaFolder)
            run.start()
            self.runs[name] = run
        return run

    def get(self, name):
        return self.runs.get(name)

    def all(self):
        ###newest first
        return list(reversed(list(self.runs.values())))

    def forget(self, name):
        with self.lock:
            run = self.runs.get(name)
            if run is not None and not run.isLive():
                del self.runs[name]
                return True
        return False

    def stopAll(self, waitSeconds=5.0):
        ###when the server stops: ask every run to stop, then end any still going
        for run in self.runs.values():
            run.refresh()
            run.stop()
        deadline = time.time() + waitSeconds
        for run in self.runs.values():
            if run.process is None:
                continue
            try:
                run.process.wait(timeout=max(0.0, deadline - time.time()))
            except subprocess.TimeoutExpired:
                run.process.terminate()
            run.refresh()
