(component
  (import "cinnabar:extension/input@0.1.0" (instance $input
    (export "read-native-editing" (func (result (result bool (error string)))))))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $editing (canon lower (func $input "read-native-editing") (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "editing" (func $editing (param i32)))
    (import "host" "memory" (memory 1))
    (func (export "init"))
    ;; The result's tag is at 256 and its bool payload at 260; a mismatch traps.
    (func (export "frame")
      i32.const 256 call $editing
      (if (i32.ne (i32.load8_u (i32.const 256)) (i32.const 0)) (then unreachable))
      (if (i32.ne (i32.load8_u (i32.const 260)) (i32.const $EXPECT)) (then unreachable))))
  (core instance $host (export "editing" (func $editing)) (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
