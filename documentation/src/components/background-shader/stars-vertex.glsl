#version 300 es
precision highp float;

in vec3 a_position;
in float a_size;
in float a_brightness;

uniform mat4 u_viewProjection;
uniform float u_time;
uniform vec2 u_resolution;

out float v_brightness;
out float v_depth;

mat3 rotateY(float angle) {
  float s = sin(angle);
  float c = cos(angle);
  return mat3(c, 0, s, 0, 1, 0, -s, 0, c);
}

mat3 rotateX(float angle) {
  float s = sin(angle);
  float c = cos(angle);
  return mat3(1, 0, 0, 0, c, -s, 0, s, c);
}

void main() {
  float speed = 0.05;
  float angle = u_time * speed;

  vec3 rotatedPos = rotateY(angle) * rotateX(angle * 0.7) * a_position;

  vec4 pos = u_viewProjection * vec4(rotatedPos, 1.0);
  gl_Position = pos;

  // Size attenuation based on depth
  float depth = pos.z / pos.w;
  v_depth = depth;

  // Point size with perspective
  float baseSize = a_size * u_resolution.y / 800.0;
  gl_PointSize = baseSize * (1.0 - depth * 0.3);

  v_brightness = a_brightness;
}
