
VIDA - Tree and Forest Growth Simulation Software
----------

VIDA is a software suite that attempts to model the growth of individual trees using empiriclly derived--or randomly chosen--values for use with allometric relationships. By modeling the behavior of an individual tree, it is possible to model population dynamics in a spatially explicit simulationspace. 

This is a development release.

email: seanth@gmail.com

DEPENDENCIES/REQUIREMENTS
----------
	•Python v3.11
	•pyYAML v5.3
	•ContextFree v3.4.2 (optional. Needed to generate graphics)
		Apple users can make use of Homebrew (http://brew.sh/) to install the command line version of cfdg using the following commands:
			brew tap kn1kn1/cfdg
			brew install cfdg
	•ffmpeg (optional. Needed to generate videos from graphics)
  	•assimp (optional. Used to convert dxf files into stl)

  	There are environment.yml(for conda) and requirements.txt(for pip) files included. Context Free must be installed independently.

HOW TO USE
----------
After installing all the dependencies necessary, simply cd to the VIDA folder and, in the simplest form, type:

	>python VIDA.py
	
For more information, including command line options and ways to make species, event files and define planting locations, please see VIDA HOWTO.txt

To watch a simulation cycle by cycle in a web browser, add -j and open the Output folder's viewer.jsonl in viewer/index.html (see viewer/README.md). Or start Vida's web server (python -m server, after pip install -r requirements-server.txt) to start, pause, step, stop and watch simulations from the browser (see server/README.md).

EXAMPLES
--------
For more examples, including command line options and ways to make species, event files and define planting locations, please see VIDA HOWTO.txt
