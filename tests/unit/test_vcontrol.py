"""Tests for Vida_Data/vcontrol.py: the file that pauses, steps and stops a run (-control)."""

import json
import threading
import time

import vcontrol


def writeControl(path, settings):
    path.write_text(json.dumps(settings))


def test_without_a_file_vida_carries_on(tmp_path):
    control = vcontrol.Control(str(tmp_path / "missing.control"))
    assert control.mayRunCycle(0)
    assert control.mayRunCycle(100)


def test_reading_the_file(tmp_path):
    path = tmp_path / "run.control"
    writeControl(path, {"pauseAt": 12, "stop": False})
    control = vcontrol.Control(str(path))
    assert control.read()
    assert control.pauseAt == 12
    assert not control.stop
    # it waits before cycle 12 and every cycle after it
    assert not control.waiting(11)
    assert control.waiting(12)
    assert control.waiting(13)


def test_a_half_written_file_changes_nothing(tmp_path):
    path = tmp_path / "run.control"
    writeControl(path, {"pauseAt": 5})
    control = vcontrol.Control(str(path))
    control.read()
    path.write_text('{"pauseAt": nu')
    assert not control.read()
    assert control.pauseAt == 5


def test_only_whole_numbers_pause(tmp_path):
    path = tmp_path / "run.control"
    control = vcontrol.Control(str(path))
    for wrong in [True, "3", 2.5, [1]]:
        writeControl(path, {"pauseAt": wrong})
        control.read()
        assert control.pauseAt is None


def test_stop(tmp_path):
    path = tmp_path / "run.control"
    writeControl(path, {"pauseAt": None, "stop": True})
    control = vcontrol.Control(str(path))
    assert not control.mayRunCycle(3)


def test_stop_ends_a_pause(tmp_path):
    path = tmp_path / "run.control"
    writeControl(path, {"pauseAt": 0})
    control = vcontrol.Control(str(path))
    results = []

    def runCycleZero():
        results.append(control.mayRunCycle(0))

    waiter = threading.Thread(target=runCycleZero)
    waiter.start()
    time.sleep(0.3)
    # still waiting
    assert results == []
    writeControl(path, {"pauseAt": 0, "stop": True})
    waiter.join(5)
    assert results == [False]


def test_moving_the_pause_on_lets_a_cycle_run(tmp_path):
    path = tmp_path / "run.control"
    writeControl(path, {"pauseAt": 4})
    control = vcontrol.Control(str(path))
    results = []

    def runCycleFour():
        results.append(control.mayRunCycle(4))

    waiter = threading.Thread(target=runCycleFour)
    waiter.start()
    time.sleep(0.3)
    assert results == []
    # one more cycle: wait before cycle 5 instead
    writeControl(path, {"pauseAt": 5})
    waiter.join(5)
    assert results == [True]
    assert control.waiting(5)
