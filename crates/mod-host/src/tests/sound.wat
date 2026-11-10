(component
  (import "cinnabar:extension/sound-handling@0.1.0" (instance $types
    (type $p (record (field "x" f32) (field "y" f32) (field "z" f32)))
    (export "position" (type $position (eq $p)))
    (type $c (record (field "name" string) (field "position" (option $position))
      (field "looping" bool) (field "cancelled" bool)))
    (export "sound-context" (type (eq $c)))))
  (alias export $types "sound-context" (type $context))
  (import "cinnabar:extension/budget@0.1.0" (instance $budget
    (export "remaining-fuel" (func (result u64)))))
  (alias export $budget "remaining-fuel" (func $fuel))
  (core func $fuel-lowered (canon lower (func $fuel)))
  (core module $m
    (import "host" "fuel" (func $fuel (result i64)))
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 32768))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) (local $pointer i32)
      global.get $heap local.tee $pointer
      local.get 3 i32.const 7 i32.add i32.const -8 i32.and
      i32.add global.set $heap
      local.get $pointer)
    (func (export "init"))
    (func (export "frame"))
    ;; name, length, position tag, x, y, z, looping, cancelled
    (func (export "handle") (param i32 i32 i32 f32 f32 f32 i32 i32) (result i32)
      $BODY
      i32.const 512 local.get 0 i32.store
      i32.const 516 local.get 1 i32.store
      i32.const 520 local.get 2 i32.store8
      i32.const 524 local.get 3 f32.store
      i32.const 528 local.get 4 f32.store
      i32.const 532 local.get 5 f32.store
      i32.const 536 $LOOPING i32.store8
      i32.const 537 $CANCELLED i32.store8
      i32.const 512))
  (core instance $host (export "fuel" (func $fuel-lowered)))
  (core instance $i (instantiate $m (with "host" (instance $host))))
  (alias core export $i "memory" (core memory $memory))
  (alias core export $i "realloc" (core func $realloc))
  (func (export "init") (canon lift (core func $i "init")))
  (func (export "frame") (canon lift (core func $i "frame")))
  (func (export "handle-sound") (param "ctx" $context) (result $context)
    (canon lift (core func $i "handle") (memory $memory) (realloc $realloc))))
