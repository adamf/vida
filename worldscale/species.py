"""This file is part of Vida.
    --------------------------
    Copyright 2026, Sean T. Hammond

    Vida is experimental in nature and is made available as a research courtesy "AS IS," but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

    You should have received a copy of academic software agreement along with Vida. If not, see <https://github.com/seanth/Vida/blob/master/LICENSE.txt>.
"""

###Species and world settings for the world-scale prototype, read from
###Vida's own files: Default_species.yml, the species files and
###"Vida World Preferences.yml".
###
###Each setting becomes one array with a value for each species, so a
###tree's setting is `table.photoConstant[tree species number]`, looked up for
###millions of trees at once.

import os

import numpy
import yaml

###settings that are numbers (all the ones the prototype uses)
NUMBER_SETTINGS = [
    "densityStem", "densityLeaf", "densitySeed", "canopyTransmittance",
    "fractionMinimumSurvival", "heightLeafMax", "heightStemMax", "youngsModulusStem",
    "fractionSelfishness", "startMakingSeedsAge", "reproductionConstant",
    "reproductionExponent", "numYearsGrowthMemory", "massSeedMax", "delayInGermination",
    "randomSlowGrowth", "fractionFailGerminate", "photoConstant", "photoExponent",
    "fractionCarbonToSeeds", "fractMassSeedMaxToGerm", "fractionSeedMassToPlant",
    "fractionCarbonToStem",
    "speciesConstant1", "speciesExponent1", "speciesConstant2", "speciesExponent2",
    "speciesConstant3", "speciesExponent3", "speciesConstant6", "speciesConstant7",
    "speciesExponent7", "speciesConstant8", "speciesConstant20", "speciesExponent20",
]
TRUE_FALSE_SETTINGS = ["makeSeeds", "leafIsHemisphere"]


class SpeciesTable:
    def __init__(self, speciesFiles, vidaFolder):
        defaults = readYaml(os.path.join(vidaFolder, "Vida_Data", "Default_species.yml"))
        self.names = []
        allSettings = []
        for fileName in speciesFiles:
            settings = dict(defaults)
            settings.update(readYaml(fileName))
            allSettings.append(settings)
            self.names.append(str(settings.get("nameSpecies", os.path.basename(fileName))))
        self.count = len(allSettings)
        for name in NUMBER_SETTINGS:
            values = []
            for settings in allSettings:
                values.append(float(settings[name]))
            setattr(self, name, numpy.array(values))
        for name in TRUE_FALSE_SETTINGS:
            values = []
            for settings in allSettings:
                values.append(bool(settings[name]))
            setattr(self, name, numpy.array(values))
        ###the lists: where on the canopy seeds form, and how they're dispersed
        formationMax = []
        formationMin = []
        method = []
        dispersal1 = []
        dispersal2 = []
        for settings in allSettings:
            formation = settings["locSeedFormation"]
            formationMax.append(min(max(float(formation[0]), 0.0), 1.0))
            formationMin.append(min(max(float(formation[1]), 0.0), 1.0))
            dispersal = list(settings["seedDispersalMethod"]) + [0.0, 0.0]
            method.append(int(dispersal[0]))
            dispersal1.append(float(dispersal[1]))
            dispersal2.append(float(dispersal[2]))
        self.formationMax = numpy.array(formationMax)
        self.formationMin = numpy.array(formationMin)
        self.dispersalMethod = numpy.array(method)
        self.dispersal1 = numpy.array(dispersal1)
        self.dispersal2 = numpy.array(dispersal2)
        ###Vida sets photoConstantShade to photoConstant for every plant
        self.photoConstantShade = self.photoConstant.copy()
        self.memory = numpy.maximum(self.numYearsGrowthMemory.astype(int), 1)


class WorldSettings:
    ###"Vida World Preferences.yml", with the few settings the prototype uses
    def __init__(self, vidaFolder):
        settings = readYaml(os.path.join(vidaFolder, "Vida World Preferences.yml"))
        self.gravity = float(settings["gravity"])
        self.lightIntensity = float(settings["lightIntensity"])
        self.maxSeedsPerPlant = int(settings["maxSeedsPerPlant"])
        self.ignoreGermDeathAtStart = bool(settings["ignoreGermDeathAtStart"])
        self.allowRandomDeath = bool(settings["allowRandomDeath"])
        self.randomDeathPlant = float(settings["randomDeathPlant"])
        self.randomDeathSeed = float(settings["randomDeathSeed"])
        self.allowSlowGrowthDeath = bool(settings["allowSlowGrowthDeath"])
        self.randomSlowGrowth = float(settings["randomSlowGrowth"])
        self.allowEulerGreenhillViolations = bool(settings["allowEulerGreenhillViolations"])
        self.allowOverlaps = bool(settings["allowOverlaps"])
        self.allowOffWorld = bool(settings["allowOffWorld"])


def readYaml(fileName):
    with open(fileName) as theFile:
        return yaml.safe_load(theFile)


def speciesFilesIn(folder):
    ###the .yml species files directly in a folder, in sorted order
    names = []
    for name in sorted(os.listdir(folder)):
        if name.endswith(".yml"):
            names.append(os.path.join(folder, name))
    return names
