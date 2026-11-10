(component
  (import "$INVENTORY" (instance $inventory
    (export "swap-offhand" (func (param "slot" u8) (result (result (error string)))))))
  (alias export $inventory "swap-offhand" (func $swap))
  (core module $mem
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.tee $old local.get 3 i32.add global.set $next local.get $old))
  (core instance $memory (instantiate $mem))
  (alias core export $memory "memory" (core memory $m))
  (alias core export $memory "realloc" (core func $realloc))
  (core func $lower (canon lower (func $swap) (memory $m) (realloc $realloc)))
  (core module $code
    (import "host" "swap" (func $swap (param i32 i32)))
    (import "host" "memory" (memory 1))
    (func (export "init"))
    (func (export "frame")
      i32.const 20 i32.const 0 call $swap
      i32.const 0 i32.load if unreachable end))
  (core instance $imports (export "swap" (func $lower)) (export "memory" (memory $m)))
  (core instance $run (instantiate $code (with "host" (instance $imports))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
