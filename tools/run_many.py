"""Run several Vida simulations at once, one on each processor core.

Within one simulation everything happens in a set order, drawing on the
same stream of random numbers, so one simulation can't be split across
cores without changing its results. But separate simulations don't share
anything: this runs each one as its own copy of Vida, side by side. Each
gives exactly the same results as it would if run on its own; only the
order they finish in changes.

Run it from the folder that has Vida.py in it. Examples:

    # the same simulation with the run ids 1 to 8, four at a time
    python tools/run_many.py -runids 1-8 -jobs 4 -- -n forest -w 100 -s 400 -t 50

    # every line of runs.txt is the options for one run, e.g.
    #     -n dry -w 100 -s 400 -t 50 -runid 1
    #     -n wet -w 100 -s 400 -t 50 -runid 1
    python tools/run_many.py -file runs.txt

With -runids, each run is named after the -n name and its run id (forest-run1,
forest-run2, ...). Each run's output goes where Vida puts it
(Output-forest-run1/ and so on), and what it prints goes to a log file next
to it (Output-forest-run1.log). -jobs is how many run at once; it starts
at the number of cores.
"""

import argparse
import os
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor


def readRunids(text):
    # "1-8" or "1,3,5" or "1-4,10" -> [1, 2, ...]
    runids = []
    for part in text.split(","):
        part = part.strip()
        if "-" in part:
            first, last = part.split("-")
            for runid in range(int(first), int(last) + 1):
                runids.append(runid)
        elif part:
            runids.append(int(part))
    return runids


def nameOf(options):
    # the value after -n in a run's options, or None
    for i in range(len(options) - 1):
        if options[i] == "-n":
            return options[i + 1]
    return None


def runsForRunids(runids, options):
    # One run per run id: the same options, with its own -runid and name.
    if "-runid" in options:
        sys.exit("run_many: leave -runid out of the options; -runids sets it for each run")
    baseName = nameOf(options) or "default"
    rest = []
    skipNext = False
    for option in options:
        if skipNext:
            skipNext = False
        elif option == "-n":
            skipNext = True
        else:
            rest.append(option)
    runs = []
    for runid in runids:
        name = "%s-run%d" % (baseName, runid)
        runs.append(["-n", name, "-runid", str(runid)] + rest)
    return runs


def runsFromFile(fileName):
    # One run for each line that isn't empty or a comment (#).
    runs = []
    with open(fileName) as theFile:
        for line in theFile:
            line = line.strip()
            if line and not line.startswith("#"):
                runs.append(line.split())
    return runs


def runOne(options):
    # Run Vida once, with what it prints going to a log file.
    name = nameOf(options) or "run"
    logName = "Output-%s.log" % name
    start = time.time()
    with open(logName, "w") as log:
        result = subprocess.run([sys.executable, "Vida.py"] + options,
                                stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
    seconds = time.time() - start
    if result.returncode == 0:
        print("finished %s in %.1f s" % (name, seconds))
    else:
        print("FAILED %s (exit code %d): see %s" % (name, result.returncode, logName))
    return seconds, result.returncode


def main():
    parser = argparse.ArgumentParser(description="Run several Vida simulations at once.")
    parser.add_argument("-runids", help="run ids to run, e.g. 1-8 or 1,3,5")
    parser.add_argument("-file", help="a file with the options for one run on each line")
    parser.add_argument("-jobs", type=int, default=os.cpu_count() or 1, help="how many to run at once (default: the number of cores)")
    parser.add_argument("options", nargs=argparse.REMAINDER, help="after --, Vida's options for every run (with -runids)")
    arguments = parser.parse_args()
    options = arguments.options
    if options and options[0] == "--":
        options = options[1:]

    if not os.path.exists("Vida.py"):
        sys.exit("run_many: run this from the folder that has Vida.py in it")
    if arguments.runids and arguments.file:
        sys.exit("run_many: use -runids or -file, not both")
    if arguments.runids:
        runs = runsForRunids(readRunids(arguments.runids), options)
    elif arguments.file:
        runs = runsFromFile(arguments.file)
    else:
        sys.exit("run_many: say which runs, with -runids or -file (see python tools/run_many.py -h)")

    jobs = max(1, arguments.jobs)
    print("%d runs, %d at a time" % (len(runs), jobs))
    start = time.time()
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        # each thread just starts one copy of Vida and waits for it
        results = list(pool.map(runOne, runs))
    wallClock = time.time() - start
    total = 0.0
    failed = 0
    for seconds, returnCode in results:
        total += seconds
        if returnCode != 0:
            failed += 1
    print("all done in %.1f s (the runs took %.1f s between them)" % (wallClock, total))
    if failed:
        sys.exit("%d of the runs failed" % failed)


if __name__ == "__main__":
    main()
