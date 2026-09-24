// Vida viewer, natural scene: the land.
//
// Inside the simulated world the height of the ground comes from the
// terrain file (to scale). Outside it, where Vida doesn't simulate
// anything, the land eases into low rolling hills, high enough to hold the
// water in.
//
// The ground is also darker, and the grass thinner, where the simulation's
// canopies shade it: the "shade map" is the light that would reach the
// ground straight down through every canopy above it (each canopy lets
// through its species' canopyTransmittance).

"use strict";

var SHADE_MAP_CELLS = 256;
var HEIGHT_MAP_CELLS = 256;

function smoothStep(from, to, value) {
  var t = Math.max(0, Math.min(1, (value - from) / (to - from)));
  return t * t * (3 - 2 * t);
}

function terrainHeightFunction(header) {
  // The height of the ground from the terrain grid (see Vida_Data/vjson.py),
  // smoothly in between the points of the grid, and level beyond the edges.
  // x and y are Vida's (east and north).
  var terrain = header.terrain;
  var world = header.worldSize;
  if (!terrain) {
    return function flat() { return 0; };
  }
  var cells = terrain.cells;
  var size = terrain.cellSize;
  function at(row, column) {
    row = Math.max(0, Math.min(cells - 1, row));
    column = Math.max(0, Math.min(cells - 1, column));
    return terrain.elevation[row][column] || 0;
  }
  function height(x, y) {
    var column = Math.max(0, Math.min(cells - 1, (x + world / 2) / size));
    var row = Math.max(0, Math.min(cells - 1, (y + world / 2) / size));
    var c0 = Math.floor(column);
    var r0 = Math.floor(row);
    var fc = column - c0;
    var fr = row - r0;
    var low = at(r0, c0) * (1 - fc) + at(r0, c0 + 1) * fc;
    var high = at(r0 + 1, c0) * (1 - fc) + at(r0 + 1, c0 + 1) * fc;
    return low * (1 - fr) + high * fr;
  }
  return height;
}

function groundHeightFunction(header, highestWater) {
  // The height of the ground anywhere: the terrain inside the world, and
  // hills outside it.
  var world = header.worldSize;
  var inside = terrainHeightFunction(header);
  var edgeTotal = 0;
  for (var i = 0; i < 40; i++) {
    var along = -world / 2 + (i + 0.5) / 40 * world;
    edgeTotal += inside(along, -world / 2) + inside(along, world / 2) + inside(-world / 2, along) + inside(world / 2, along);
  }
  var surroundings = Math.max(edgeTotal / 160, highestWater + 0.6);
  function height(x, y) {
    var h = inside(x, y);
    var outside = Math.max(Math.abs(x), Math.abs(y)) - world / 2;
    if (outside <= 0) {
      return h;
    }
    var hills = (Math.sin(x * 0.045 + Math.sin(y * 0.03) * 2.1) + Math.sin(y * 0.052 + Math.sin(x * 0.027) * 1.7)) * world * 0.014;
    var eased = h + (surroundings - h) * smoothStep(0, world * 0.28, outside);
    return eased + Math.max(0, hills + world * 0.01) * smoothStep(world * 0.08, world * 0.6, outside);
  }
  return height;
}

