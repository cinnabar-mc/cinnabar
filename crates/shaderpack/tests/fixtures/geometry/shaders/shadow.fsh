#version 450 compatibility
layout(location = 1) in vec4 expandedTint;
flat in int lastCorner;
in vec2 inheritedPadding;
void main() { gl_FragData[0] = expandedTint + vec4(0.0, 0.0, float(lastCorner) * 0.5 * inheritedPadding.y * (1.0 - inheritedPadding.x), 0.0); }
