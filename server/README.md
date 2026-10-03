# Vida's web server

Start Vida simulations, control them while they run, and watch them grow,
from a web browser or from any program that can make web requests. It is
built with [FastAPI](https://fastapi.tiangolo.com) and serves the viewer
(`viewer/`), which gets a **Simulations** panel when it comes from here.

## Starting it

From the folder with `Vida.py` in it:

```
pip install -r requirements-server.txt
python -m server
```

Then go to <http://127.0.0.1:8000> for the viewer, or
<http://127.0.0.1:8000/docs> to read and try the API in the browser.
Ctrl-C stops the server, and any runs it started.

- `-port 8001`: listen on another port.
- `-maxruns 2`: the most runs going at once (to begin with, the number of
  cores; each run uses one).
- `-host 0.0.0.0`: let other computers in. See [Safety](#safety) first.
- `-vida folder`: run the Vida in another folder.

## In the browser

See "Watching a simulation as it runs" in `viewer/README.md`: start a run
from a form, watch it a cycle at a time, pause it, step it a cycle at a
time, stop it, read its log, and open any saved run.

## The API

Everything is under `/api`. Answers are JSON unless it says otherwise.

| | | |
|---|---|---|
| `GET` | `/api/health` | the server, and Vida's version and folder |
| `GET` | `/api/choices` | Vida.ini's defaults, and the species, event, terrain and placement files |
| `GET` | `/api/runs` | every run the server has started, newest first |
| `POST` | `/api/runs` | start a run (the settings are below) |
| `GET` | `/api/runs/{name}` | one run (described below) |
| `POST` | `/api/runs/{name}/pause` | wait after the cycle it is on |
| `POST` | `/api/runs/{name}/resume` | carry on |
| `POST` | `/api/runs/{name}/step?cycles=1` | run this many more cycles, then wait |
| `POST` | `/api/runs/{name}/stop` | end the run after the cycle it is on (Vida still saves what it saves at the end); `?now=true` ends it straight away |
| `DELETE` | `/api/runs/{name}` | forget a run that has ended (its files stay) |
| `GET` | `/api/runs/{name}/viewer` | its `viewer.jsonl` so far |
| `GET` | `/api/runs/{name}/stream` | its `viewer.jsonl`, a line at a time as Vida saves them (below) |
| `GET` | `/api/runs/{name}/log?lines=40` | what Vida printed (text) |
| `GET` | `/api/outputs` | every `Output-` folder, newest first, from the server or the command line |
| `GET` | `/api/outputs/{folder}` | the files in one |
| `GET` | `/api/outputs/{folder}/{file}` | one of them |

### Starting a run

`POST /api/runs` takes Vida's options by name. Anything left out comes
from Vida.ini, as on the command line.

| setting | Vida's option | |
|---|---|---|
| `name` | `-n` | a number is added if the name has been used (`forest-2`) |
| `worldSize` | `-w` | |
| `cycles` | `-t` | the last cycle (cycles 0 to this are run) |
| `maxPopulation` | `-m` | |
| `seeds` and `placement` | `-s`, `-ss` or `-sh` | `placement` is `random`, `square` or `hex` |
| `placementFile` | `-sf` | e.g. `Placement_Files/treePattern.csv` |
| `rngStart` | `-rngstart` | |
| `shade`, `shadeCell` | `-shade`, `-shadecell` | |
| `eventFile` | `-e` | e.g. `Event_Files/islands.yml` |
| `terrain` | `-i` | a terrain image, or a folder with one in it |
| `terrainMax`, `terrainMin`, `terrainScale`, `waterLevel` | `-imax`, `-imin`, `-iscale`, `-iwater` | |
| `saveData`, `archive` | `-f`, `-a` | `a`, `e`, `n` or `s` |
| `graphics` | `-g` | a list, e.g. `["bs", "glb"]` |
| `paused` | | `true` to start paused, before cycle 0 |
| `extra` | | any more of Vida's options, as a list: `["-iwaterstyle", "none"]` |

Files must be inside Vida's folder. The server adds `-n`, `-j` and
`-control` itself, so they (and `-x`) can't be in `extra`.

```
curl -X POST http://127.0.0.1:8000/api/runs -H "Content-Type: application/json" \
     -d '{"name": "forest", "worldSize": 100, "seeds": 300, "cycles": 200, "rngStart": 1}'
curl -X POST http://127.0.0.1:8000/api/runs/forest/pause
curl -X POST "http://127.0.0.1:8000/api/runs/forest/step?cycles=5"
curl http://127.0.0.1:8000/api/runs/forest
curl -X POST http://127.0.0.1:8000/api/runs/forest/resume
```

### What a run looks like

```json
{
  "name": "forest",
  "state": "paused",
  "cyclesDone": 12,
  "lastCycle": 200,
  "pauseAt": 12,
  "started": "2026-10-03T14:02:11",
  "ended": null,
  "seconds": 9.4,
  "exitCode": null,
  "command": "python Vida.py -n forest -w 100 -t 200 -s 300 -rngstart 1 -j -control Output-forest.control",
  "settings": {"name": "forest", "worldSize": 100, "cycles": 200, "seeds": 300, "rngStart": 1},
  "outputFolder": "Output-forest",
  "logFile": "Output-forest.log"
}
```

- `state`: while Vida runs, `starting` (no cycle saved yet), `running`,
  `paused` or `stopping`; then `finished`, `stopped` (it was asked to stop)
  or `failed` (see its log).
- `cyclesDone`: how many cycles are saved. They are cycles 0 to
  `cyclesDone - 1`; `lastCycle` is the last one it will run, unless it stops
  early (its population dies out or reaches `maxPopulation`).
- `pauseAt`: it waits before running this cycle (`null`: it doesn't wait).

### The stream

`GET /api/runs/{name}/stream` sends the run's `viewer.jsonl` as
[server-sent events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events),
each as soon as Vida saves it:

- `header`: the first line of the file (the world);
- `cycle`: each line after it (one cycle), with its line number as the id;
- `status`: the run (as above) whenever its state or `cyclesDone` changes;
- `end`: the run, once it has ended and every line has been sent.

`?from=N` starts at line N. A browser's `EventSource` that loses the
connection carries on from the last line it got. `viewer/server.js` shows
how to use it; with curl:

```
curl -N http://127.0.0.1:8000/api/runs/forest/stream
```

### From Python

With nothing to install:

```python
import json
import urllib.request

SERVER = "http://127.0.0.1:8000/api/"

def call(method, path, settings=None):
    data = None
    if settings is not None:
        data = json.dumps(settings).encode()
    request = urllib.request.Request(SERVER + path, data=data, method=method,
                                     headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request) as answer:
        return json.loads(answer.read())

run = call("POST", "runs", {"name": "dry", "worldSize": 50, "seeds": 100, "cycles": 50})
print(run["name"], run["state"])
```

## How it works

- Each run is its own copy of Vida, `python Vida.py ...`, started with the
  options above plus `-j` and `-control`. So it gives exactly the same
  results as the same options on the command line (a test checks this), and
  its `Output-` folder is the same.
- `-j` writes each cycle to `viewer.jsonl` as soon as it is done. The server
  watches the file to know how far a run has got, and to stream it.
- `-control` gives Vida a small file to read at the start of each cycle
  (`Vida_Data/vcontrol.py`): `{"pauseAt": 12, "stop": false}`. Vida waits
  before cycle `pauseAt`, reading the file again every tenth of a second, and
  ends the run, as if it had reached its last cycle, if `stop` is true. Only
  the server writes the file, and Vida only reads it, which works the same on
  every computer. Pausing and stopping happen between cycles: in a big world
  with long cycles, that can take a while (`stop?now=true` doesn't wait).
- What Vida prints goes to `Output-<name>.log`, next to the output folder,
  as with `tools/run_many.py`. The control file, `Output-<name>.control`, is
  removed when the run ends.
- The server remembers its runs only while it is running. After it is
  started again, earlier runs are still under Saved simulations (their
  `Output-` folders), but can't be controlled. When the server stops, it
  stops any runs still going.

## Safety

To begin with, only this computer can reach the server. With
`-host 0.0.0.0`, any computer that can reach this one can start runs with
any options and read every `Output-` folder: there is no password. Only do
that on a network you trust.

## Files

- `__main__.py`: `python -m server`: its options, and starting it
- `app.py`: the API, and serving the viewer
- `runs.py`: starting runs, controlling them and keeping track of them
- `../Vida_Data/vcontrol.py`: Vida's side of the control file
- `../viewer/server.js`: the viewer's Simulations panel
- `../tests/unit/test_server.py` and `test_vcontrol.py`: the tests
