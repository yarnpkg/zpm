import {useCallback, useEffect, useRef} from 'preact/hooks';

import catSdfUrl                        from './cat-shader/cat-sdf.png?url';
import catShapeUrl                      from './cat-shader/cat-shape.png?url';
import fragmentShaderSource             from './cat-shader/fragment.glsl?raw';
import vertexShaderSource               from './cat-shader/vertex.glsl?raw';

class ShaderController {
  canvas: HTMLCanvasElement;
  gl: WebGL2RenderingContext;

  program: WebGLProgram;
  texture: WebGLTexture | null;
  distanceField: WebGLTexture | null;
  textureWidth: number;
  textureHeight: number;
  startTime: number;
  falloff: number;
  fbmsubstract: number;

  constructor(canvas: HTMLCanvasElement, gl: WebGL2RenderingContext) {
    this.canvas = canvas;
    this.gl = gl;

    this.program = this.createProgram(gl);
    this.texture = null;
    this.distanceField = null;
    this.textureWidth = 0;
    this.textureHeight = 0;
    this.startTime = Date.now();
    this.falloff = 7.0;
    this.fbmsubstract = 1.8;

    this.init();
  }

  init() {
    this.gl.enable(this.gl.BLEND);
    this.gl.blendFunc(this.gl.SRC_ALPHA, this.gl.ONE_MINUS_SRC_ALPHA);

    this.setupGeometry();
    this.loadTexture();
    this.loadDistanceField();
    this.render();
  }

  dispose() {
  }

  createProgram(gl: WebGL2RenderingContext) {
    const vertexShader = this.createShader(gl, gl.VERTEX_SHADER, vertexShaderSource);
    const fragmentShader = this.createShader(gl, gl.FRAGMENT_SHADER, fragmentShaderSource);

    const program = gl.createProgram();
    gl.attachShader(program, vertexShader);
    gl.attachShader(program, fragmentShader);
    gl.linkProgram(program);

    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      const info = gl.getProgramInfoLog(program);
      gl.deleteProgram(program);
      throw new Error(`Error linking program: ${info}`);
    }

    gl.useProgram(program);

