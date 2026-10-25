#version 330 compatibility
in vec4 surfaceTint;
void main() { gl_FragData[0] = surfaceTint; }
