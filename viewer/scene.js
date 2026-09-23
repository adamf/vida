// Vida viewer: the natural scene, drawn in 3D with three.js (lib/three.min.js).
//
// What comes from the simulation is drawn to scale:
//   - where each plant stands, and the height of the ground under it;
//   - the height and width of its stem (a cylinder, as in Vida's 3D files);
//   - the size and shape of its crown: the top half of a sphere, as wide as
//     the canopy radius, whose top is the top of the stem. It is as deep as
//     it is wide, or, for species with crownShape PARA, reaches from
//     boleHeight percent of the way down the tree to the top (again as in
//     Vida's 3D files);
//   - the shape of the ground (from a terrain file) and the water level;
//   - how much light each plant got, which darkens its leaves.
// The leaves, bark and grass themselves, and the colour of each species'
// leaves (from its genus: pines, oaks, maples...), are decoration, to make
// it look like a real place. So is the angle of the sun: Vida's light comes
// straight down.
//
// Coordinates: Vida's x is east, y is north and height is up. In three.js,
// y is up, so a point (x, y, height) in Vida is (x, height, -y) here.
//
// The main pieces, in order:
//   textures       leaves, needles, bark and grass, drawn once on canvases
//   trees          turning each plant into a stem and a crown of leaf clumps
//   ground         the land, the water and the sky
//   camera         turning, zooming and moving with a mouse or fingers
//   the scene      setting up, showing a cycle, and drawing

"use strict";

var scene3d = null;   // everything the scene needs, made by sceneStart

// Leaf colours by genus (the first word of the species name), in summer.
// Needles are drawn for conifers. Anything not listed is a broadleaf tree.
var FOLIAGE = {
  Pinus: { colour: "#4e6b2e", needles: true },
  Picea: { colour: "#2f5238", needles: true },
  Abies: { colour: "#2c5236", needles: true },
  Tsuga: { colour: "#355a36", needles: true },
  Juniperus: { colour: "#4d6344", needles: true },
  Taxodium: { colour: "#5b7a34", needles: true },
  Gymnosperm: { colour: "#3f6035", needles: true },
  Acer: { colour: "#4f7f2f", needles: false },
  Quercus: { colour: "#46672a", needles: false },
  Carya: { colour: "#5d7f2d", needles: false },
  Cornus: { colour: "#5b8537", needles: false },
  Liquidambar: { colour: "#4d7c2b", needles: false },
  Liriodendron: { colour: "#5a8a31", needles: false },
  Magnolia: { colour: "#3c5f2c", needles: false },
  Nyssa: { colour: "#46702d", needles: false },
  Prunus: { colour: "#4b6f2a", needles: false },
  Fagus: { colour: "#5a7f33", needles: false },
  Betula: { colour: "#6a8c35", needles: false },
  Castanea: { colour: "#4f732c", needles: false },
  Angiosperm: { colour: "#52792f", needles: false }
};
var DEFAULT_FOLIAGE = { colour: "#52792f", needles: false };

