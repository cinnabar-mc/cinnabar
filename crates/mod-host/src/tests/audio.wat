(component
  (import "cinnabar:extension/sound-filter@0.1.0" (instance $types
    (type $p (record (field "x" f32) (field "y" f32) (field "z" f32)))
    (export "position" (type $position (eq $p)))
    (type $e (record (field "name" string) (field "position" (option $position)) (field "looping" bool)))
    (export "sound-event" (type (eq $e)))))
  (alias export $types "sound-event" (type $event))
  (type $events (list $event))
  (type $decisions (list bool))
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
    (func (export "filter") (param i32 i32) (result i32)
      $BODY
      i32.const 512 i32.const 1024 i32.store
      i32.const 516 $LENGTH i32.store
      i32.const 1024 i32.const 0 i32.store8
      i32.const 1025 i32.const 1 i32.store8
      i32.const 512))
  (core instance $host (export "fuel" (func $fuel-lowered)))
  (core instance $i (instantiate $m (with "host" (instance $host))))
  (alias core export $i "memory" (core memory $memory))
  (alias core export $i "realloc" (core func $realloc))
  (func (export "init") (canon lift (core func $i "init")))
  (func (export "frame") (canon lift (core func $i "frame")))
  (func (export "filter-sounds") (param "events" $events) (result $decisions)
    (canon lift (core func $i "filter") (memory $memory) (realloc $realloc))))
