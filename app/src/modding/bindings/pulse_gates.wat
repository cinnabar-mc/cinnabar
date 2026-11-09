(component
  (import "cinnabar:extension/gameplay@0.1.0" (instance $gameplay
    (export "pulse-attack" (func (result (result (error string)))))))
  (import "cinnabar:extension/item-use@0.1.0" (instance $item-use
    (export "pulse-use" (func (result (result (error string)))))))
  (alias export $gameplay "pulse-attack" (func $attack))
  (alias export $item-use "pulse-use" (func $use))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-attack (canon lower (func $attack) (memory $memory) (realloc $realloc)))
  (core func $lower-use (canon lower (func $use) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "attack" (func $attack (param i32)))
    (import "host" "use" (func $use (param i32)))
    (import "host" "memory" (memory 1))
    (func (export "init"))
    (func (export "frame")
      i32.const 128 call $attack
      i32.const 128 i32.load8_u i32.const $ATTACK_DENIED i32.ne if unreachable end
      i32.const 256 call $use
      i32.const 256 i32.load8_u i32.const $USE_DENIED i32.ne if unreachable end))
  (core instance $host
    (export "attack" (func $lower-attack))
    (export "use" (func $lower-use))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