var SKY_TOP = "#4f86c6";
var SKY_HORIZON = "#d9e6ef";
var SUN_COLOUR = "#fff1dc";
var CLUMP_BUDGET = 7000;      // leaf clumps in the whole scene, roughly
if (typeof navigator !== "undefined" && /Mobi|Android|iPhone|iPad/.test(navigator.userAgent)) {
  CLUMP_BUDGET = 3500;
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

function makeRandom(seed) {
  // A small random number generator that gives the same numbers for the same
  // seed, so a tree looks the same from one cycle to the next.
  var state = seed >>> 0;
  function next() {
    state = (state + 0x6D2B79F5) >>> 0;
    var t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  }
  return next;
}

function seedFor(x, y) {
  // plants don't move, so their position identifies them
  return (Math.round(x * 1000) * 73856093) ^ (Math.round(y * 1000) * 19349663);
}

function foliageFor(speciesName) {
  var name = String(speciesName || "");
  var words = name.split(/[_\s]+/);
  for (var i = 0; i < words.length; i++) {
    if (FOLIAGE[words[i]]) {
      return FOLIAGE[words[i]];
    }
  }
  return DEFAULT_FOLIAGE;
}

function colourFromHsv(hsv) {
  // Vida stores colours as [hue in degrees, saturation 0-1, brightness 0-1]
  var hue = ((hsv[0] % 360) + 360) % 360 / 60;
  var s = hsv[1];
  var v = hsv[2];
  var c = v * s;
  var x = c * (1 - Math.abs((hue % 2) - 1));
  var r = 0;
  var g = 0;
  var b = 0;
  if (hue < 1) { r = c; g = x; } else if (hue < 2) { r = x; g = c; } else if (hue < 3) { g = c; b = x; }
  else if (hue < 4) { g = x; b = c; } else if (hue < 5) { r = x; b = c; } else { r = c; b = x; }
  var m = v - c;
  var colour = new THREE.Color();
  colour.setRGB(r + m, g + m, b + m);
  colour.convertSRGBToLinear();
  return colour;
}

// ---------------------------------------------------------------------------
// Textures, drawn once on canvases
// ---------------------------------------------------------------------------

function canvasTexture(canvas, repeat) {
  var texture = new THREE.CanvasTexture(canvas);
  texture.encoding = THREE.sRGBEncoding;
  texture.anisotropy = 4;
  if (repeat) {
    texture.wrapS = THREE.RepeatWrapping;
    texture.wrapT = THREE.RepeatWrapping;
  }
  return texture;
}

function drawLeaf(context, x, y, length, width, angle, shade) {
  // one leaf: a pointed oval with a paler midrib
  context.save();
  context.translate(x, y);
  context.rotate(angle);
  context.fillStyle = "rgb(" + shade + "," + shade + "," + shade + ")";
  context.beginPath();
  context.moveTo(0, -length / 2);
  context.bezierCurveTo(width, -length / 4, width, length / 4, 0, length / 2);
  context.bezierCurveTo(-width, length / 4, -width, -length / 4, 0, -length / 2);
  context.fill();
  context.strokeStyle = "rgba(255,255,255,0.35)";
  context.lineWidth = 1.2;
  context.beginPath();
  context.moveTo(0, -length / 2 + 2);
  context.lineTo(0, length / 2 - 2);
  context.stroke();
  context.restore();
}

function leafTexture() {
  // A clump of leaves, in greys: the tree's own colour is multiplied in.
  var canvas = document.createElement("canvas");
  canvas.width = 256;
  canvas.height = 256;
  var context = canvas.getContext("2d");
  var random = makeRandom(7);
  for (var i = 0; i < 34; i++) {
    var angle = random() * Math.PI * 2;
    var distance = Math.sqrt(random()) * 92;
    var x = 128 + Math.cos(angle) * distance;
    var y = 128 + Math.sin(angle) * distance;
    var shade = 150 + Math.floor(random() * 105);
    drawLeaf(context, x, y, 46 + random() * 22, 16 + random() * 8, random() * Math.PI * 2, shade);
  }
  return canvasTexture(canvas, false);
}

function needleTexture() {
  // A spray of pine needles: thin strokes fanning out from little twigs.
  var canvas = document.createElement("canvas");
  canvas.width = 256;
  canvas.height = 256;
  var context = canvas.getContext("2d");
  var random = makeRandom(11);
  context.lineCap = "round";
  for (var twig = 0; twig < 16; twig++) {
    var angle = random() * Math.PI * 2;
    var distance = Math.sqrt(random()) * 80;
    var x = 128 + Math.cos(angle) * distance;
    var y = 128 + Math.sin(angle) * distance;
    for (var n = 0; n < 26; n++) {
      var needleAngle = random() * Math.PI * 2;
      var length = 18 + random() * 22;
      var shade = 140 + Math.floor(random() * 115);
      context.strokeStyle = "rgb(" + shade + "," + shade + "," + shade + ")";
      context.lineWidth = 2.2;
      context.beginPath();
      context.moveTo(x, y);
      context.lineTo(x + Math.cos(needleAngle) * length, y + Math.sin(needleAngle) * length);
      context.stroke();
    }
  }
  return canvasTexture(canvas, false);
}

function barkTexture() {
  // Vertical furrows, in greys.
  var canvas = document.createElement("canvas");
  canvas.width = 64;
  canvas.height = 256;
  var context = canvas.getContext("2d");
  context.fillStyle = "rgb(200,200,200)";
  context.fillRect(0, 0, 64, 256);
  var random = makeRandom(5);
  for (var i = 0; i < 90; i++) {
    var shade = 90 + Math.floor(random() * 140);
    context.fillStyle = "rgb(" + shade + "," + shade + "," + shade + ")";
    var x = random() * 64;
    var width = 1 + random() * 4;
    var y = random() * 256;
    context.fillRect(x, y, width, 20 + random() * 120);
  }
  var texture = canvasTexture(canvas, true);
  return texture;
}

function grassTexture() {
  // Speckles of grass and soil, in greys; the ground's colour is multiplied in.
  var canvas = document.createElement("canvas");
  canvas.width = 256;
  canvas.height = 256;
  var context = canvas.getContext("2d");
  context.fillStyle = "rgb(205,205,205)";
  context.fillRect(0, 0, 256, 256);
  var random = makeRandom(3);
  for (var i = 0; i < 2600; i++) {
    var shade = 140 + Math.floor(random() * 115);
    context.strokeStyle = "rgba(" + shade + "," + shade + "," + shade + ",0.8)";
    context.lineWidth = 1;
    var x = random() * 256;
    var y = random() * 256;
    var lean = (random() - 0.5) * 3;
    context.beginPath();
    context.moveTo(x, y);
    context.lineTo(x + lean, y - 3 - random() * 5);
    context.stroke();
  }
  return canvasTexture(canvas, true);
}

// ---------------------------------------------------------------------------
// Trees
// ---------------------------------------------------------------------------

function clumpCoreGeometry() {
  // The inside of a clump of leaves: a lumpy ball, darker than the leaves,
  // so there are no holes to see through.
  var geometry = new THREE.IcosahedronGeometry(0.58, 1);
  var position = geometry.attributes.position;
  var point = new THREE.Vector3();
  for (var i = 0; i < position.count; i++) {
    point.fromBufferAttribute(position, i);
    // a smooth bump that depends only on where the point is, so the copies of
    // a corner shared by two triangles move together
    var bump = 1 + 0.12 * Math.sin(3.1 * point.x + 1.3) * Math.sin(3.7 * point.y + 0.4) * Math.sin(2.9 * point.z + 2.1);
    point.multiplyScalar(bump);
    position.setXYZ(i, point.x, point.y, point.z);
  }
  geometry.computeVertexNormals();
  return geometry;
}

function clumpLeavesGeometry(cards, cardSize, seed) {
  // Many small squares ("cards") with leaves drawn on them, scattered over a
  // ball of radius 1, facing roughly outwards. Their normals point straight
  // out from the centre, so the clump is lit like a rounded mass of leaves.
  var random = makeRandom(seed);
  var positions = [];
  var normals = [];
  var uvs = [];
  var indices = [];
  var outward = new THREE.Vector3();
  var facing = new THREE.Vector3();
  var across = new THREE.Vector3();
  var up = new THREE.Vector3();
  var centre = new THREE.Vector3();
  var helper = new THREE.Vector3(0, 1, 0);
  var lit = new THREE.Vector3();
  var skyward = new THREE.Vector3(0, 0.55, 0);
  for (var i = 0; i < cards; i++) {
    // a direction evenly spread over the ball
    var z = random() * 2 - 1;
    var angle = random() * Math.PI * 2;
    var ring = Math.sqrt(1 - z * z);
    outward.set(ring * Math.cos(angle), z, ring * Math.sin(angle));
    centre.copy(outward).multiplyScalar(0.5 + random() * 0.45);
    // the direction the card is lit from: outwards, tipped towards the sky
    lit.copy(outward).multiplyScalar(0.7).add(skyward).normalize();
    // tip the card a little away from facing straight out
    facing.set(random() - 0.5, random() - 0.5, random() - 0.5).multiplyScalar(0.9).add(outward).normalize();
    if (Math.abs(facing.y) > 0.95) {
      helper.set(1, 0, 0);
    } else {
      helper.set(0, 1, 0);
    }
    across.crossVectors(helper, facing).normalize();
    up.crossVectors(facing, across).normalize();
    var spin = random() * Math.PI * 2;
    var cosine = Math.cos(spin) * cardSize / 2;
    var sine = Math.sin(spin) * cardSize / 2;
    var corners = [[-1, -1], [1, -1], [1, 1], [-1, 1]];
    var start = positions.length / 3;
    for (var c = 0; c < 4; c++) {
      var a = corners[c][0];
      var b = corners[c][1];
      positions.push(
        centre.x + across.x * (a * cosine - b * sine) + up.x * (a * sine + b * cosine),
        centre.y + across.y * (a * cosine - b * sine) + up.y * (a * sine + b * cosine),
        centre.z + across.z * (a * cosine - b * sine) + up.z * (a * sine + b * cosine));
      normals.push(lit.x, lit.y, lit.z);
      uvs.push((a + 1) / 2, (b + 1) / 2);
    }
    indices.push(start, start + 1, start + 2, start, start + 2, start + 3);
  }
  var geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("normal", new THREE.Float32BufferAttribute(normals, 3));
  geometry.setAttribute("uv", new THREE.Float32BufferAttribute(uvs, 2));
  geometry.setIndex(indices);
  return geometry;
}

function crownDepth(species, stemHeight, canopyRadius) {
  // how far down from the top of the stem the crown reaches (see the top of the file)
  if (species && species.crownShape === "PARA" && species.boleHeight !== null && species.boleHeight !== undefined) {
    return stemHeight * species.boleHeight / 100;
  }
  return canopyRadius;
}

function crownClumps(x, top, z, radius, depth, clumpBudget) {
  // Where to put the clumps of leaves that make up a crown, as
  // [x, y, z, clump radius]. Together they fill the top half of an
  // ellipsoid that is `radius` wide and `depth` deep, with its top at `top`,
  // layer by layer from the bottom, each layer as wide as the crown is at
  // that height. The same plant always gets the same clumps.
  // clumpBudget is roughly how many clumps this crown may have.
  var random = makeRandom(seedFor(x, z));
  var size = Math.max(0.04, Math.min(1.6, Math.min(radius, depth) * 0.6));
  // bigger clumps for big crowns when there are a lot of trees to draw
  var needed = estimateClumps(radius, depth, size);
  if (needed > clumpBudget) {
    size = size * Math.sqrt(needed / clumpBudget);
    size = Math.min(size, Math.min(radius, depth));
  }
  var base = top - depth;
  var clumps = [];
  var step = size * 0.9;
  var height = base + Math.min(depth * 0.5, size * 0.75);
  while (height < top - size * 0.9) {
    addLayer(clumps, random, x, z, height, widthAt(radius, depth, height - base), size);
    height += step * (0.85 + random() * 0.3);
  }
  // the very top
  clumps.push([x, top - size, z, size]);
  return clumps;
}

function widthAt(radius, depth, above) {
  // the radius of the crown at a height `above` its bottom
  var fraction = Math.max(0, Math.min(1, above / depth));
  return radius * Math.sqrt(1 - fraction * fraction);
}

function estimateClumps(radius, depth, size) {
  var total = 1;
  var step = size * 0.9;
  for (var above = Math.min(depth * 0.5, size * 0.75); above < depth - size * 0.9; above += step) {
    total += layerCount(widthAt(radius, depth, above), size);
  }
  return total;
}

function layerCount(width, size) {
  if (width <= size * 1.15) {
    return 1;
  }
  var ring = Math.max(3, Math.round(2 * Math.PI * (width - size) / (size * 1.3)));
  if (width > size * 2.6) {
    return ring + 1;
  }
  return ring;
}

function addLayer(clumps, random, x, z, height, width, size) {
  // one layer of clumps: a ring round the edge of the crown at this height,
  // and one in the middle if the crown is wide there
  var count = layerCount(width, size);
  if (count === 1) {
    var small = Math.max(0.03, Math.min(size, width));
    clumps.push([x + (random() - 0.5) * 0.2 * small, height, z + (random() - 0.5) * 0.2 * small, small]);
    return;
  }
  var ring = count;
  if (width > size * 2.6) {
    ring = count - 1;
    clumps.push([x, height + (random() - 0.5) * size * 0.3, z, size]);
  }
  var reach = width - size;
  var phase = random() * Math.PI * 2;
  for (var i = 0; i < ring; i++) {
    var angle = phase + (i + (random() - 0.5) * 0.4) / ring * Math.PI * 2;
    var along = reach * (0.9 + random() * 0.1);
    clumps.push([x + Math.cos(angle) * along, height + (random() - 0.5) * size * 0.6,
      z + Math.sin(angle) * along, size * (0.8 + random() * 0.2)]);
  }
}

function ensureCapacity(state, key, needed) {
  // InstancedMesh has a fixed size; make a bigger one when there are more trees
  var mesh = state.meshes[key];
  if (mesh.userData.capacity >= needed) {
    return mesh;
  }
  var capacity = Math.max(needed, Math.ceil(mesh.userData.capacity * 1.6));
  var bigger = new THREE.InstancedMesh(mesh.geometry, mesh.material, capacity);
  bigger.userData.capacity = capacity;
  bigger.setColorAt(0, new THREE.Color(1, 1, 1));
  bigger.castShadow = mesh.castShadow;
  bigger.receiveShadow = mesh.receiveShadow;
  bigger.customDepthMaterial = mesh.customDepthMaterial;
  bigger.frustumCulled = false;
  state.scene.remove(mesh);
  mesh.dispose();
  state.scene.add(bigger);
  state.meshes[key] = bigger;
  return bigger;
}

function makeInstanced(state, key, geometry, material, castShadow) {
  var mesh = new THREE.InstancedMesh(geometry, material, 64);
  mesh.userData.capacity = 64;
  // give it colours now: three.js makes its shaders the first time it draws,
  // and only uses the colours if there were some then
  mesh.setColorAt(0, new THREE.Color(1, 1, 1));
  mesh.count = 0;
  mesh.castShadow = castShadow;
  mesh.receiveShadow = true;
  mesh.frustumCulled = false;
  state.scene.add(mesh);
  state.meshes[key] = mesh;
  return mesh;
}

// ---------------------------------------------------------------------------
// Ground, water and sky
// ---------------------------------------------------------------------------

function smoothStep(from, to, value) {
  var t = Math.max(0, Math.min(1, (value - from) / (to - from)));
  return t * t * (3 - 2 * t);
}

function groundHeightFunction(header, highestWater) {
  // The height of the ground at any x, y (Vida's coordinates). Inside the
  // world it comes from the terrain grid (see Vida_Data/vjson.py), smoothly
  // in between the points of the grid. Outside the world (which Vida doesn't
  // simulate) the land eases into low rolling hills, high enough to hold
  // the water in.
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
    var hills = (Math.sin(x * 0.045 + Math.sin(y * 0.03) * 2.1) + Math.sin(y * 0.052 + Math.sin(x * 0.027) * 1.7)) * world * 0.012;
    var eased = h + (surroundings - h) * smoothStep(0, world * 0.28, outside);
    return eased + hills * smoothStep(world * 0.1, world * 0.6, outside);
  }
  return height;
}

