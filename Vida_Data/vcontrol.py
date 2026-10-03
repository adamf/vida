"""This file is part of Vida.
    --------------------------
    Copyright 2023, Sean T. Hammond

    Vida is experimental in nature and is made available as a research courtesy "AS IS," but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

    You should have received a copy of academic software agreement along with Vida. If not, see <https://github.com/seanth/Vida/blob/master/LICENSE.txt>.
"""

###Lets another program pause, step and stop a simulation while it runs,
###with the -control option. Vida's web server (server/) uses it.
###
###The other program writes a small JSON file, for example
###    {"pauseAt": 12, "stop": false}
###  pauseAt  wait before running this cycle (null: don't wait). To run one
###           cycle at a time, the other program moves it on by one.
###  stop     finish now. Vida ends the run as if it had reached its last
###           cycle, so what it saves at the end is still saved.
###
###Vida only ever reads the file: at the start of each cycle, and every tenth
###of a second while it waits. If the file is missing or only half written,
###Vida carries on as it was last told.

import json
import time

###how long to wait between reads of the file while paused
WAIT_SECONDS = 0.1


class Control(object):
    def __init__(self, fileName):
        self.fileName = fileName
        self.pauseAt = None
        self.stop = False

    def read(self):
        ###read the file again. Returns False (and changes nothing) if it
        ###can't be read.
        try:
            with open(self.fileName) as theFile:
                settings = json.load(theFile)
        except (OSError, ValueError):
            return False
        if not isinstance(settings, dict):
            return False
        pauseAt = settings.get("pauseAt")
        ###(True and False are ints in Python, so they are left out on purpose)
        if pauseAt is None or (isinstance(pauseAt, int) and not isinstance(pauseAt, bool)):
            self.pauseAt = pauseAt
        self.stop = settings.get("stop") is True
        return True

    def waiting(self, cycleNumber):
        ###True if the file says to wait before running this cycle
        return (not self.stop) and self.pauseAt is not None and cycleNumber >= self.pauseAt

    def mayRunCycle(self, cycleNumber):
        ###Call before running a cycle. Waits for as long as the file says to
        ###pause before this cycle. Returns False if the simulation should stop.
        self.read()
        while self.waiting(cycleNumber):
            time.sleep(WAIT_SECONDS)
            self.read()
        return not self.stop