function makeGroundMaterial(state, textures) {
  // The ground's colour is worked out in the shader, point by point: grass
  // in patches, drier grass outside the world, soil on slopes, rock on
  // steep ones, dark wet soil at the water's edge, sand and mud under the
  // water with the flickering light patterns (caustics) that ripples
  // focus on the bottom, and darker ground under the trees.
  var material = new THREE.MeshStandardMaterial({
    map: textures.colour,
    normalMap: textures.normal,
    normalScale: new THREE.Vector2(0.7, 0.7),
    roughness: 0.95,
    metalness: 0,
    envMapIntensity: 0.4
  });
  function onBeforeCompile(shader) {
    shader.uniforms.time = state.uniforms.time;
    shader.uniforms.waterLevel = state.uniforms.waterLevel;
    shader.uniforms.worldSize = state.uniforms.worldSize;
    shader.uniforms.shadeMap = state.uniforms.shadeMap;
    shader.uniforms.sunColour = state.uniforms.sunColour;
    shader.vertexShader = shader.vertexShader
      .replace("#include <common>", "#include <common>\nvarying vec3 vGroundPlace;\nvarying vec3 vGroundNormal;")
      .replace("#include <begin_vertex>", "#include <begin_vertex>\nvGroundPlace = (modelMatrix * vec4(transformed, 1.0)).xyz;\nvGroundNormal = normalize(mat3(modelMatrix) * objectNormal);");
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <common>", [
        "#include <common>",
        "varying vec3 vGroundPlace;",
        "varying vec3 vGroundNormal;",
        "uniform float time;",
        "uniform float waterLevel;",
        "uniform float worldSize;",
        "uniform sampler2D shadeMap;",
        "uniform vec3 sunColour;",
        "float groundWetness = 0.0;",
        SCENE_NOISE_GLSL,
        SHADE_LOOKUP_GLSL
      ].join("\n"))
      .replace("#include <map_fragment>", [
        "vec3 place = vGroundPlace;",
        "float slope = 1.0 - clamp(vGroundNormal.y, 0.0, 1.0);",
        "float patches = sceneFbm(place.xz * 0.07);",
        "float grain = sceneFbm(place.xz * 0.9);",
        "vec3 colour = mix(vec3(0.09, 0.17, 0.035), vec3(0.19, 0.28, 0.07), smoothstep(0.3, 0.72, patches));",
        "float outside = max(abs(place.x), abs(place.z)) - worldSize * 0.5;",
        "colour = mix(colour, vec3(0.32, 0.29, 0.13), smoothstep(0.0, worldSize * 0.3, outside) * 0.55 + smoothstep(0.62, 0.82, patches) * 0.2);",
        "colour = mix(colour, vec3(0.2, 0.13, 0.07), smoothstep(0.16, 0.34, slope + (grain - 0.5) * 0.2));",
        "colour = mix(colour, vec3(0.3, 0.29, 0.27), smoothstep(0.36, 0.55, slope + (grain - 0.5) * 0.15));",
        "vec4 detail = texture2D(map, vUv);",
        "colour *= 0.55 + 0.8 * detail.r;",
        // shade of the canopies above, from the simulation
        "float light = shadeLightAt(place.xz);",
        "colour *= 0.42 + 0.58 * pow(light, 0.3);",
        // the water's edge and the bottom
        "float above = place.y - waterLevel;",
        "if (waterLevel > -500.0) {",
        "  groundWetness = 1.0 - smoothstep(0.0, 0.4, above + (grain - 0.5) * 0.12);",
        "  colour = mix(colour, colour * 0.42, groundWetness * 0.85);",
        "  if (above < 0.0) {",
        "    float depth = -above;",
        "    colour = mix(vec3(0.12, 0.11, 0.06), vec3(0.04, 0.05, 0.035), smoothstep(0.0, 1.5, depth)) * (0.75 + 0.5 * grain);",
        "    float c1 = sceneCells(place.xz * 1.3, time * 1.1);",
        "    float c2 = sceneCells(place.xz * 1.9 + 7.0, time * 1.4);",
        "    float caustic = pow(1.0 - c1, 7.0) + pow(1.0 - c2, 7.0);",
        "    colour += sunColour * caustic * 0.16 * exp(-depth * 0.7);",
        "  }",
        "}",
        "diffuseColor.rgb = colour;"
      ].join("\n"))
      .replace("#include <roughnessmap_fragment>", "#include <roughnessmap_fragment>\nroughnessFactor = mix(roughnessFactor, 0.28, groundWetness);");
  }
  material.onBeforeCompile = onBeforeCompile;
  return material;
}

// The light reaching the ground at a point (three.js x and z), from the
// shade map, which covers the simulated world; outside it there is full light.
var SHADE_LOOKUP_GLSL = [
  "float shadeLightAt(vec2 xz) {",
  "  vec2 uv = vec2(xz.x / worldSize + 0.5, -xz.y / worldSize + 0.5);",
  "  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {",
  "    return 1.0;",
  "  }",
  "  return texture2D(shadeMap, uv).r;",
  "}"
].join("\n");

function makeShadeMap() {
  var data = new Uint8Array(SHADE_MAP_CELLS * SHADE_MAP_CELLS * 4);
  data.fill(255);
  var texture = new THREE.DataTexture(data, SHADE_MAP_CELLS, SHADE_MAP_CELLS, THREE.RGBAFormat);
  texture.magFilter = THREE.LinearFilter;
  texture.minFilter = THREE.LinearFilter;
  texture.needsUpdate = true;
  return texture;
}

