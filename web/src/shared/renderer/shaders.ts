/** WebGL shader sources with section cut support. */

export const VERTEX_SHADER = `
  attribute vec3 aPosition;
  attribute vec3 aNormal;
  uniform mat4 uProjection;
  uniform mat4 uView;
  uniform mat4 uModel;
  varying vec3 vNormal;
  varying vec3 vPosition;
  void main() {
    vec4 worldPos = uModel * vec4(aPosition, 1.0);
    vPosition = worldPos.xyz;
    vNormal = mat3(uModel) * aNormal;
    gl_Position = uProjection * uView * worldPos;
  }
`;

export const FRAGMENT_SHADER = `
  precision mediump float;
  varying vec3 vNormal;
  varying vec3 vPosition;
  uniform vec4 uColor;
  uniform vec3 uLightDir;
  uniform vec3 uCameraPos;
  uniform float uClipZ;
  uniform bool uClipEnabled;
  uniform float uClipY;
  uniform bool uClipYEnabled;
  uniform float uHighlight;
  uniform vec3 uHighlightColor;
  uniform float uSpecular;
  uniform float uShininess;
  void main() {
    if (uClipEnabled && vPosition.z > uClipZ) discard;
    if (uClipYEnabled && vPosition.y > uClipY) discard;
    vec3 normal = normalize(vNormal);
    // Two-sided lighting
    vec3 viewDir = normalize(uCameraPos - vPosition);
    if (dot(normal, viewDir) < 0.0) normal = -normal;
    vec3 keyDir = normalize(uLightDir);
    float ambient = 0.45;
    float diffuse = max(dot(normal, keyDir), 0.0) * 0.35;
    // Fill light from below-left
    float fill = max(dot(normal, normalize(vec3(-0.5, -0.3, 0.4))), 0.0) * 0.15;
    float lighting = ambient + diffuse + fill;
    vec3 baseColor = uColor.rgb * lighting;
    // Blinn-Phong highlight from the IFC surface style, so a polished metal
    // reads differently from matte plaster even at a similar hue. Scaled down:
    // Revit writes specular 0.5 on nearly every material, which at full
    // strength washes large floors and glazing out to white.
    vec3 halfDir = normalize(keyDir + viewDir);
    float spec = 0.25 * uSpecular * pow(max(dot(normal, halfDir), 0.0), uShininess);
    baseColor += vec3(spec);
    // Blend with highlight color when selected
    vec3 finalColor = mix(baseColor, uHighlightColor * lighting + vec3(0.15), uHighlight * 0.45);
    // Glazing viewed edge-on reads more solid than face-on, which keeps very
    // transparent glass legible without hiding what is behind it.
    float alpha = uColor.a;
    if (alpha < 1.0) {
      float fresnel = pow(1.0 - max(dot(normal, viewDir), 0.0), 3.0);
      alpha = clamp(alpha + fresnel * (1.0 - alpha) * 0.6, 0.0, 1.0);
    }
    gl_FragColor = vec4(finalColor, alpha);
  }
`;

export interface ShaderLocations {
  aPosition: number;
  aNormal: number;
  uProjection: WebGLUniformLocation | null;
  uView: WebGLUniformLocation | null;
  uModel: WebGLUniformLocation | null;
  uColor: WebGLUniformLocation | null;
  uLightDir: WebGLUniformLocation | null;
  uCameraPos: WebGLUniformLocation | null;
  uClipZ: WebGLUniformLocation | null;
  uClipEnabled: WebGLUniformLocation | null;
  uClipY: WebGLUniformLocation | null;
  uClipYEnabled: WebGLUniformLocation | null;
  uHighlight: WebGLUniformLocation | null;
  uHighlightColor: WebGLUniformLocation | null;
  uSpecular: WebGLUniformLocation | null;
  uShininess: WebGLUniformLocation | null;
}

function createShader(gl: WebGLRenderingContext, type: number, source: string): WebGLShader | null {
  const shader = gl.createShader(type);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    console.error(gl.getShaderInfoLog(shader));
    gl.deleteShader(shader);
    return null;
  }
  return shader;
}

export function createShaderProgram(gl: WebGLRenderingContext): { program: WebGLProgram; locs: ShaderLocations } | null {
  const vs = createShader(gl, gl.VERTEX_SHADER, VERTEX_SHADER);
  const fs = createShader(gl, gl.FRAGMENT_SHADER, FRAGMENT_SHADER);
  if (!vs || !fs) return null;

  const program = gl.createProgram();
  if (!program) return null;
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);

  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    console.error(gl.getProgramInfoLog(program));
    return null;
  }

  const locs: ShaderLocations = {
    aPosition: gl.getAttribLocation(program, 'aPosition'),
    aNormal: gl.getAttribLocation(program, 'aNormal'),
    uProjection: gl.getUniformLocation(program, 'uProjection'),
    uView: gl.getUniformLocation(program, 'uView'),
    uModel: gl.getUniformLocation(program, 'uModel'),
    uColor: gl.getUniformLocation(program, 'uColor'),
    uLightDir: gl.getUniformLocation(program, 'uLightDir'),
    uCameraPos: gl.getUniformLocation(program, 'uCameraPos'),
    uClipZ: gl.getUniformLocation(program, 'uClipZ'),
    uClipEnabled: gl.getUniformLocation(program, 'uClipEnabled'),
    uClipY: gl.getUniformLocation(program, 'uClipY'),
    uClipYEnabled: gl.getUniformLocation(program, 'uClipYEnabled'),
    uHighlight: gl.getUniformLocation(program, 'uHighlight'),
    uSpecular: gl.getUniformLocation(program, 'uSpecular'),
    uShininess: gl.getUniformLocation(program, 'uShininess'),
    uHighlightColor: gl.getUniformLocation(program, 'uHighlightColor'),
  };

  return { program, locs };
}
