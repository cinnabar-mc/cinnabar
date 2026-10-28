package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
)

// Active output paths remain available to the source adapter's existing command-line surface.
const (
	v2225PhysicsOutputPath    = "crates/assets/data/block-physics-seed-v2225.bin"
	v2225PhysicsBREGInputPath = v2225BlockOutputPath
	v2225BiomeOutputPath      = "crates/assets/data/biome-registry-v2225.bin"
	v2225BiomeAllowlistPath   = "crates/protocol/data/retail_biomes_1_26_60.txt"
	v2225BiomeProjectionPath  = "assets/biome-projection-v2225.json"
)

// compilerRoot finds the checkout containing both the source module and the Rust compiler.
func compilerRoot() (string, error) {
	dir, err := os.Getwd()
	if err != nil {
		return "", err
	}
	for {
		if _, err := os.Stat(filepath.Join(dir, "assets", "bedrock-target.json")); err == nil {
			return dir, nil
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return "", fmt.Errorf("registry compiler checkout not found")
		}
		dir = parent
	}
}

// compilerPath preserves the Go command's working-directory-relative path interpretation.
func compilerPath(path string) string {
	if path == "" {
		return ""
	}
	absolute, err := filepath.Abs(path)
	if err != nil {
		return path
	}
	return absolute
}

// runCompiler exports only library facts, then delegates the selected product pass to Rust.
func runCompiler(args ...string) error {
	root, err := compilerRoot()
	if err != nil {
		return err
	}
	scratch, err := os.CreateTemp("", "registry-facts-*.json")
	if err != nil {
		return err
	}
	path := scratch.Name()
	if err := scratch.Close(); err != nil {
		os.Remove(path)
		return err
	}
	defer os.Remove(path)
	if err := exportCompilerFacts(path); err != nil {
		return err
	}
	flags := append([]string{"--root", root, "--facts", path}, args...)
	var command *exec.Cmd
	if binary := os.Getenv("CINNABAR_REGISTRYC_BIN"); binary != "" {
		command = exec.Command(binary, flags...)
	} else {
		command = exec.Command("cargo", append([]string{"run", "--locked", "-p", "asset-compiler", "--bin", "registryc", "--"}, flags...)...)
	}
	command.Dir = root
	output, err := command.CombinedOutput()
	if err != nil {
		return fmt.Errorf("registry compiler: %w: %s", err, output)
	}
	return nil
}

// writeV2225BlockProjection delegates admission and carrier construction to asset-compiler.
func writeV2225BlockProjection(legacy, allowed, out, light, manifest string) error {
	return runCompiler("--kind", "blocks", "--legacy", compilerPath(legacy), "--allowlist", compilerPath(allowed), "--out", compilerPath(out), "--light-out", compilerPath(light), "--manifest", compilerPath(manifest))
}

// writeV2225PhysicsProjection delegates movement policy and seed construction to asset-compiler.
func writeV2225PhysicsProjection(breg, out, sha, manifest string) error {
	args := []string{"--kind", "physics", "--breg", compilerPath(breg), "--out", compilerPath(out)}
	if sha != "" {
		args = append(args, "--sha", compilerPath(sha))
	}
	if manifest != "" {
		args = append(args, "--manifest", compilerPath(manifest))
	}
	return runCompiler(args...)
}

// writeV2225BiomeProjection delegates retail admission and biome construction to asset-compiler.
func writeV2225BiomeProjection(allowed, out, manifest string) error {
	return runCompiler("--kind", "biomes", "--allowlist", compilerPath(allowed), "--out", compilerPath(out), "--manifest", compilerPath(manifest))
}
