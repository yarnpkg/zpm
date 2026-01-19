#version 300 es
precision highp float;

in float v_brightness;
in float v_depth;

out vec4 fragColor;

void main() {
  // Create circular point with soft edges
  vec2 coord = gl_PointCoord - vec2(0.5);
  float dist = length(coord);

  // Soft circular falloff
  float alpha = 1.0 - smoothstep(0.3, 0.5, dist);

  // Add glow effect
  float glow = exp(-dist * 4.0) * 0.5;
  alpha += glow;

  // Apply brightness and depth-based fade
  float depthFade = 1.0 - v_depth * 0.5;
  alpha *= v_brightness * depthFade;

  // Slight blue-white tint for stars
  vec3 starColor = mix(vec3(0.8, 0.9, 1.0), vec3(1.0), v_brightness);

  fragColor = vec4(starColor, alpha * 0.8);
}
