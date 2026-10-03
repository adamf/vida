"""Vida's web server: start simulations, control them while they run, and
watch them in the viewer as they grow.

    python -m server            (from the folder with Vida.py in it)

then open http://127.0.0.1:8000 in a web browser for the viewer, or
http://127.0.0.1:8000/docs to try the API. server/README.md describes it.

The API, under /api:
    GET    /health                     the server and Vida's version
    GET    /choices                    Vida.ini's defaults, and the species,
                                       event, terrain and placement files
    GET    /runs                       every run the server has started
    POST   /runs                       start a run (see RunSettings in runs.py)
    GET    /runs/{name}                one run: its state, cycle, command...
    POST   /runs/{name}/pause          wait after the cycle it is on
    POST   /runs/{name}/resume         carry on
    POST   /runs/{name}/step?cycles=1  run this many more cycles, then wait
    POST   /runs/{name}/stop?now=false end the run (now: without saving more)
    DELETE /runs/{name}                forget a run that has ended (its files stay)
    GET    /runs/{name}/viewer         its viewer.jsonl so far
    GET    /runs/{name}/stream         its viewer.jsonl, a line at a time as
                                       Vida saves them (server-sent events)
    GET    /runs/{name}/log            what Vida printed
    GET    /outputs                    the Output- folders, from any run
    GET    /outputs/{folder}           the files in one
    GET    /outputs/{folder}/{file}    one of them
Everything else is the viewer (the viewer folder).
"""

import asyncio
import json
import os
from contextlib import asynccontextmanager
from pathlib import Path
from typing import Optional
from urllib.parse import quote

from fastapi import APIRouter, FastAPI, HTTPException, Query, Request
from fastapi.responses import FileResponse, PlainTextResponse, Response, StreamingResponse
from fastapi.staticfiles import StaticFiles

from server.runs import (GRAPHICAL_VIEWS, LineReader, RunList, RunSettings, SettingsError, readDefaults,
                         readVidaVersion, whenText)

###the folder this file is in, and Vida's folder (the one above it)
SERVER_FOLDER = Path(__file__).resolve().parent
VIDA_FOLDER = SERVER_FOLDER.parent

###how often a stream looks for new cycles, and sends something to keep the
###connection open when there is nothing new
STREAM_SECONDS = 0.25
KEEP_OPEN_SECONDS = 15.0

###the most files listed from one output folder
MOST_FILES = 5000

api = APIRouter(prefix="/api")


def theRuns(request):
    return request.app.state.runs


def vidaFolder(request):
    return request.app.state.runs.vidaFolder


def runCalled(request, name):
    run = theRuns(request).get(name)
    if run is None:
        raise HTTPException(status_code=404, detail="No run called '%s'" % name)
    return run


def filesIn(folder, folderName, endings):
    ###the files under one of Vida's folders with these endings, as paths
    ###from Vida's folder (with / between folders, on every computer)
    found = []
    top = folder / folderName
    if top.is_dir():
        for path in sorted(top.rglob("*")):
            if path.is_file() and path.suffix.lower() in endings:
                found.append(path.relative_to(folder).as_posix())
    return found


def terrainChoices(folder):
    ###Terrain images, and the folders with a .tif in them: given a folder,
    ###Vida also reads the elevations from an .xlsx file in it.
    images = filesIn(folder, "Terrain_files", (".tif", ".tiff", ".png", ".jpg", ".jpeg"))
    folders = []
    for image in images:
        if image.lower().endswith((".tif", ".tiff")):
            parent = image.rsplit("/", 1)[0] + "/"
            if parent not in folders:
                folders.append(parent)
    return folders + images


@api.get("/health")
def health(request: Request):
    """Whether the server is up, and which Vida it runs."""
    runs = theRuns(request)
    return {
        "server": "vida",
        "vida": readVidaVersion(runs.vidaFolder),
        "folder": str(runs.vidaFolder),
        "runs": len(runs.runs),
        "maxRunning": runs.maxRunning,
    }