function terrainHeightFunction(header) {
  // the terrain grid alone: level beyond the edges of the world
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
    var column = (x + world / 2) / size;
    var row = (y + world / 2) / size;
    column = Math.max(0, Math.min(cells - 1, column));
    row = Math.max(0, Math.min(cells - 1, row));
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

function buildGround(state, header, highestWater) {
  var world = header.worldSize;
  var margin = world * 0.9;
  var size = world + 2 * margin;
  var step = world / 120;
  if (header.terrain) {
    step = Math.max(header.terrain.cellSize / 2, size / 360);
  }
  var across = Math.min(420, Math.ceil(size / step));
  var height = groundHeightFunction(header, highestWater);
  state.groundHeight = height;
  var positions = [];
  var colours = [];
  var uvs = [];
  var indices = [];
  var grassLight = new THREE.Color("#6a9340");
  var grassDark = new THREE.Color("#436a2b");
  var dry = new THREE.Color("#8f8e5c");
  var soil = new THREE.Color("#6b5a3e");
  var mixed = new THREE.Color();
  var random = makeRandom(19);
  var lowest = Infinity;
  var highest = -Infinity;
  for (var j = 0; j <= across; j++) {
    for (var i = 0; i <= across; i++) {
      var x = -size / 2 + i / across * size;
      var y = -size / 2 + j / across * size;
      var h = height(x, y);
      lowest = Math.min(lowest, h);
      highest = Math.max(highest, h);
      positions.push(x, h, -y);
      uvs.push(x / 3, y / 3);
      // patches of lighter and darker grass, soil on the steep parts, and
      // drier, paler grass outside the simulated world
      var patch = 0.5 + 0.25 * Math.sin(x * 0.37 + Math.sin(y * 0.21) * 2) + 0.25 * Math.sin(y * 0.31 + Math.sin(x * 0.17) * 2);
      mixed.copy(grassDark).lerp(grassLight, Math.max(0, Math.min(1, patch + (random() - 0.5) * 0.15)));
      var slope = Math.abs(height(x + 0.5, y) - h) + Math.abs(height(x, y + 0.5) - h);
      mixed.lerp(soil, Math.min(0.7, slope * 1.2));
      var outside = Math.max(Math.abs(x), Math.abs(y)) - world / 2;
      if (outside > 0) {
        mixed.lerp(dry, Math.min(0.55, outside / (world * 0.25)));
      }
      colours.push(mixed.r, mixed.g, mixed.b);
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
  geometry.setAttribute("color", new THREE.Float32BufferAttribute(colours, 3));
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
  state.groundLowest = lowest;
  state.groundHighest = highest;

  // the water: one flat sheet at the water level
  if (state.water) {
    state.scene.remove(state.water);
    state.water.geometry.dispose();
  }
  var water = new THREE.Mesh(new THREE.PlaneGeometry(world * 1.6, world * 1.6), state.materials.water);
  water.rotation.x = -Math.PI / 2;
  water.receiveShadow = true;
  water.visible = false;
  state.scene.add(water);
  state.water = water;

  // a faint line round the edge of the simulated world
  if (state.edge) {
    state.scene.remove(state.edge);
    state.edge.geometry.dispose();
  }
  var edgePoints = [];
  var corners = [[-1, -1], [1, -1], [1, 1], [-1, 1], [-1, -1]];
  for (var k = 0; k < 4; k++) {
    for (var s = 0; s < 40; s++) {
      var t = s / 40;
      var ex = (corners[k][0] + (corners[k + 1][0] - corners[k][0]) * t) * world / 2;
      var ey = (corners[k][1] + (corners[k + 1][1] - corners[k][1]) * t) * world / 2;
      edgePoints.push(new THREE.Vector3(ex, height(ex, ey) + 0.05, -ey));
    }
  }
  edgePoints.push(edgePoints[0].clone());
  var edge = new THREE.Line(new THREE.BufferGeometry().setFromPoints(edgePoints), state.materials.edge);
  state.scene.add(edge);
  state.edge = edge;

  // sky, fog and the sun's shadow box, sized to the world
  state.scene.fog.near = world * 1.6;
  state.scene.fog.far = world * 6.5;
  state.sky.scale.setScalar(world * 7);
  var shadowSize = world * 0.75;
  var shadowCamera = state.sun.shadow.camera;
  shadowCamera.left = -shadowSize;
  shadowCamera.right = shadowSize;
  shadowCamera.top = shadowSize;
  shadowCamera.bottom = -shadowSize;
  shadowCamera.near = 0.5;
  shadowCamera.far = world * 5;
  shadowCamera.updateProjectionMatrix();
  state.sun.position.copy(state.sunDirection).multiplyScalar(world * 2);
  state.sun.target.position.set(0, 0, 0);
  state.camera.far = world * 20;
  state.camera.updateProjectionMatrix();
}

function makeSky(state) {
  var material = new THREE.ShaderMaterial({
    uniforms: {
      topColour: { value: new THREE.Color(SKY_TOP) },
      horizonColour: { value: new THREE.Color(SKY_HORIZON) },
      sunDirection: { value: state.sunDirection }
    },
    vertexShader: [
      "varying vec3 direction;",
      "void main() {",
      "  direction = normalize(position);",
      "  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);",
      "}"
    ].join("\n"),
    fragmentShader: [
      "uniform vec3 topColour;",
      "uniform vec3 horizonColour;",
      "uniform vec3 sunDirection;",
      "varying vec3 direction;",
      "void main() {",
      "  float up = max(direction.y, 0.0);",
      "  vec3 colour = mix(horizonColour, topColour, pow(up, 0.55));",
      "  float sun = max(dot(normalize(direction), normalize(sunDirection)), 0.0);",
      "  colour += vec3(1.0, 0.93, 0.8) * (pow(sun, 900.0) * 3.0 + pow(sun, 18.0) * 0.18);",
      "  if (direction.y < 0.0) { colour = horizonColour; }",
      "  gl_FragColor = vec4(colour, 1.0);",
      "}"
    ].join("\n"),
    side: THREE.BackSide,
    depthWrite: false,
    fog: false
  });
  var sky = new THREE.Mesh(new THREE.SphereGeometry(1, 32, 16), material);
  sky.renderOrder = -1;
  sky.frustumCulled = false;
  return sky;
}

// ---------------------------------------------------------------------------
// Camera: drag to turn, pinch or scroll to zoom, two fingers or right-drag to move
// ---------------------------------------------------------------------------

function placeCamera(state) {
  var view = state.view;
  var flat = Math.cos(view.elevation) * view.distance;
  state.camera.position.set(
    view.target.x + Math.sin(view.azimuth) * flat,
    view.target.y + Math.sin(view.elevation) * view.distance,
    view.target.z + Math.cos(view.azimuth) * flat);
  // don't go under the ground
  var groundBelow = state.groundHeight(state.camera.position.x, -state.camera.position.z) + 1.0;
  if (state.camera.position.y < groundBelow) {
    state.camera.position.y = groundBelow;
  }
  state.camera.lookAt(view.target);
}

function resetView(state, header) {
  var world = header.worldSize;
  var middle = state.groundHeight(0, 0);
  // stand further back on a tall, narrow screen, so the whole world fits
  var narrow = Math.sqrt(Math.max(1, 1.2 / state.camera.aspect));
  state.view = {
    target: new THREE.Vector3(0, middle + 2, 0),
    distance: world * 1.3 * narrow,
    azimuth: 0.65,
    elevation: 0.48
  };
  placeCamera(state);
}

function attachControls(state, canvas) {
  var pointers = {};
  var lastPinch = 0;
  var lastMiddle = null;

  function count() {
    return Object.keys(pointers).length;
  }
  function middleOf() {
    var keys = Object.keys(pointers);
    var x = 0;
    var y = 0;
    for (var i = 0; i < keys.length; i++) {
      x += pointers[keys[i]].x;
      y += pointers[keys[i]].y;
    }
    return { x: x / keys.length, y: y / keys.length };
  }
  function pinchLength() {
    var keys = Object.keys(pointers);
    var a = pointers[keys[0]];
    var b = pointers[keys[1]];
    return Math.hypot(a.x - b.x, a.y - b.y);
  }
  function turn(dx, dy) {
    state.view.azimuth -= dx * 0.006;
    state.view.elevation = Math.max(0.03, Math.min(1.5, state.view.elevation + dy * 0.005));
  }
  function zoom(factor) {
    var world = state.world || 100;
    state.view.distance = Math.max(1.5, Math.min(world * 4, state.view.distance * factor));
  }
  function move(dx, dy) {
    // slide the point we look at across the ground
    var scale = state.view.distance * 0.0016;
    var sin = Math.sin(state.view.azimuth);
    var cos = Math.cos(state.view.azimuth);
    state.view.target.x -= (dx * cos + dy * sin) * scale;
    state.view.target.z -= (-dx * sin + dy * cos) * scale;
    var limit = (state.world || 100) * 0.9;
    state.view.target.x = Math.max(-limit, Math.min(limit, state.view.target.x));
    state.view.target.z = Math.max(-limit, Math.min(limit, state.view.target.z));
    state.view.target.y = state.groundHeight(state.view.target.x, -state.view.target.z) + 2;
  }
  function changed() {
    placeCamera(state);
    requestSceneDraw();
  }

  function onDown(event) {
    canvas.setPointerCapture(event.pointerId);
    pointers[event.pointerId] = { x: event.clientX, y: event.clientY, button: event.button, shift: event.shiftKey };
    if (count() === 2) {
      lastPinch = pinchLength();
      lastMiddle = middleOf();
    }
  }
  function onMove(event) {
    var pointer = pointers[event.pointerId];
    if (!pointer) {
      return;
    }
    var dx = event.clientX - pointer.x;
    var dy = event.clientY - pointer.y;
    if (count() === 1) {
      if (pointer.button === 2 || pointer.shift) {
        move(dx, dy);
      } else {
        turn(dx, dy);
      }
      pointer.x = event.clientX;
      pointer.y = event.clientY;
      changed();
    } else if (count() === 2) {
      pointer.x = event.clientX;
      pointer.y = event.clientY;
      var length = pinchLength();
      var middle = middleOf();
      if (lastPinch > 0) {
        zoom(lastPinch / length);
      }
      if (lastMiddle) {
        move(middle.x - lastMiddle.x, middle.y - lastMiddle.y);
      }
      lastPinch = length;
      lastMiddle = middle;
      changed();
    }
  }
  function onUp(event) {
    delete pointers[event.pointerId];
    lastPinch = 0;
    lastMiddle = null;
  }
  function onWheel(event) {
    event.preventDefault();
    zoom(Math.exp(event.deltaY * 0.0012));
    changed();
  }
  function onContextMenu(event) {
    event.preventDefault();
  }
  function onDoubleClick() {
    if (state.header) {
      resetView(state, state.header);
      requestSceneDraw();
    }
  }
  canvas.addEventListener("pointerdown", onDown);
  canvas.addEventListener("pointermove", onMove);
  canvas.addEventListener("pointerup", onUp);
  canvas.addEventListener("pointercancel", onUp);
  canvas.addEventListener("wheel", onWheel, { passive: false });
  canvas.addEventListener("contextmenu", onContextMenu);
  canvas.addEventListener("dblclick", onDoubleClick);
}

// ---------------------------------------------------------------------------
// The scene
// ---------------------------------------------------------------------------

function sceneStart(box) {
  // Set up the scene inside box (an element). Returns false if this browser
  // can't draw in 3D.
  if (scene3d !== null) {
    return true;
  }
  if (typeof THREE === "undefined") {
    return false;
  }
  var renderer;
  try {
    renderer = new THREE.WebGLRenderer({ antialias: true });
  } catch (error) {
    return false;
  }
  THREE.ColorManagement.legacyMode = false;
  renderer.outputEncoding = THREE.sRGBEncoding;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.0;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  box.appendChild(renderer.domElement);
  renderer.domElement.style.touchAction = "none";

  var state = {
    renderer: renderer,
    box: box,
    scene: new THREE.Scene(),
    camera: new THREE.PerspectiveCamera(42, 1, 0.05, 2000),
    meshes: {},
    materials: {},
    groundHeight: function flat() { return 0; },
    header: null,
    world: 100,
    drawPending: false
  };
  state.scene.fog = new THREE.Fog(new THREE.Color(SKY_HORIZON), 160, 650);
  // the sun: high in the south-east, a little behind the usual view
  state.sunDirection = new THREE.Vector3(0.45, 0.82, 0.36).normalize();
  var sun = new THREE.DirectionalLight(new THREE.Color(SUN_COLOUR), 2.4);
  sun.castShadow = true;
  var mapSize = Math.min(4096, renderer.capabilities.maxTextureSize);
  if (/Mobi|Android|iPhone|iPad/.test(navigator.userAgent)) {
    mapSize = Math.min(mapSize, 2048);
  }
  sun.shadow.mapSize.set(mapSize, mapSize);
  sun.shadow.bias = -0.0004;
  sun.shadow.normalBias = 0.03;
  state.scene.add(sun);
  state.scene.add(sun.target);
  state.sun = sun;
  state.scene.add(new THREE.HemisphereLight(new THREE.Color("#c9ddf0"), new THREE.Color("#5a5236"), 0.9));
  state.sky = makeSky(state);
  state.scene.add(state.sky);

  var leaves = leafTexture();
  var needles = needleTexture();
  var bark = barkTexture();
  var grass = grassTexture();
  state.materials.ground = new THREE.MeshStandardMaterial({ map: grass, vertexColors: true, roughness: 1.0, metalness: 0 });
  state.materials.water = new THREE.MeshStandardMaterial({ color: new THREE.Color("#2e5d6e"), roughness: 0.12, metalness: 0.1, transparent: true, opacity: 0.86 });
  state.materials.edge = new THREE.LineBasicMaterial({ color: new THREE.Color("#f4f1e2"), transparent: true, opacity: 0.35, fog: true });
  state.materials.stem = new THREE.MeshStandardMaterial({ map: bark, roughness: 0.95, metalness: 0 });

  var stemGeometry = new THREE.CylinderGeometry(1, 1, 1, 10, 1, false);
  stemGeometry.translate(0, 0.5, 0);
  makeInstanced(state, "stems", stemGeometry, state.materials.stem, true);

  var core = clumpCoreGeometry();
  var styles = [["broad", leaves, 40, 0.78], ["needle", needles, 42, 0.8]];
  for (var i = 0; i < styles.length; i++) {
    var key = styles[i][0];
    var texture = styles[i][1];
    var cardMaterial = new THREE.MeshStandardMaterial({ map: texture, alphaTest: 0.45, side: THREE.DoubleSide, roughness: 0.85, metalness: 0 });
    var coreMaterial = new THREE.MeshStandardMaterial({ roughness: 1.0, metalness: 0 });
    var cards = makeInstanced(state, key + "Leaves", clumpLeavesGeometry(styles[i][2], styles[i][3], 101 + i), cardMaterial, true);
    cards.customDepthMaterial = new THREE.MeshDepthMaterial({ depthPacking: THREE.RGBADepthPacking, map: texture, alphaTest: 0.45 });
    makeInstanced(state, key + "Core", core, coreMaterial, true);
  }
  attachControls(state, renderer.domElement);
  scene3d = state;
  sceneResize();
  return true;
}

function sceneSetRun(theRun) {
  // a new file: the ground, water and camera for its world
  if (scene3d === null) {
    return;
  }
  scene3d.header = theRun.header;
  scene3d.world = theRun.header.worldSize;
  var highestWater = 0;
  if (theRun.header.terrain) {
    for (var c = 0; c < theRun.cycles.length; c++) {
      if (theRun.cycles[c].waterLevel !== null && theRun.cycles[c].waterLevel > highestWater) {
        highestWater = theRun.cycles[c].waterLevel;
      }
    }
  }
  buildGround(scene3d, theRun.header, highestWater);
  resetView(scene3d, theRun.header);
  scene3d.foliage = [];
  scene3d.bark = [];
  for (var s = 0; s < theRun.species.length; s++) {
    var species = theRun.species[s];
    var foliage = foliageFor(species ? species.name : "");
    // each species a little different, so neighbouring species can be told apart
    var colour = new THREE.Color(foliage.colour);
    var random = makeRandom(s * 7919 + 17);
    var hsl = {};
    colour.getHSL(hsl);
    colour.setHSL(hsl.h + (random() - 0.5) * 0.03, hsl.s * (0.9 + random() * 0.2), hsl.l * (0.92 + random() * 0.16));
    scene3d.foliage.push({ colour: colour, needles: foliage.needles });
    // bark from the species' stem colour, greyed towards real bark
    var stem = species && species.stemColour ? colourFromHsv(species.stemColour) : new THREE.Color("#5b4636");
    stem.lerp(new THREE.Color("#4a4037"), 0.72);
    scene3d.bark.push(stem);
  }
}

function sceneShowCycle(theRun, cycle, options) {
  // Put every plant of one cycle in the scene.
  // options: { colourBy: "species" or "light", highlight: species number or -1,
  //            lightColour: function(light) giving a CSS colour }
  if (scene3d === null) {
    return;
  }
  var state = scene3d;
  var index = theRun.fieldIndex;
  var plants = cycle.plants;
  var matrix = new THREE.Matrix4();
  var rotation = new THREE.Quaternion();
  var noTurn = new THREE.Quaternion();
  var scale = new THREE.Vector3();
  var place = new THREE.Vector3();
  var colour = new THREE.Color();
  var dim = new THREE.Color("#5f6358");
  var turnAxis = new THREE.Vector3();

  var stems = ensureCapacity(state, "stems", plants.length);
  var counts = { broadLeaves: 0, broadCore: 0, needleLeaves: 0, needleCore: 0 };
  // share the clumps out: a crown may have up to twice its share, since
  // most crowns need fewer
  var clumpBudget = Math.max(3, 2 * CLUMP_BUDGET / Math.max(1, plants.length));
  // work out every crown first, to know how big the meshes must be
  var crowns = [];
  var total = 0;
  for (var q = 0; q < plants.length; q++) {
    var crownPlant = plants[q];
    var crownHeight = Math.max(crownPlant[index.stemHeight], 0.001);
    var crownRadius = Math.max(crownPlant[index.canopyRadius], 0.01);
    var crownTop = (crownPlant[index.elevation] || 0) + crownHeight;
    var crownSpecies = theRun.species[crownPlant[index.species]];
    var crownDeep = Math.max(crownDepth(crownSpecies, crownHeight, crownRadius), 0.01);
    var clumpList = crownClumps(crownPlant[index.x], crownTop, -crownPlant[index.y], crownRadius, crownDeep, clumpBudget);
    crowns.push(clumpList);
    total += clumpList.length;
  }
  var broadLeaves = ensureCapacity(state, "broadLeaves", total + 1);
  var broadCore = ensureCapacity(state, "broadCore", total + 1);
  var needleLeaves = ensureCapacity(state, "needleLeaves", total + 1);
  var needleCore = ensureCapacity(state, "needleCore", total + 1);

  for (var p = 0; p < plants.length; p++) {
    var plant = plants[p];
    var x = plant[index.x];
    var y = plant[index.y];
    var base = plant[index.elevation] || 0;
    var stemRadius = Math.max(plant[index.stemRadius], 0.002);
    var stemHeight = Math.max(plant[index.stemHeight], 0.001);
    var radius = Math.max(plant[index.canopyRadius], 0.01);
    var speciesNumber = plant[index.species];
    var species = theRun.species[speciesNumber];
    var light = plant[index.light];
    if (light === null || light === undefined) {
      light = 1;
    }
    var faded = options.highlight >= 0 && speciesNumber !== options.highlight;

    // the stem, from a little below the ground (so it never floats) to its top
    var sink = Math.min(0.4, stemHeight * 0.2);
    place.set(x, base - sink, -y);
    scale.set(stemRadius, stemHeight + sink, stemRadius);
    matrix.compose(place, noTurn, scale);
    stems.setMatrixAt(p, matrix);
    colour.copy(state.bark[speciesNumber] || dim);
    if (faded) {
      colour.lerp(dim, 0.6);
    }
    stems.setColorAt(p, colour);

    // the crown
    var foliage = state.foliage[speciesNumber] || { colour: new THREE.Color(DEFAULT_FOLIAGE.colour), needles: false };
    if (options.colourBy === "light" && options.lightColour) {
      colour.set(options.lightColour(light));
    } else {
      colour.copy(foliage.colour);
      // leaves in the shade are darker, as Vida's own pictures show them
      colour.multiplyScalar(0.55 + 0.45 * Math.max(0, Math.min(1, light)));
    }
    if (faded) {
      colour.lerp(dim, 0.75);
    }
    var leavesMesh = foliage.needles ? needleLeaves : broadLeaves;
    var coreMesh = foliage.needles ? needleCore : broadCore;
    var leavesKey = foliage.needles ? "needleLeaves" : "broadLeaves";
    var coreKey = foliage.needles ? "needleCore" : "broadCore";
    var clumps = crowns[p];
    var turnRandom = makeRandom(seedFor(x, y) + 3);
    for (var c = 0; c < clumps.length; c++) {
      var clump = clumps[c];
      place.set(clump[0], clump[1], clump[2]);
      turnAxis.set(turnRandom() - 0.5, 1.5, turnRandom() - 0.5).normalize();
      rotation.setFromAxisAngle(turnAxis, turnRandom() * Math.PI * 2);
      scale.set(clump[3], clump[3], clump[3]);
      matrix.compose(place, rotation, scale);
      leavesMesh.setMatrixAt(counts[leavesKey], matrix);
      leavesMesh.setColorAt(counts[leavesKey], colour);
      counts[leavesKey] += 1;
      coreMesh.setMatrixAt(counts[coreKey], matrix);
      var inner = colour.clone().multiplyScalar(0.38);
      coreMesh.setColorAt(counts[coreKey], inner);
      counts[coreKey] += 1;
    }
  }
  stems.count = plants.length;
  var keys = ["stems", "broadLeaves", "broadCore", "needleLeaves", "needleCore"];
  for (var k = 0; k < keys.length; k++) {
    var mesh = state.meshes[keys[k]];
    if (keys[k] !== "stems") {
      mesh.count = counts[keys[k]];
    }
    mesh.instanceMatrix.needsUpdate = true;
    if (mesh.instanceColor) {
      mesh.instanceColor.needsUpdate = true;
    }
  }

  // the water
  var water = cycle.waterLevel;
  if (theRun.header.terrain && water !== null && water > 0) {
    state.water.visible = true;
    state.water.position.y = water;
  } else {
    state.water.visible = false;
  }
  requestSceneDraw();
}

function sceneResize() {
  if (scene3d === null) {
    return;
  }
  var box = scene3d.box.getBoundingClientRect();
  var width = Math.max(1, Math.round(box.width));
  var height = Math.max(1, Math.round(box.height));
  scene3d.renderer.setSize(width, height, false);
  scene3d.renderer.domElement.style.width = "100%";
  scene3d.renderer.domElement.style.height = "100%";
  scene3d.camera.aspect = width / height;
  scene3d.camera.updateProjectionMatrix();
  requestSceneDraw();
}

function requestSceneDraw() {
  // draw once, the next time the browser is ready (several changes in a row
  // are drawn together)
  if (scene3d === null || scene3d.drawPending) {
    return;
  }
  scene3d.drawPending = true;
  window.requestAnimationFrame(drawSceneNow);
}

function drawSceneNow() {
  scene3d.drawPending = false;
  scene3d.renderer.render(scene3d.scene, scene3d.camera);
}
