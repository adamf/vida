"""This file is part of Vida.
    --------------------------
    Copyright 2026, Sean T. Hammond

    Vida is experimental in nature and is made available as a research courtesy "AS IS," but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

    You should have received a copy of academic software agreement along with Vida. If not, see <https://github.com/seanth/Vida/blob/master/LICENSE.txt>.
"""

###How the processors running one world talk to each other.
###
###Each processor (a "rank", numbered from 0) has its own part of the world.
###They only ever talk in three ways, all together, like a meeting where
###nobody leaves until everyone has spoken:
###    alltoall(letters): letters[r] goes to rank r; gives back what each
###        rank sent to this one
###    allreduce(number, "sum" or "max"): the total (or largest) over all ranks
###    allgather(thing): everyone's thing, in rank order
###
###There are three ways to run them:
###    SerialComm   one processor
###    ThreadComm   several ranks as threads of one Python process (for the
###                 tests, so they don't need MPI)
###    MpiComm      real separate processes, over MPI (mpi4py). This is how
###                 it would run on a supercomputer, on thousands of ranks:
###                 mpiexec -n 4 python -m worldscale.run ...

import threading


class SerialComm:
    rank = 0
    size = 1

    def alltoall(self, letters):
        return list(letters)

    def allreduce(self, value, how="sum"):
        return value

    def allgather(self, value):
        return [value]


class ThreadWorld:
    ###what the ranks of a ThreadComm share: a table of letters and a barrier
    def __init__(self, size):
        self.size = size
        self.barrier = threading.Barrier(size)
        self.table = [None] * size


class ThreadComm:
    def __init__(self, world, rank):
        self.world = world
        self.rank = rank
        self.size = world.size

    def meet(self, value):
        ###everyone puts their value on the table, then everyone reads the table
        self.world.table[self.rank] = value
        self.world.barrier.wait()
        everything = list(self.world.table)
        self.world.barrier.wait()
        return everything

    def alltoall(self, letters):
        everything = self.meet(letters)
        received = []
        for sender in range(self.size):
            received.append(everything[sender][self.rank])
        return received

    def allreduce(self, value, how="sum"):
        everything = self.meet(value)
        if how == "max":
            return max(everything)
        total = everything[0]
        for other in everything[1:]:
            total = total + other
        return total

    def allgather(self, value):
        return self.meet(value)


class MpiComm:
    def __init__(self):
        from mpi4py import MPI
        self.mpi = MPI
        self.comm = MPI.COMM_WORLD
        self.rank = self.comm.Get_rank()
        self.size = self.comm.Get_size()

    def alltoall(self, letters):
        return self.comm.alltoall(letters)

    def allreduce(self, value, how="sum"):
        if how == "max":
            return self.comm.allreduce(value, op=self.mpi.MAX)
        return self.comm.allreduce(value, op=self.mpi.SUM)

    def allgather(self, value):
        return self.comm.allgather(value)


def runAsThreads(size, work, arguments):
    ###Run work(comm, *arguments) on `size` ranks at once, as threads. Gives
    ###back each rank's answer, in rank order.
    world = ThreadWorld(size)
    answers = [None] * size
    problems = []

    def runOne(rank):
        try:
            answers[rank] = work(ThreadComm(world, rank), *arguments)
        except BaseException as problem:
            problems.append(problem)
            world.barrier.abort()

    threads = []
    for rank in range(size):
        thread = threading.Thread(target=runOne, args=(rank,))
        threads.append(thread)
        thread.start()
    for thread in threads:
        thread.join()
    if problems:
        raise problems[0]
    return answers
