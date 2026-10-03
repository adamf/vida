"""python -m server: start Vida's web server (from the folder with Vida.py in it).

    python -m server
    python -m server -port 8001 -maxruns 2

Then open http://127.0.0.1:8000 in a web browser. Ctrl-C stops the server,
and any runs it started. python -m server -h lists the options.
"""

import argparse
import sys

try:
    import uvicorn
    from server.app import makeApp
except ImportError as error:
    sys.exit("The web server needs FastAPI and uvicorn (%s). Install them with\n"
             "    pip install -r requirements-server.txt" % error)


def main():
    parser = argparse.ArgumentParser(prog="python -m server",
                                     description="Vida's web server: start, control and watch simulations from a web browser.")
    parser.add_argument("-port", type=int, default=8000, help="the port to listen on (default 8000)")
    parser.add_argument("-host", default="127.0.0.1",
                        help="the address to listen on (default 127.0.0.1: only this computer). 0.0.0.0 lets other "
                             "computers in, and anyone who can reach it can run Vida and read its output folders")
    parser.add_argument("-maxruns", type=int, default=None, help="the most runs going at once (default: the number of cores)")
    parser.add_argument("-vida", default=None, help="Vida's folder (default: the one the server folder is in)")
    arguments = parser.parse_args()

    app = makeApp(arguments.vida, arguments.maxruns)
    if arguments.host not in ("127.0.0.1", "localhost", "::1"):
        print("*** Listening on %s: anyone who can reach this computer can start runs and read their output. ***"
              % arguments.host)
    print("Vida's web server: open http://%s:%d in a web browser (and /docs for the API). Ctrl-C stops it."
          % (arguments.host, arguments.port))
    ###"warning": don't print a line for every request
    uvicorn.run(app, host=arguments.host, port=arguments.port, log_level="warning")


if __name__ == "__main__":
    main()