@api.get("/choices")
def choices(request: Request):
    """What a run can be given: Vida.ini's defaults, and the files in Vida's folders."""
    folder = vidaFolder(request)
    species = []
    speciesFolder = folder / "Species"
    if speciesFolder.is_dir():
        ###Vida uses every .yml file directly in the Species folder
        for path in sorted(speciesFolder.glob("*.yml")):
            species.append(path.stem)
    return {
        "defaults": readDefaults(folder),
        "species": species,
        "eventFiles": filesIn(folder, "Event_Files", (".yml", ".yaml")),
        "terrainFiles": terrainChoices(folder),
        "placementFiles": filesIn(folder, "Placement_Files", (".csv",)),
        "graphicalViews": GRAPHICAL_VIEWS,
    }


@api.get("/runs")
def listRuns(request: Request):
    """Every run the server has started, newest first."""
    described = []
    for run in theRuns(request).all():
        described.append(run.describe())
    return described


@api.post("/runs", status_code=201)
def startRun(settings: RunSettings, request: Request):
    """Start a run. Anything left out comes from Vida.ini."""
    try:
        run = theRuns(request).start(settings)
    except SettingsError as error:
        raise HTTPException(status_code=400, detail=str(error))
    return run.describe()


@api.get("/runs/{name}")
def getRun(name: str, request: Request):
    return runCalled(request, name).describe()


@api.post("/runs/{name}/pause")
def pauseRun(name: str, request: Request):
    """Wait after the cycle it is on."""
    run = runCalled(request, name)
    run.pause()
    return run.describe()


@api.post("/runs/{name}/resume")
def resumeRun(name: str, request: Request):
    run = runCalled(request, name)
    run.resume()
    return run.describe()


@api.post("/runs/{name}/step")
def stepRun(name: str, request: Request, cycles: int = Query(1, ge=1)):
    """Run this many more cycles, then wait."""
    run = runCalled(request, name)
    run.step(cycles)
    return run.describe()


@api.post("/runs/{name}/stop")
def stopRun(name: str, request: Request, now: bool = False):
    """End the run after the cycle it is on, saving what Vida saves at the
    end. With now=true, end it straight away instead."""
    run = runCalled(request, name)
    run.stop(now)
    return run.describe()


@api.delete("/runs/{name}", status_code=204)
def forgetRun(name: str, request: Request):
    """Forget a run that has ended. Its files are left where they are."""
    runCalled(request, name)
    if not theRuns(request).forget(name):
        raise HTTPException(status_code=409, detail="'%s' is still going; stop it first" % name)
    return Response(status_code=204)


@api.get("/runs/{name}/viewer")
def runViewerFile(name: str, request: Request):
    """The run's viewer.jsonl so far (every complete line)."""
    run = runCalled(request, name)
    run.refresh()
    if not run.viewerFile.exists():
        raise HTTPException(status_code=404, detail="'%s' hasn't saved anything yet" % name)
    if not run.isLive():
        return FileResponse(run.viewerFile, media_type="application/x-ndjson")
    ###still being written: up to the end of the last complete line
    data = run.viewerFile.read_bytes()
    data = data[:data.rfind(b"\n") + 1]
    return Response(data, media_type="application/x-ndjson")


def streamEvent(kind, data, number=None):
    ###one server-sent event. The data is one line of JSON.
    text = ""
    if number is not None:
        text += "id: %d\n" % number
    return text + "event: %s\ndata: %s\n\n" % (kind, data)


async def streamLines(run, request, firstLine):
    ###Send each line of the run's viewer.jsonl as Vida saves it: the first
    ###line as a "header" event, the rest as "cycle" events, each with its
    ###line number as its id. A "status" event is sent when the run's state
    ###changes, and an "end" event once it has ended and every line is sent.
    reader = LineReader(run.viewerFile)
    lastStatus = None
    quietSeconds = 0.0
    while True:
        if await request.is_disconnected():
            return
        ###look at whether it has ended before reading, so no line is missed
        run.refresh()
        ended = not run.isLive()
        lines = reader.newLines()
        number = reader.count - len(lines)
        for line in lines:
            if number >= firstLine:
                if number == 0:
                    yield streamEvent("header", line, number)
                else:
                    yield streamEvent("cycle", line, number)
            number += 1
        status = run.describe()
        if (status["state"], status["cyclesDone"], status["pauseAt"]) != lastStatus:
            lastStatus = (status["state"], status["cyclesDone"], status["pauseAt"])
            yield streamEvent("status", json.dumps(status))
        if ended and not lines:
            yield streamEvent("end", json.dumps(status))
            return
        if lines:
            quietSeconds = 0.0
        else:
            await asyncio.sleep(STREAM_SECONDS)
            quietSeconds += STREAM_SECONDS
            if quietSeconds >= KEEP_OPEN_SECONDS:
                quietSeconds = 0.0
                yield ": still here\n\n"


