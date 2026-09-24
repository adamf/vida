// Vida viewer, natural scene: textures drawn on canvases when the scene
// starts, and small pieces of shader code the other scene files share.
//
// Every texture is drawn in greys where the colour comes from elsewhere
// (a tree's own colour, the ground's colour), so one texture serves all.

"use strict";

// Noise for shaders: sceneHash (a random number for a point), sceneNoise
// (smooth random hills), sceneFbm (several sizes of hills added together)
// sceneCells (distance to the nearest of a set of moving points, for foam)
// and sceneCellEdges (how far from the line halfway between the two nearest
// points: small along a net of thin lines, like the light patterns that
// ripples focus on the bottom of the water).
var SCENE_NOISE_GLSL = [
  "float sceneHash(vec2 p) {",
  "  p = fract(p * vec2(123.34, 456.21));",
  "  p += dot(p, p + 45.32);",
  "  return fract(p.x * p.y);",
  "}",
  "float sceneNoise(vec2 p) {",
  "  vec2 i = floor(p);",
  "  vec2 f = fract(p);",
  "  vec2 u = f * f * (3.0 - 2.0 * f);",
  "  return mix(mix(sceneHash(i), sceneHash(i + vec2(1.0, 0.0)), u.x),",
  "             mix(sceneHash(i + vec2(0.0, 1.0)), sceneHash(i + vec2(1.0, 1.0)), u.x), u.y);",
  "}",
  "float sceneFbm(vec2 p) {",
  "  float total = 0.0;",
  "  float amplitude = 0.5;",
  "  for (int i = 0; i < 5; i++) {",
  "    total += sceneNoise(p) * amplitude;",
  "    p = p * 2.03 + vec2(17.1, 9.2);",
  "    amplitude *= 0.5;",
  "  }",
  "  return total;",
  "}",
  "float sceneCells(vec2 p, float t) {",
  "  vec2 i = floor(p);",
  "  vec2 f = fract(p);",
  "  float best = 8.0;",
  "  for (int y = -1; y <= 1; y++) {",
  "    for (int x = -1; x <= 1; x++) {",
  "      vec2 cell = vec2(float(x), float(y));",
  "      vec2 point = vec2(sceneHash(i + cell), sceneHash(i + cell + 31.7));",
  "      point = 0.5 + 0.5 * sin(t + 6.2831 * point);",
  "      best = min(best, length(cell + point - f));",
  "    }",
  "  }",
  "  return best;",
  "}",
  "float sceneCellEdges(vec2 p, float t) {",
  "  vec2 i = floor(p);",
  "  vec2 f = fract(p);",
  "  float nearest = 8.0;",
  "  float next = 8.0;",
  "  for (int y = -1; y <= 1; y++) {",
  "    for (int x = -1; x <= 1; x++) {",
  "      vec2 cell = vec2(float(x), float(y));",
  "      vec2 point = vec2(sceneHash(i + cell), sceneHash(i + cell + 31.7));",
  "      point = 0.5 + 0.5 * sin(t + 6.2831 * point);",
  "      float d = length(cell + point - f);",
  "      next = min(next, max(nearest, d));",
  "      nearest = min(nearest, d);",
  "    }",
  "  }",
  "  return next - nearest;",
  "}"
].join("\n");

