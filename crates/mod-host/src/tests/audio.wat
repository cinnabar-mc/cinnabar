(component
  (import "cinnabar:extension/audio@0.1.0" (instance $audio
    (export "set-muted-sounds" (func (param "names" (list string)) (result (result (error string)))))))
  (alias export $audio "set-muted-sounds" (func $set))
  (core module $memory-module
    (memory (export "memory") 1)
    (data (i32.const 0) "\00\04\00\00\0b\00\00\00")
    (data (i32.const 1024) "fixture.one")
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 32768))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower (canon lower (func $set) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "set" (func $set (param i32 i32 i32)))
    (import "host" "memory" (memory 1))
    (func $on
      i32.const 0 i32.const 1 i32.const 512 call $set
      i32.const 512 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    (func $off
      i32.const 0 i32.const 0 i32.const 512 call $set
      i32.const 512 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    (func (export "init") $INIT)
    (func (export "frame") $FRAME))
  (core instance $host (export "set" (func $lower)) (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
