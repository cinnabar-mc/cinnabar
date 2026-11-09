(component
  (import "$ITEM_USE" (instance $item-use
    (export "pulse-use" (func (result (result (error string)))))))
  (alias export $item-use "pulse-use" (func $pulse))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower (canon lower (func $pulse) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "pulse" (func $pulse (param i32)))
    (import "host" "memory" (memory 1))
    (func (export "init"))
    (func (export "frame") $FRAME))
  (core instance $host
    (export "pulse" (func $lower))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