function updateShadeMap(state, theRun, cycle) {
  // The light that reaches the ground straight down through the canopies
  // of this cycle (each lets through its species' canopyTransmittance),
  // softened a little at the edges.
  var cells = SHADE_MAP_CELLS;
  var world = theRun.header.worldSize;
  var size = world / cells;
  var light = new Float32Array(cells * cells);
  light.fill(1);
  var index = theRun.fieldIndex;
  var plants = cycle.plants;
  for (var p = 0; p < plants.length; p++) {
    var plant = plants[p];
    var radius = plant[index.canopyRadius];
    if (!(radius > 0)) {
      continue;
    }
    var species = theRun.species[plant[index.species]];
    var through = species && species.canopyTransmittance !== null && species.canopyTransmittance !== undefined ? species.canopyTransmittance : 0.05;
    var x = plant[index.x];
    var y = plant[index.y];
    var firstColumn = Math.max(0, Math.floor((x - radius + world / 2) / size));
    var lastColumn = Math.min(cells - 1, Math.floor((x + radius + world / 2) / size));
    var firstRow = Math.max(0, Math.floor((y - radius + world / 2) / size));
    var lastRow = Math.min(cells - 1, Math.floor((y + radius + world / 2) / size));
    for (var row = firstRow; row <= lastRow; row++) {
      var cy = -world / 2 + (row + 0.5) * size - y;
      for (var column = firstColumn; column <= lastColumn; column++) {
        var cx = -world / 2 + (column + 0.5) * size - x;
        if (cx * cx + cy * cy <= radius * radius) {
          light[row * cells + column] *= through;
        }
      }
    }
  }
  var data = state.uniforms.shadeMap.value.image.data;
  for (var r = 0; r < cells; r++) {
    for (var c = 0; c < cells; c++) {
      // a small blur, so the edge of the shade is soft
      var total = 0;
      var count = 0;
      for (var dr = -1; dr <= 1; dr++) {
        for (var dc = -1; dc <= 1; dc++) {
          var rr = r + dr;
          var cc = c + dc;
          if (rr >= 0 && rr < cells && cc >= 0 && cc < cells) {
            total += light[rr * cells + cc];
            count += 1;
          }
        }
      }
      var value = Math.round(total / count * 255);
      var at = (r * cells + c) * 4;
      data[at] = value;
      data[at + 1] = value;
      data[at + 2] = value;
      data[at + 3] = 255;
    }
  }
  state.uniforms.shadeMap.value.needsUpdate = true;
  state.shadeLight = light;
  state.shadeCells = cells;
}

function shadeLightAtPoint(state, x, y) {
  // the same light, read in JavaScript (Vida's x and y)
  if (!state.shadeLight) {
    return 1;
  }
  var world = state.world;
  var cells = state.shadeCells;
  var column = Math.floor((x + world / 2) / world * cells);
  var row = Math.floor((y + world / 2) / world * cells);
  if (column < 0 || row < 0 || column >= cells || row >= cells) {
    return 1;
  }
  return state.shadeLight[row * cells + column];
}

