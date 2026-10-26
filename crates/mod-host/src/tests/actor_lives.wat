(component
  (import "$PLAYER_STATE" (instance $player-state
    (type $l (record (field "life" u64) (field "dead" bool)))
    (export "actor-life" (type $life (eq $l)))
    (export "read-actor-lives" (func (param "runtime-ids" (list u64))
      (result (result (list (option $life)) (error string)))))))
  (alias export $player-state "read-actor-lives" (func $read))
  (core module $memory-module
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.get 2 i32.add i32.const 1 i32.sub
      i32.const 0 local.get 2 i32.sub i32.and local.tee $old
      local.get 3 i32.add global.set $next local.get $old))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower (canon lower (func $read) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "read" (func $read (param i32 i32 i32)))
    (import "host" "memory" (memory 1))
    (func (export "init") $INIT)
    (func (export "frame") $FRAME))
  (core instance $host
    (export "read" (func $lower))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
