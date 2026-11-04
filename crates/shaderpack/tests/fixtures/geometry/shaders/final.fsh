#version 330 compatibility
uniform sampler2D colortex0;
in vec2 frameUv;
void main() { gl_FragData[0] = texture2D(colortex0, frameUv); }
