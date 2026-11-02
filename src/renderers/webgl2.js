import { BG, VIEW, KIND_COLORS, busyWait, drawScreenLayer, bytesToBase64Png } from './common.js';

const VS_RECT = `#version 300 es
layout(location=0) in vec2 aCorner; layout(location=1) in vec4 aRect;
uniform vec3 uCam; uniform vec2 uRes;
void main(){ vec2 w = aRect.xy + aCorner*aRect.zw; vec2 s = (w - uCam.xy)*uCam.z;
  gl_Position = vec4(s.x/(uRes.x*0.5), -s.y/(uRes.y*0.5), 0., 1.); }`;
const FS_FLAT = `#version 300 es
precision mediump float; uniform vec4 uColor; out vec4 o; void main(){ o = uColor; }`;

const VS_SEG = `#version 300 es
layout(location=0) in vec2 aCorner; layout(location=1) in vec4 aSeg;
uniform vec3 uCam; uniform vec2 uRes; uniform float uHalf;
void main(){ vec2 a = (aSeg.xy - uCam.xy)*uCam.z, b = (aSeg.zw - uCam.xy)*uCam.z;
  vec2 d = b - a; float l = max(length(d), 1e-4); vec2 n = vec2(-d.y, d.x)/l;
  vec2 p = mix(a, b, aCorner.x) + n*aCorner.y*uHalf;
  gl_Position = vec4(p.x/(uRes.x*0.5), -p.y/(uRes.y*0.5), 0., 1.); }`;

const VS_NODE = `#version 300 es
layout(location=0) in vec2 aCorner; layout(location=1) in vec4 aNode;
uniform vec3 uCam; uniform vec2 uRes; uniform vec3 uPal[8];
out vec2 vP; out vec3 vCol; flat out float vShape; flat out float vPx;
void main(){ float shape = mod(aNode.w, 4.), kind = floor(aNode.w/4.);
  vec2 w = aNode.xy + aCorner*aNode.z*1.5; vec2 s = (w - uCam.xy)*uCam.z;
  gl_Position = vec4(s.x/(uRes.x*0.5), -s.y/(uRes.y*0.5), 0., 1.);
  vP = aCorner*1.5; vCol = uPal[int(kind)]; vShape = shape; vPx = aNode.z*uCam.z; }`;
const FS_NODE = `#version 300 es
precision mediump float; in vec2 vP; in vec3 vCol; flat in float vShape; flat in float vPx; out vec4 o;
void main(){ float d;
  if (vShape < 0.5) d = length(vP) - 1.;
  else if (vShape < 1.5) d = abs(vP.x) + abs(vP.y) - 1.3;
  else d = max(abs(vP.x), abs(vP.y)) - 1.;
  float a = clamp(0.5 - d*vPx, 0., 1.); if (a <= 0.) discard; o = vec4(vCol, a); }`;

function program(gl, vs, fs) {
  const mk = (type, src) => { const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s); if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s)); return s; };
  const p = gl.createProgram(); gl.attachShader(p, mk(gl.VERTEX_SHADER, vs)); gl.attachShader(p, mk(gl.FRAGMENT_SHADER, fs)); gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
  return p;
}

// Hand-written WebGL2: three instanced draws (bands, edges, nodes with an SDF fragment shader), all geometry uploaded once.
// Text and HUD go on a transparent Canvas 2D stacked above: the usual split for a GL scene.
export class WebGL2Renderer {
  constructor() { this.id = 'webgl2-raw'; }

  async init(host, scene, dpr) {
    this.scene = scene; this.dpr = dpr;
    const c = document.createElement('canvas'); c.width = Math.round(VIEW.w * dpr); c.height = Math.round(VIEW.h * dpr);
    c.style.width = VIEW.w + 'px'; c.style.height = VIEW.h + 'px'; host.appendChild(c);
    const gl = c.getContext('webgl2', { antialias: false, alpha: false, powerPreference: 'high-performance', preserveDrawingBuffer: false });
    if (!gl) throw new Error('webgl2 context unavailable');
    this.canvas = c; this.gl = gl;
    const dbg = gl.getExtension('WEBGL_debug_renderer_info');
    const info = { backend: 'webgl2', gpu: dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER), vendor: dbg ? gl.getParameter(dbg.UNMASKED_VENDOR_WEBGL) : gl.getParameter(gl.VENDOR), version: gl.getParameter(gl.VERSION) };