@api.get("/runs/{name}/stream")
def streamRun(name: str, request: Request, start: Optional[int] = Query(None, alias="from", ge=0)):
    """The run's viewer.jsonl, a line at a time as Vida saves them, as
    server-sent events (see streamLines). from=N starts at line N; a browser
    that reconnects starts after the last line it was sent."""
    run = runCalled(request, name)
    firstLine = start or 0
    lastSeen = request.headers.get("last-event-id")
    if lastSeen is not None and lastSeen.isdigit():
        firstLine = int(lastSeen) + 1
    headers = {"Cache-Control": "no-cache", "X-Accel-Buffering": "no"}
    return StreamingResponse(streamLines(run, request, firstLine), media_type="text/event-stream", headers=headers)


@api.get("/runs/{name}/log", response_class=PlainTextResponse)
def runLog(name: str, request: Request, lines: Optional[int] = Query(None, ge=1)):
    """What Vida printed (the last few lines, with lines=N)."""
    return runCalled(request, name).logText(lines)


def outputFolder(request, folder):
    ###one of the Output- folders in Vida's folder, or a 404
    top = vidaFolder(request)
    path = (top / folder).resolve()
    if not folder.startswith("Output-") or path.parent != top or not path.is_dir():
        raise HTTPException(status_code=404, detail="No output folder called '%s'" % folder)
    return path


@api.get("/outputs")
def listOutputs(request: Request):
    """The Output- folders in Vida's folder (from the server's runs or any
    other), newest first, with a link to each one's viewer.jsonl."""
    found = []
    for path in vidaFolder(request).glob("Output-*"):
        if not path.is_dir():
            continue
        modified = path.stat().st_mtime
        viewer = None
        if (path / "viewer.jsonl").is_file():
            viewer = "api/outputs/%s/viewer.jsonl" % quote(path.name)
            modified = max(modified, (path / "viewer.jsonl").stat().st_mtime)
        found.append({"folder": path.name, "modified": modified, "viewer": viewer})
    found.sort(key=modifiedTime, reverse=True)
    for entry in found:
        entry["modified"] = whenText(entry["modified"])
    return found


def modifiedTime(entry):
    return entry["modified"]


@api.get("/outputs/{folder}")
def listOutputFiles(folder: str, request: Request):
    """The files in one Output- folder."""
    path = outputFolder(request, folder)
    files = []
    for top, folders, names in os.walk(path):
        folders.sort()
        for name in sorted(names):
            file = Path(top) / name
            files.append({"path": file.relative_to(path).as_posix(), "size": file.stat().st_size})
            if len(files) >= MOST_FILES:
                return {"folder": folder, "files": files, "complete": False}
    return {"folder": folder, "files": files, "complete": True}


@api.get("/outputs/{folder}/{file:path}")
def getOutputFile(folder: str, file: str, request: Request):
    """One file from an Output- folder."""
    path = outputFolder(request, folder)
    target = (path / file).resolve()
    if not target.is_relative_to(path) or not target.is_file():
        raise HTTPException(status_code=404, detail="No file called '%s' in %s" % (file, folder))
    if target.suffix == ".jsonl":
        return FileResponse(target, media_type="application/x-ndjson")
    return FileResponse(target)


def makeApp(folder=None, maxRunning=None):
    """The web server for the Vida in this folder (the one above server/)."""
    runs = RunList(folder or VIDA_FOLDER, maxRunning)

    @asynccontextmanager
    async def lifespan(app):
        yield
        ###the server is stopping: so are its runs
        runs.stopAll()

    app = FastAPI(title="Vida", description="Start Vida simulations, control them while they run, and watch them.",
                  version=readVidaVersion(runs.vidaFolder) or "", lifespan=lifespan)
    app.state.runs = runs
    app.include_router(api)
    ###everything else is the viewer
    app.mount("/", StaticFiles(directory=runs.vidaFolder / "viewer", html=True), name="viewer")
    return app