function makeRandom(seed) {
  // A small random number generator that gives the same numbers for the same
  // seed, so a tree or a patch of grass looks the same every time.
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

function newCanvas(width, height) {
  var canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  return canvas;
}

function colourTexture(canvas, repeat) {
  // a texture of colours (drawn in sRGB, like everything on a canvas)
  var texture = new THREE.CanvasTexture(canvas);
  texture.encoding = THREE.sRGBEncoding;
  texture.anisotropy = 8;
  if (repeat) {
    texture.wrapS = THREE.RepeatWrapping;
    texture.wrapT = THREE.RepeatWrapping;
  }
  return texture;
}

function dataTexture(canvas, repeat) {
  // a texture of numbers, like a normal map (no sRGB conversion)
  var texture = new THREE.CanvasTexture(canvas);
  texture.anisotropy = 8;
  if (repeat) {
    texture.wrapS = THREE.RepeatWrapping;
    texture.wrapT = THREE.RepeatWrapping;
  }
  return texture;
}

function normalMapFrom(heightCanvas, strength) {
  // A normal map (which way each point of a surface faces) from a canvas
  // of heights (lighter is higher). It wraps round at the edges, so it
  // tiles if the heights do.
  var width = heightCanvas.width;
  var height = heightCanvas.height;
  var heights = heightCanvas.getContext("2d").getImageData(0, 0, width, height).data;
  var canvas = newCanvas(width, height);
  var context = canvas.getContext("2d");
  var image = context.createImageData(width, height);
  function at(x, y) {
    x = (x + width) % width;
    y = (y + height) % height;
    return heights[(y * width + x) * 4] / 255;
  }
  for (var y = 0; y < height; y++) {
    for (var x = 0; x < width; x++) {
      var dx = (at(x + 1, y) - at(x - 1, y)) * strength;
      var dy = (at(x, y + 1) - at(x, y - 1)) * strength;
      var length = Math.sqrt(dx * dx + dy * dy + 1);
      var index = (y * width + x) * 4;
      image.data[index] = Math.round((-dx / length * 0.5 + 0.5) * 255);
      image.data[index + 1] = Math.round((dy / length * 0.5 + 0.5) * 255);
      image.data[index + 2] = Math.round((1 / length * 0.5 + 0.5) * 255);
      image.data[index + 3] = 255;
    }
  }
  context.putImageData(image, 0, 0);
  return dataTexture(canvas, true);
}

function tilingNoiseCanvas(size, seed, waves, contrast) {
  // Soft, random-looking heights that tile: a sum of waves that each fit a
  // whole number of times across the canvas.
  var random = makeRandom(seed);
  var list = [];
  for (var w = 0; w < waves; w++) {
    list.push({
      fx: Math.floor(1 + random() * 9) * (random() < 0.5 ? -1 : 1),
      fy: Math.floor(1 + random() * 9) * (random() < 0.5 ? -1 : 1),
      phase: random() * Math.PI * 2,
      amplitude: 0.3 + random()
    });
  }
  var canvas = newCanvas(size, size);
  var context = canvas.getContext("2d");
  var image = context.createImageData(size, size);
  var total = 0;
  for (var k = 0; k < list.length; k++) {
    total += list[k].amplitude;
  }
  for (var y = 0; y < size; y++) {
    for (var x = 0; x < size; x++) {
      var value = 0;
      for (var i = 0; i < list.length; i++) {
        var wave = list[i];
        value += wave.amplitude * Math.sin((wave.fx * x + wave.fy * y) / size * Math.PI * 2 + wave.phase);
      }
      value = 0.5 + 0.5 * Math.max(-1, Math.min(1, value / total * contrast));
      var index = (y * size + x) * 4;
      var grey = Math.round(value * 255);
      image.data[index] = grey;
      image.data[index + 1] = grey;
      image.data[index + 2] = grey;
      image.data[index + 3] = 255;
    }
  }
  context.putImageData(image, 0, 0);
  return canvas;
}

// ---------------------------------------------------------------------------
// Leaves and needles
// ---------------------------------------------------------------------------

function drawLeaf(context, x, y, length, width, angle, shade, tint) {
  // one leaf: a pointed oval, lighter towards the tip, with a stalk,
  // a paler midrib and faint side veins
  context.save();
  context.translate(x, y);
  context.rotate(angle);
  var gradient = context.createLinearGradient(0, length / 2, 0, -length / 2);
  gradient.addColorStop(0, "rgb(" + Math.round(shade * 0.72 * tint[0]) + "," + Math.round(shade * 0.72 * tint[1]) + "," + Math.round(shade * 0.72 * tint[2]) + ")");
  gradient.addColorStop(1, "rgb(" + Math.round(Math.min(255, shade * 1.08 * tint[0])) + "," + Math.round(Math.min(255, shade * 1.08 * tint[1])) + "," + Math.round(Math.min(255, shade * 1.08 * tint[2])) + ")");
  context.fillStyle = gradient;
  context.beginPath();
  context.moveTo(0, -length / 2);
  context.bezierCurveTo(width, -length / 4, width * 0.9, length / 4, 0, length / 2);
  context.bezierCurveTo(-width * 0.9, length / 4, -width, -length / 4, 0, -length / 2);
  context.fill();
  context.strokeStyle = "rgba(255,255,240,0.32)";
  context.lineWidth = 1.4;
  context.beginPath();
  context.moveTo(0, length / 2 + 6);
  context.lineTo(0, -length / 2 + 3);
  context.stroke();
  context.lineWidth = 0.8;
  context.strokeStyle = "rgba(255,255,240,0.16)";
  for (var v = -3; v <= 3; v++) {
    if (v === 0) {
      continue;
    }
    var along = v / 4 * length * 0.4;
    context.beginPath();
    context.moveTo(0, along);
    context.lineTo(Math.sign(v) * width * 0.7, along - length * 0.12);
    context.moveTo(0, along);
    context.lineTo(-Math.sign(v) * width * 0.7, along - length * 0.12);
    context.stroke();
  }
  context.restore();
}

function leafTexture() {
  // A spray of leaves on twigs, in greys (with a little warmth and coolness
  // here and there): the tree's own colour is multiplied in.
  var canvas = newCanvas(512, 512);
  var context = canvas.getContext("2d");
  var random = makeRandom(7);
  context.strokeStyle = "rgba(70,60,50,0.9)";
  context.lineWidth = 3;
  for (var t = 0; t < 5; t++) {
    context.beginPath();
    context.moveTo(256, 256);
    var angle = random() * Math.PI * 2;
    context.lineTo(256 + Math.cos(angle) * 190, 256 + Math.sin(angle) * 190);
    context.stroke();
  }
  for (var i = 0; i < 46; i++) {
    var leafAngle = random() * Math.PI * 2;
    var distance = Math.sqrt(random()) * 185;
    var x = 256 + Math.cos(leafAngle) * distance;
    var y = 256 + Math.sin(leafAngle) * distance;
    var shade = 165 + Math.floor(random() * 90);
    var tint = [1, 1, 1];
    var pick = random();
    if (pick < 0.12) {
      tint = [1.08, 1.04, 0.86];
    } else if (pick < 0.22) {
      tint = [0.92, 1.0, 1.06];
    }
    drawLeaf(context, x, y, 70 + random() * 38, 26 + random() * 12, leafAngle + Math.PI / 2 + (random() - 0.5) * 1.2, shade, tint);
  }
  return colourTexture(canvas, false);
}

function needleTexture() {
  // Sprays of pine needles along little twigs, in greys.
  var canvas = newCanvas(512, 512);
  var context = canvas.getContext("2d");
  var random = makeRandom(11);
  context.lineCap = "round";
  for (var twig = 0; twig < 14; twig++) {
    var angle = random() * Math.PI * 2;
    var startDistance = random() * 60;
    var sx = 256 + Math.cos(angle) * startDistance;
    var sy = 256 + Math.sin(angle) * startDistance;
    var length = 120 + random() * 90;
    var ex = sx + Math.cos(angle) * length;
    var ey = sy + Math.sin(angle) * length;
    context.strokeStyle = "rgb(95,85,70)";
    context.lineWidth = 3;
    context.beginPath();
    context.moveTo(sx, sy);
    context.lineTo(ex, ey);
    context.stroke();
    for (var n = 0; n < 60; n++) {
      var along = random();
      var px = sx + (ex - sx) * along;
      var py = sy + (ey - sy) * along;
      var side = random() < 0.5 ? -1 : 1;
      var needleAngle = angle + side * (0.5 + random() * 0.7);
      var needleLength = 26 + random() * 26;
      var shade = 150 + Math.floor(random() * 105);
      context.strokeStyle = "rgb(" + shade + "," + shade + "," + Math.round(shade * 0.97) + ")";
      context.lineWidth = 2.4;
      context.beginPath();
      context.moveTo(px, py);
      context.lineTo(px + Math.cos(needleAngle) * needleLength, py + Math.sin(needleAngle) * needleLength);
      context.stroke();
    }
  }
  return colourTexture(canvas, false);
}

// ---------------------------------------------------------------------------
// Bark, ground and water
// ---------------------------------------------------------------------------

function barkHeights() {
  // Furrows running up the trunk, with plates of bark between them.
  var canvas = newCanvas(128, 512);
  var context = canvas.getContext("2d");
  context.fillStyle = "rgb(170,170,170)";
  context.fillRect(0, 0, 128, 512);
  var random = makeRandom(5);
  for (var i = 0; i < 220; i++) {
    var shade = 60 + Math.floor(random() * 170);
    context.fillStyle = "rgba(" + shade + "," + shade + "," + shade + ",0.8)";
    var x = random() * 128;
    var width = 2 + random() * 7;
    var y = random() * 512;
    var tall = 30 + random() * 180;
    context.fillRect(x, y, width, tall);
    // wrap round the top and bottom, and the sides, so it tiles
    context.fillRect(x, y - 512, width, tall);
    context.fillRect(x - 128, y, width, tall);
  }
  return canvas;
}

function barkTextures() {
  var heights = barkHeights();
  var colour = colourTexture(heights, true);
  colour.repeat.set(1, 3);
  var normal = normalMapFrom(heights, 5);
  normal.repeat.set(1, 3);
  return { colour: colour, normal: normal };
}

function groundDetail() {
  // Fine speckles of grass, soil and small stones, in greys, that tile.
  var size = 512;
  var canvas = tilingNoiseCanvas(size, 23, 14, 1.4);
  var context = canvas.getContext("2d");
  var random = makeRandom(3);
  for (var i = 0; i < 9000; i++) {
    var shade = 110 + Math.floor(random() * 145);
    context.strokeStyle = "rgba(" + shade + "," + shade + "," + shade + ",0.55)";
    context.lineWidth = 1;
    var x = random() * size;
    var y = random() * size;
    var lean = (random() - 0.5) * 4;
    var tall = 3 + random() * 7;
    for (var dx = -size; dx <= size; dx += size) {
      for (var dy = -size; dy <= size; dy += size) {
        if (x + dx > -10 && x + dx < size + 10 && y + dy > -10 && y + dy < size + 10) {
          context.beginPath();
          context.moveTo(x + dx, y + dy);
          context.lineTo(x + dx + lean, y + dy - tall);
          context.stroke();
        }
      }
    }
  }
  var colour = colourTexture(canvas, true);
  var normal = normalMapFrom(canvas, 2.5);
  return { colour: colour, normal: normal };
}

function waterNormals() {
  // Ripples that tile, as a normal map: many small waves of whole-number
  // sizes across the texture, so there is no seam when it repeats.
  var heights = tilingNoiseCanvas(256, 41, 36, 1.6);
  return normalMapFrom(heights, 3.2);
}