function buildGround(state, header, highestWater) {
  // the land as one big mesh, the height map the water reads, and the edge
  // line of the simulated world
  var world = header.worldSize;
  var margin = world * 0.9;
  var size = world + 2 * margin;
  var step = world / 150;
  if (header.terrain) {
    step = Math.max(header.terrain.cellSize / 2, size / 420);
  }
  var across = Math.min(460, Math.ceil(size / step));
  var height = groundHeightFunction(header, highestWater);
  state.groundHeight = height;
  state.groundSize = size;
  var positions = [];
  var uvs = [];
  var indices = [];
  for (var j = 0; j <= across; j++) {
    for (var i = 0; i <= across; i++) {
      var x = -size / 2 + i / across * size;
      var y = -size / 2 + j / across * size;
      positions.push(x, height(x, y), -y);
      uvs.push(x / 2.5, y / 2.5);
    }
  }
  for (var row = 0; row < across; row++) {
    for (var column = 0; column < across; column++) {
      var a = row * (across + 1) + column;
      var b = a + 1;
      var c = a + across + 1;
      var d = c + 1;
      // counter-clockwise seen from above, so the ground faces up
      indices.push(a, b, c, b, d, c);
    }
  }
  var geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("uv", new THREE.Float32BufferAttribute(uvs, 2));
  geometry.setIndex(indices);
  geometry.computeVertexNormals();
  if (state.ground) {
    state.scene.remove(state.ground);
    state.ground.geometry.dispose();
  }
  var ground = new THREE.Mesh(geometry, state.materials.ground);
  ground.receiveShadow = true;
  state.scene.add(ground);
  state.ground = ground;

  // the height of the ground as a texture, for the water to know how deep it is
  var cells = HEIGHT_MAP_CELLS;
  var data = new Uint16Array(cells * cells * 4);
  for (var r = 0; r < cells; r++) {
    for (var q = 0; q < cells; q++) {
      var hx = -size / 2 + (q + 0.5) / cells * size;
      var hy = size / 2 - (r + 0.5) / cells * size;
      var half = THREE.DataUtils.toHalfFloat(height(hx, hy));
      var at = (r * cells + q) * 4;
      data[at] = half;
      data[at + 1] = half;
      data[at + 2] = half;
      data[at + 3] = THREE.DataUtils.toHalfFloat(1);
    }
  }
  var heightMap = new THREE.DataTexture(data, cells, cells, THREE.RGBAFormat, THREE.HalfFloatType);
  heightMap.magFilter = THREE.LinearFilter;
  heightMap.minFilter = THREE.LinearFilter;
  heightMap.needsUpdate = true;
  if (state.uniforms.heightMap.value) {
    state.uniforms.heightMap.value.dispose();
  }
  state.uniforms.heightMap.value = heightMap;
  state.uniforms.heightMapSize.value = size;

  // low hills far away all round, fading into the haze
  if (state.farHills) {
    state.scene.remove(state.farHills);
    state.farHills.geometry.dispose();
  }
  var ringPositions = [];
  var ringColours = [];
  var ringIndices = [];
  var segments = 180;
  var rings = 12;
  var near = new THREE.Color(0.1, 0.16, 0.06);
  var far = new THREE.Color(0.2, 0.26, 0.28);
  var shade = new THREE.Color();
  var baseHeight = height(size / 2, 0);
  for (var ringNumber = 0; ringNumber <= rings; ringNumber++) {
    var fraction = ringNumber / rings;
    var ringRadius = size * 0.62 + fraction * world * 5;
    for (var segment = 0; segment <= segments; segment++) {
      var angle = segment / segments * Math.PI * 2;
      var bumps = 0.55 + 0.3 * Math.sin(angle * 5 + 1.3) + 0.2 * Math.sin(angle * 13 + 0.4) + 0.12 * Math.sin(angle * 29 + 2.2);
      var rise = world * 0.2 * Math.pow(fraction, 1.3) * bumps;
      ringPositions.push(Math.cos(angle) * ringRadius, baseHeight - 3 + rise, Math.sin(angle) * ringRadius);
      shade.copy(near).lerp(far, fraction);
      ringColours.push(shade.r, shade.g, shade.b);
    }
  }
  for (var rn = 0; rn < rings; rn++) {
    for (var sg = 0; sg < segments; sg++) {
      var p0 = rn * (segments + 1) + sg;
      var p1 = p0 + 1;
      var p2 = p0 + segments + 1;
      var p3 = p2 + 1;
      ringIndices.push(p0, p2, p1, p1, p2, p3);
    }
  }
  var ringGeometry = new THREE.BufferGeometry();
  ringGeometry.setAttribute("position", new THREE.Float32BufferAttribute(ringPositions, 3));
  ringGeometry.setAttribute("color", new THREE.Float32BufferAttribute(ringColours, 3));
  ringGeometry.setIndex(ringIndices);
  ringGeometry.computeVertexNormals();
  var farHills = new THREE.Mesh(ringGeometry, state.materials.farHills);
  state.scene.add(farHills);
  state.farHills = farHills;

  // a faint line round the edge of the simulated world
  if (state.edge) {
    state.scene.remove(state.edge);
    state.edge.geometry.dispose();
  }
  var edgePoints = [];
  var corners = [[-1, -1], [1, -1], [1, 1], [-1, 1], [-1, -1]];
  for (var k = 0; k < 4; k++) {
    for (var s = 0; s < 60; s++) {
      var t = s / 60;
      var ex = (corners[k][0] + (corners[k + 1][0] - corners[k][0]) * t) * world / 2;
      var ey = (corners[k][1] + (corners[k + 1][1] - corners[k][1]) * t) * world / 2;
      edgePoints.push(new THREE.Vector3(ex, height(ex, ey) + 0.06, -ey));
    }
  }
  edgePoints.push(edgePoints[0].clone());
  var edge = new THREE.Line(new THREE.BufferGeometry().setFromPoints(edgePoints), state.materials.edge);
  state.scene.add(edge);
  state.edge = edge;
}