    this.pRect = program(gl, VS_RECT, FS_FLAT); this.pSeg = program(gl, VS_SEG, FS_FLAT); this.pNode = program(gl, VS_NODE, FS_NODE);
    const quad = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, quad);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, -1, 1, -1, 0, 1, 1, 1]), gl.STATIC_DRAW); // x 0..1, y -1..1
    const centred = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, centred);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    const rectCorner = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, rectCorner);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);

    const inst = (data, cornerBuf) => {
      const vao = gl.createVertexArray(); gl.bindVertexArray(vao);
      gl.bindBuffer(gl.ARRAY_BUFFER, cornerBuf); gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
      const buf = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, buf); gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
      gl.enableVertexAttribArray(1); gl.vertexAttribPointer(1, 4, gl.FLOAT, false, 0, 0); gl.vertexAttribDivisor(1, 1);
      return vao;
    };
    const b = scene.bands, e = scene.edges, n = scene.nodes;
    const rects = new Float32Array(b.n * 4); for (let i = 0; i < b.n; i++) { rects.set([b.x[i], b.y[i], b.w[i], b.h[i]], i * 4); }
    const segs = new Float32Array(e.n * 4); for (let i = 0; i < e.n; i++) { segs.set([e.x1[i], e.y1[i], e.x2[i], e.y2[i]], i * 4); }
    const nodes = new Float32Array(scene.N * 4); for (let i = 0; i < scene.N; i++) { nodes.set([n.x[i], n.y[i], n.r[i], n.shape[i] + n.kind[i] * 4], i * 4); }
    this.vaoRect = inst(rects, rectCorner); this.nRect = b.n;
    this.vaoSeg = inst(segs, quad); this.nSeg = e.n;
    this.vaoNode = inst(nodes, centred); this.nNode = scene.N;
    gl.bindVertexArray(null);

    const o = document.createElement('canvas'); o.width = c.width; o.height = c.height; o.style.width = VIEW.w + 'px'; o.style.height = VIEW.h + 'px'; o.style.pointerEvents = 'none';
    host.appendChild(o); this.overlay = o; this.octx = o.getContext('2d');
    this.busy = 0;
    const bg = BG.match(/#(..)(..)(..)/).slice(1).map((h) => parseInt(h, 16) / 255); this.bg = bg;
    return info;
  }

  frame(cam, labels) {
    this.last = [cam, labels];
    const gl = this.gl, dpr = this.dpr;
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.clearColor(this.bg[0], this.bg[1], this.bg[2], 1); gl.clear(gl.COLOR_BUFFER_BIT);
    gl.enable(gl.BLEND); gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    const set = (p) => { gl.useProgram(p); gl.uniform3f(gl.getUniformLocation(p, 'uCam'), cam.cx, cam.cy, cam.z); gl.uniform2f(gl.getUniformLocation(p, 'uRes'), VIEW.w, VIEW.h); };

    set(this.pRect); gl.uniform4f(gl.getUniformLocation(this.pRect, 'uColor'), 0.909, 0.639, 0.239, 0.16);
    gl.bindVertexArray(this.vaoRect); gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.nRect);

    set(this.pSeg); gl.uniform4f(gl.getUniformLocation(this.pSeg, 'uColor'), 0.627, 0.588, 0.51, 0.55); gl.uniform1f(gl.getUniformLocation(this.pSeg, 'uHalf'), 0.9);
    gl.bindVertexArray(this.vaoSeg); gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.nSeg);

    set(this.pNode);
    if (!this.pal) { this.pal = new Float32Array(24); KIND_COLORS.forEach((c, i) => this.pal.set([(c >> 16 & 255) / 255, (c >> 8 & 255) / 255, (c & 255) / 255], i * 3)); }
    gl.uniform3fv(gl.getUniformLocation(this.pNode, 'uPal'), this.pal);
    gl.bindVertexArray(this.vaoNode); gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.nNode);
    gl.bindVertexArray(null);

    const ctx = this.octx;
    ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.clearRect(0, 0, this.overlay.width, this.overlay.height);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    drawScreenLayer(ctx, cam, labels, this.scene);
    if (this.busy) busyWait(this.busy);
  }

  async snapshot() {
    this.frame(...this.last); // the drawing buffer is cleared after compositing: draw again and copy within the same task
    const c = document.createElement('canvas'); c.width = this.canvas.width; c.height = this.canvas.height;
    const x = c.getContext('2d'); x.drawImage(this.canvas, 0, 0); x.drawImage(this.overlay, 0, 0);
    return bytesToBase64Png(c.toDataURL('image/png'));
  }
  destroy() { this.gl.getExtension('WEBGL_lose_context')?.loseContext(); this.canvas.remove(); this.overlay.remove(); }
}
