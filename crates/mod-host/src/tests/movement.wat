(component
  (import "cinnabar:extension/gameplay@0.1.0" (instance $gameplay
    (type $v (record (field "x" f32) (field "y" f32) (field "z" f32)))
    (export "vector3" (type $vector3 (eq $v)))
    (type $m (record (field "session" u64) (field "dimension" s32) (field "tick" u64)
      (field "velocity" $vector3) (field "on-ground" bool)
       (field "eligible" bool) (field "knockback-sequence" u64)))
    (export "movement-snapshot" (type $movement (eq $m)))
    (export "read-movement" (func (result (result (option $movement) (error string)))))))
  (alias export $gameplay "read-movement" (func $read))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower (canon lower (func $read) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "read" (func $read (param i32)))
    (import "host" "memory" (memory 1))
    (func (export "init"))
    (func (export "frame")
      i32.const 128 call $read
      i32.const 128 i32.load8_u if unreachable end
      i32.const 136 i32.load8_u i32.const 1 i32.ne if unreachable end
      i32.const 144 i64.load i64.const 1 i64.ne if unreachable end
      i32.const 180 i32.load8_u i32.const 1 i32.ne if unreachable end
      i32.const 181 i32.load8_u i32.const 1 i32.ne if unreachable end))
  (core instance $host (export "read" (func $lower)) (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
