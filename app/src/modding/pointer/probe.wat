(component
  (import "cinnabar:extension/input@0.1.0" (instance $input
    (export "position-pointer" (func (param "x" f32) (param "y" f32)
      (result (result (error string)))))))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $position (canon lower (func $input "position-pointer")
    (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "position" (func $position (param f32 f32 i32)))
    (import "host" "memory" (memory 1))
    (global $done (mut i32) (i32.const 0))
    (func (export "init"))
    (func (export "frame")
      global.get $done i32.eqz
      if
        f32.const 0.5 f32.const 0.5 i32.const 256 call $position
        i32.const 256 i32.load8_u i32.eqz
        if i32.const 1 global.set $done end
      end))
  (core instance $host (export "position" (func $position)) (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
