// Vida viewer, natural scene: the trees.
//
// Drawn to scale from the simulation (see the top of scene.js): each stem is
// a cylinder of the stem's height and width, and each crown is the top half
// of a sphere as wide as the canopy radius with its top at the top of the
// stem, as deep as it is wide or, for crownShape PARA, reaching from
// boleHeight percent of the way down the tree to the top. The crown is
// filled with clumps of leaves, layer by layer. Leaves are darker when the
// plant got less light.
//
// The wind sways the clumps a few centimetres and flutters the leaves, and
// leaves glow when the sun shines through them from behind: decoration.

"use strict";

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

function foliageFor(speciesName) {
  var words = String(speciesName || "").split(/[_\s]+/);
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
// Shapes
// ---------------------------------------------------------------------------

function clumpCoreGeometry() {
  // the inside of a clump of leaves: a lumpy ball, darker than the leaves,
  // so there are no holes to see through
  var geometry = new THREE.IcosahedronGeometry(0.56, 1);
  var position = geometry.attributes.position;
  var point = new THREE.Vector3();
  for (var i = 0; i < position.count; i++) {
    point.fromBufferAttribute(position, i);
    // a smooth bump that depends only on where the point is, so the copies
    // of a corner shared by two triangles move together
    var bump = 1 + 0.12 * Math.sin(3.1 * point.x + 1.3) * Math.sin(3.7 * point.y + 0.4) * Math.sin(2.9 * point.z + 2.1);
    point.multiplyScalar(bump);
    position.setXYZ(i, point.x, point.y, point.z);
  }
  geometry.computeVertexNormals();
  return geometry;
}

function clumpLeavesGeometry(cards, cardSize, seed) {
  // Many small squares ("cards") with leaves drawn on them, scattered over a
  // ball of radius 1, facing roughly outwards. They are lit as if facing
  // outwards and a little up, so a crown looks like one soft mass of leaves.
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
  var helper = new THREE.Vector3();
  var lit = new THREE.Vector3();
  var skyward = new THREE.Vector3(0, 0.55, 0);
  for (var i = 0; i < cards; i++) {
    var z = random() * 2 - 1;
    var angle = random() * Math.PI * 2;
    var ring = Math.sqrt(1 - z * z);
    outward.set(ring * Math.cos(angle), z, ring * Math.sin(angle));
    centre.copy(outward).multiplyScalar(0.5 + random() * 0.45);
    lit.copy(outward).multiplyScalar(0.7).add(skyward).normalize();
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
  // how far down from the top of the stem the crown reaches (see the top of this file)
  if (species && species.crownShape === "PARA" && species.boleHeight !== null && species.boleHeight !== undefined) {
    return stemHeight * species.boleHeight / 100;
  }
  return canopyRadius;
}

function widthAt(radius, depth, above) {
  // the radius of the crown at a height `above` its bottom
  var fraction = Math.max(0, Math.min(1, above / depth));
  return radius * Math.sqrt(1 - fraction * fraction);
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

function estimateClumps(radius, depth, size) {
  var total = 1;
  var step = size * 0.9;
  for (var above = Math.min(depth * 0.5, size * 0.75); above < depth - size * 0.9; above += step) {
    total += layerCount(widthAt(radius, depth, above), size);
  }
  return total;
}

function addLayer(clumps, random, x, z, height, width, size, fraction) {
  // one layer of clumps: a ring round the edge of the crown at this height,
  // and one in the middle if the crown is wide there. The last number of
  // each clump is how far up the crown it is (0 bottom, 1 top).
  var count = layerCount(width, size);
  if (count === 1) {
    var small = Math.max(0.03, Math.min(size, width));
    clumps.push([x + (random() - 0.5) * 0.2 * small, height, z + (random() - 0.5) * 0.2 * small, small, fraction]);
    return;
  }
  var ring = count;
  if (width > size * 2.6) {
    ring = count - 1;
    clumps.push([x, height + (random() - 0.5) * size * 0.3, z, size, fraction * 0.7]);
  }
  var reach = width - size;
  var phase = random() * Math.PI * 2;
  for (var i = 0; i < ring; i++) {
    var angle = phase + (i + (random() - 0.5) * 0.4) / ring * Math.PI * 2;
    var along = reach * (0.9 + random() * 0.1);
    clumps.push([x + Math.cos(angle) * along, height + (random() - 0.5) * size * 0.6,
      z + Math.sin(angle) * along, size * (0.8 + random() * 0.2), fraction]);
  }
}

function crownClumps(x, top, z, radius, depth, clumpBudget) {
  // Where to put the clumps of leaves that make up a crown, as
  // [x, y, z, clump radius, height in the crown], filling the top half of an
  // ellipsoid `radius` wide and `depth` deep with its top at `top`. The
  // same plant always gets the same clumps.
  var random = makeRandom(seedFor(x, z));
  var size = Math.max(0.04, Math.min(1.6, Math.min(radius, depth) * 0.6));
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
    addLayer(clumps, random, x, z, height, widthAt(radius, depth, height - base), size, (height - base) / depth);
    height += step * (0.85 + random() * 0.3);
  }
  clumps.push([x, top - size, z, size, 1]);
  return clumps;
}

// ---------------------------------------------------------------------------
// Materials: wind and light through the leaves
// ---------------------------------------------------------------------------

var WIND_VERTEX = [
  "vec3 clumpOrigin = vec3(instanceMatrix[3]);",
  "float swayPhase = time * 1.1 + clumpOrigin.x * 0.31 + clumpOrigin.z * 0.23;",
  "float sway = (sin(swayPhase) * 0.6 + sin(swayPhase * 2.27 + 1.7) * 0.3) * windStrength;",
  "transformed.x += sway * 0.06 * (position.y + 1.2);",
  "transformed.z += sway * 0.035 * (position.y + 1.2);",
  "transformed += normal * sin(time * 6.5 + position.x * 13.0 + position.z * 11.0 + clumpOrigin.x) * 0.02 * windStrength;"
].join("\n");

function addWind(state, material, withGlow) {
  function onBeforeCompile(shader) {
    shader.uniforms.time = state.uniforms.time;
    shader.uniforms.windStrength = state.uniforms.windStrength;
    shader.uniforms.sunColour = state.uniforms.sunColour;
    shader.uniforms.sunDirectionView = state.uniforms.sunDirectionView;
    shader.vertexShader = shader.vertexShader
      .replace("#include <common>", "#include <common>\nuniform float time;\nuniform float windStrength;")
      .replace("#include <begin_vertex>", "#include <begin_vertex>\n" + WIND_VERTEX);
    if (withGlow) {
      shader.fragmentShader = shader.fragmentShader
        .replace("#include <common>", "#include <common>\nuniform vec3 sunColour;\nuniform vec3 sunDirectionView;")
        // both sides of a leaf card are lit as facing out of the clump
        .replace("#include <normal_fragment_begin>", "#include <normal_fragment_begin>\nnormal = normalize(vNormal);")
        .replace("#include <output_fragment>", [
          "float throughLeaves = pow(clamp(dot(normalize(-vViewPosition), sunDirectionView), 0.0, 1.0), 5.0);",
          "outgoingLight += diffuseColor.rgb * sunColour * throughLeaves * 1.6;",
          "#include <output_fragment>"
        ].join("\n"));
    }
  }
  material.onBeforeCompile = onBeforeCompile;
  // a different key for each kind, so three.js keeps their shaders apart
  material.customProgramCacheKey = function cacheKey() {
    return withGlow ? "vida-leaves-glow" : "vida-leaves";
  };
  return material;
}

function makeTreeMeshes(state, textures) {
  var stemGeometry = new THREE.CylinderGeometry(1, 1, 1, 12, 1, false);
  stemGeometry.translate(0, 0.5, 0);
  state.materials.stem = new THREE.MeshStandardMaterial({ map: textures.bark.colour, normalMap: textures.bark.normal,
    normalScale: new THREE.Vector2(2.2, 2.2), roughness: 0.95, metalness: 0, envMapIntensity: 0.45 });
  makeInstanced(state, "stems", stemGeometry, state.materials.stem, true);

  var core = clumpCoreGeometry();
  var styles = [["broad", textures.leaves, 44, 0.8], ["needle", textures.needles, 46, 0.82]];
  for (var i = 0; i < styles.length; i++) {
    var key = styles[i][0];
    var texture = styles[i][1];
    var cardMaterial = addWind(state, new THREE.MeshStandardMaterial({ map: texture, alphaTest: 0.5, side: THREE.DoubleSide,
      roughness: 0.88, metalness: 0, envMapIntensity: 0.32 }), true);
    var coreMaterial = addWind(state, new THREE.MeshStandardMaterial({ roughness: 1.0, metalness: 0, envMapIntensity: 0.25 }), false);
    var cards = makeInstanced(state, key + "Leaves", clumpLeavesGeometry(styles[i][2], styles[i][3], 101 + i), cardMaterial, true);
    var depthMaterial = addWind(state, new THREE.MeshDepthMaterial({ depthPacking: THREE.RGBADepthPacking, map: texture, alphaTest: 0.5 }), false);
    cards.customDepthMaterial = depthMaterial;
    var coreMesh = makeInstanced(state, key + "Core", core, coreMaterial, true);
    coreMesh.customDepthMaterial = addWind(state, new THREE.MeshDepthMaterial({ depthPacking: THREE.RGBADepthPacking }), false);
  }
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

function ensureCapacity(state, key, needed) {
  // an InstancedMesh has a fixed size; make a bigger one when there are more trees
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

// ---------------------------------------------------------------------------
// One cycle's trees
// ---------------------------------------------------------------------------

function setTreeSpecies(state, theRun) {
  // each species' leaf colour (from its genus) and bark colour
  state.foliage = [];
  state.bark = [];
  for (var s = 0; s < theRun.species.length; s++) {
    var species = theRun.species[s];
    var foliage = foliageFor(species ? species.name : "");
    var colour = new THREE.Color(foliage.colour);
    // each species a little different, so neighbours can be told apart
    var random = makeRandom(s * 7919 + 17);
    var hsl = {};
    colour.getHSL(hsl);
    colour.setHSL(hsl.h + (random() - 0.5) * 0.035, hsl.s * (0.9 + random() * 0.2), hsl.l * (0.92 + random() * 0.16));
    state.foliage.push({ colour: colour, needles: foliage.needles });
    var bark = species && species.stemColour ? colourFromHsv(species.stemColour) : new THREE.Color("#5b4636");
    bark.lerp(new THREE.Color("#6b635a"), 0.85);
    state.bark.push(bark);
  }
}

function placeTrees(state, theRun, cycle, options, clumpTotal) {
  // Put every plant of one cycle in the scene.
  var index = theRun.fieldIndex;
  var plants = cycle.plants;
  var matrix = new THREE.Matrix4();
  var rotation = new THREE.Quaternion();
  var noTurn = new THREE.Quaternion();
  var scale = new THREE.Vector3();
  var place = new THREE.Vector3();
  var colour = new THREE.Color();
  var clumpColour = new THREE.Color();
  var inner = new THREE.Color();
  var dim = new THREE.Color("#5f6358");
  var turnAxis = new THREE.Vector3();

  var stems = ensureCapacity(state, "stems", plants.length);
  var counts = { broadLeaves: 0, broadCore: 0, needleLeaves: 0, needleCore: 0 };
  // share the clumps out: a crown may have a little more than its share,
  // since most crowns need fewer
  var clumpBudget = Math.max(3, 1.3 * clumpTotal / Math.max(1, plants.length));
  var crowns = [];
  var total = 0;
  for (var q = 0; q < plants.length; q++) {
    var p0 = plants[q];
    var h0 = Math.max(p0[index.stemHeight], 0.001);
    var r0 = Math.max(p0[index.canopyRadius], 0.01);
    var top0 = (p0[index.elevation] || 0) + h0;
    var depth0 = Math.max(crownDepth(theRun.species[p0[index.species]], h0, r0), 0.01);
    var list = crownClumps(p0[index.x], top0, -p0[index.y], r0, depth0, clumpBudget);
    crowns.push(list);
    total += list.length;
  }
  var meshes = {
    broadLeaves: ensureCapacity(state, "broadLeaves", total + 1),
    broadCore: ensureCapacity(state, "broadCore", total + 1),
    needleLeaves: ensureCapacity(state, "needleLeaves", total + 1),
    needleCore: ensureCapacity(state, "needleCore", total + 1)
  };

  for (var p = 0; p < plants.length; p++) {
    var plant = plants[p];
    var x = plant[index.x];
    var y = plant[index.y];
    var base = plant[index.elevation] || 0;
    var stemRadius = Math.max(plant[index.stemRadius], 0.002);
    var stemHeight = Math.max(plant[index.stemHeight], 0.001);
    var speciesNumber = plant[index.species];
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
      colour.multiplyScalar(0.5 + 0.5 * Math.max(0, Math.min(1, light)));
    }
    if (faded) {
      colour.lerp(dim, 0.75);
    }
    var style = foliage.needles ? "needle" : "broad";
    var leavesMesh = meshes[style + "Leaves"];
    var coreMesh = meshes[style + "Core"];
    var clumps = crowns[p];
    var turnRandom = makeRandom(seedFor(x, y) + 3);
    for (var c = 0; c < clumps.length; c++) {
      var clump = clumps[c];
      place.set(clump[0], clump[1], clump[2]);
      turnAxis.set(turnRandom() - 0.5, 1.5, turnRandom() - 0.5).normalize();
      rotation.setFromAxisAngle(turnAxis, turnRandom() * Math.PI * 2);
      scale.set(clump[3], clump[3], clump[3]);
      matrix.compose(place, rotation, scale);
      // lower in the crown is darker (the leaves above shade it), and each
      // clump a little different
      clumpColour.copy(colour).multiplyScalar((0.72 + 0.28 * clump[4]) * (0.9 + turnRandom() * 0.2));
      leavesMesh.setMatrixAt(counts[style + "Leaves"], matrix);
      leavesMesh.setColorAt(counts[style + "Leaves"], clumpColour);
      counts[style + "Leaves"] += 1;
      coreMesh.setMatrixAt(counts[style + "Core"], matrix);
      inner.copy(clumpColour).multiplyScalar(0.34);
      coreMesh.setColorAt(counts[style + "Core"], inner);
      counts[style + "Core"] += 1;
    }
  }
  stems.count = plants.length;
  var keys = ["broadLeaves", "broadCore", "needleLeaves", "needleCore"];
  for (var k = 0; k < keys.length; k++) {
    meshes[keys[k]].count = counts[keys[k]];
    meshes[keys[k]].instanceMatrix.needsUpdate = true;
    meshes[keys[k]].instanceColor.needsUpdate = true;
  }
  stems.instanceMatrix.needsUpdate = true;
  stems.instanceColor.needsUpdate = true;
}
