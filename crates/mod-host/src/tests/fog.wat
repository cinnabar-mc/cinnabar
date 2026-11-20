(component
  (import "$ENVIRONMENT" (instance $environment
    (type $mediums-type (flags "air" "water" "lava"))
    (export "fog-mediums" (type $mediums (eq $mediums-type)))
    (export "set-fog-suppression" (func (param "mediums" $mediums) (result (result (error string)))))))
  (alias export $environment "set-fog-suppression" (func $fog))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 32768))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-fog (canon lower (func $fog) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "fog" (func $fog (param i32 i32)))
    (import "host" "memory" (memory 1))
    ;; Sets the suppressed mediums to a bit mask: air 1, water 2, lava 4.
    (func $set (param i32)
      local.get 0 i32.const 512 call $fog
      i32.const 512 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    (func (export "init") $INIT)
    (func (export "frame") $FRAME))
  (core instance $host
    (export "fog" (func $lower-fog))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
