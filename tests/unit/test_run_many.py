"""Tests for tools/run_many.py: working out which runs to start."""

import importlib.util
from pathlib import Path

import pytest

TOOL = Path(__file__).resolve().parents[2] / "tools" / "run_many.py"
spec = importlib.util.spec_from_file_location("run_many", TOOL)
run_many = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run_many)


def test_reading_run_ids():
    assert run_many.readRunids("1-4") == [1, 2, 3, 4]
    assert run_many.readRunids("1,3,5") == [1, 3, 5]
    assert run_many.readRunids("1-3,10") == [1, 2, 3, 10]


def test_one_run_per_run_id_each_with_its_own_name():
    runs = run_many.runsForRunids([1, 2], ["-n", "forest", "-w", "100", "-t", "50"])
    assert runs == [
        ["-n", "forest-run1", "-runid", "1", "-w", "100", "-t", "50"],
        ["-n", "forest-run2", "-runid", "2", "-w", "100", "-t", "50"],
    ]


def test_runs_without_a_name_use_vidas_default_name():
    assert run_many.runsForRunids([7], ["-w", "50"]) == [["-n", "default-run7", "-runid", "7", "-w", "50"]]


def test_runid_in_the_options_is_refused():
    with pytest.raises(SystemExit):
        run_many.runsForRunids([1], ["-n", "forest", "-runid", "3"])


def test_runs_from_a_file(tmp_path):
    runs = tmp_path / "runs.txt"
    runs.write_text("# dry and wet\n-n dry -w 100 -runid 1\n\n-n wet -w 100 -runid 1\n")
    assert run_many.runsFromFile(str(runs)) == [["-n", "dry", "-w", "100", "-runid", "1"], ["-n", "wet", "-w", "100", "-runid", "1"]]
    assert run_many.nameOf(["-n", "dry", "-w", "100"]) == "dry"