    return program;
  }

  createShader(gl: WebGL2RenderingContext, type: GLenum, source: string) {
    const shader = gl.createShader(type);
    if (!shader)
      throw new Error(`Failed to create shader`);

    gl.shaderSource(shader, source);
    gl.compileShader(shader);

    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      const info = gl.getShaderInfoLog(shader);
      gl.deleteShader(shader);
      throw new Error(`Error compiling shader: ${info}`);
    }

    return shader;
  }

  setupGeometry() {
    if (!this.program)
      throw new Error(`WebGL context not initialized`);

    // Create a full-screen quad
    const positions = new Float32Array([
      -1, -1,
      1, -1,
      -1,  1,
      -1,  1,
      1, -1,
      1,  1,
    ]);

    const texCoords = new Float32Array([
      0, 1,
      1, 1,
      0, 0,
      0, 0,
      1, 1,
      1, 0,
    ]);

    // Position buffer
    const positionBuffer = this.gl.createBuffer();
    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, positionBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, positions, this.gl.STATIC_DRAW);

    const positionLocation = this.gl.getAttribLocation(this.program, `a_position`);
    this.gl.enableVertexAttribArray(positionLocation);
    this.gl.vertexAttribPointer(positionLocation, 2, this.gl.FLOAT, false, 0, 0);

    // Texture coordinate buffer
    const texCoordBuffer = this.gl.createBuffer();
    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, texCoordBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, texCoords, this.gl.STATIC_DRAW);

    const texCoordLocation = this.gl.getAttribLocation(this.program, `a_texCoord`);
    this.gl.enableVertexAttribArray(texCoordLocation);
    this.gl.vertexAttribPointer(texCoordLocation, 2, this.gl.FLOAT, false, 0, 0);
  }

  loadTexture() {
    this.texture = this.gl.createTexture();
    this.gl.bindTexture(this.gl.TEXTURE_2D, this.texture);

    // Set up texture parameters
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_WRAP_S, this.gl.CLAMP_TO_EDGE);
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_WRAP_T, this.gl.CLAMP_TO_EDGE);
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_MIN_FILTER, this.gl.LINEAR);
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_MAG_FILTER, this.gl.LINEAR);

    // Load the image
    const image = new Image();
    image.onload = () => {
      this.textureWidth = image.width;
      this.textureHeight = image.height;
      this.gl.bindTexture(this.gl.TEXTURE_2D, this.texture);
      this.gl.texImage2D(this.gl.TEXTURE_2D, 0, this.gl.RGBA, this.gl.RGBA, this.gl.UNSIGNED_BYTE, image);
    };

    image.src = catShapeUrl;
  }

  loadDistanceField() {
    this.distanceField = this.gl.createTexture();
    this.gl.bindTexture(this.gl.TEXTURE_2D, this.distanceField);

    // Set up texture parameters
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_WRAP_S, this.gl.CLAMP_TO_EDGE);
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_WRAP_T, this.gl.CLAMP_TO_EDGE);
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_MIN_FILTER, this.gl.LINEAR);
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_MAG_FILTER, this.gl.LINEAR);

    // Load the SDF image
    const image = new Image();
    image.onload = () => {
      this.gl.bindTexture(this.gl.TEXTURE_2D, this.distanceField);
      this.gl.texImage2D(this.gl.TEXTURE_2D, 0, this.gl.RGBA, this.gl.RGBA, this.gl.UNSIGNED_BYTE, image);
    };

    image.src = catSdfUrl;
  }

  render() {
    const currentTime = Date.now();
    const time = (currentTime - this.startTime) / 1000.0;

    this.canvas.width = this.canvas.offsetWidth * window.devicePixelRatio;
    this.canvas.height = this.canvas.offsetHeight * window.devicePixelRatio;

    this.gl.viewport(0, 0, this.canvas.width, this.canvas.height);

    // Clear the canvas
    this.gl.clearColor(0.0, 0.0, 0.0, 0.0);
    this.gl.clear(this.gl.COLOR_BUFFER_BIT);

    // Set uniforms
    const timeLocation = this.gl.getUniformLocation(this.program, `time`);
    this.gl.uniform1f(timeLocation, time);

    const resolutionLocation = this.gl.getUniformLocation(this.program, `resolution`);
    this.gl.uniform2f(resolutionLocation, this.canvas.width, this.canvas.height);

    const textureSizeLocation = this.gl.getUniformLocation(this.program, `textureSize`);
    this.gl.uniform2f(textureSizeLocation, this.textureWidth, this.textureHeight);

    const falloffLocation = this.gl.getUniformLocation(this.program, `falloff`);
    this.gl.uniform1f(falloffLocation, this.falloff);

    const fbmsubstractLocation = this.gl.getUniformLocation(this.program, `fbmsubstract`);
    this.gl.uniform1f(fbmsubstractLocation, this.fbmsubstract);

    // Bind textures
    if (this.texture) {
      this.gl.activeTexture(this.gl.TEXTURE0);
      this.gl.bindTexture(this.gl.TEXTURE_2D, this.texture);
      const textureLocation = this.gl.getUniformLocation(this.program, `tex`);
      this.gl.uniform1i(textureLocation, 0);
    }

    if (this.distanceField) {
      this.gl.activeTexture(this.gl.TEXTURE1);
      this.gl.bindTexture(this.gl.TEXTURE_2D, this.distanceField);
      const distanceFieldLocation = this.gl.getUniformLocation(this.program, `distanceField`);
      this.gl.uniform1i(distanceFieldLocation, 1);
    }

    // Draw
    this.gl.drawArrays(this.gl.TRIANGLES, 0, 6);
  }
}

type ShaderRef = {
  canvas: HTMLCanvasElement;
  context: WebGL2RenderingContext;
  shader: ShaderController;
};

export function CatShader() {
  const shaderRef = useRef<ShaderRef | null>(null);

  const attachContext = useCallback((canvas: HTMLCanvasElement | null) => {
    if (!canvas) {
      shaderRef.current = null;
      return;
    }

    const context = canvas.getContext(`webgl2`, {alpha: true, premultipliedAlpha: true});
    if (!context) {
      shaderRef.current = null;
      return;
    }

    shaderRef.current = {
      canvas,
      context,
      shader: new ShaderController(canvas, context),
    };
  }, []);

  useEffect(() => {
    const shader = shaderRef.current?.shader;
    if (!shader)
      return () => {};

    shader.render();

    let renderLoop: ReturnType<typeof requestAnimationFrame>;

    const render = () => {
      shader.render();
      renderLoop = requestAnimationFrame(render);
    };

    render();

    return () => {
      cancelAnimationFrame(renderLoop);
    };
  }, []);

  useEffect(() => {
    const shader = shaderRef.current?.shader;
    if (!shader)
      return;

    shader.dispose();
  }, []);

  return (
    <canvas ref={attachContext} className={`w-full h-full`} />
  );
}
